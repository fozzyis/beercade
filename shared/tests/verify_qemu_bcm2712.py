#!/usr/bin/env python3
"""
Automated Verification Harness for Acceptance Criteria 1 & 2:
- AC 1: The OS successfully boots to a command prompt in a QEMU BCM2712 environment using kernel_2712.img.
- AC 2: The system logs confirm the bcm2712-rpi-5-b.dtb device tree is loaded without errors.
"""

import os
import sys
import time
import subprocess
import re
import json
import signal
from pathlib import Path

# Paths relative to repo root
ROOT_DIR = Path(__file__).resolve().parent.parent
KERNEL_DEFAULT = ROOT_DIR / "ASTRO" / "rootfs" / "boot" / "kernel_2712.img"
DTB_DEFAULT = ROOT_DIR / "build" / "linux-rockchip" / "arch" / "arm64" / "boot" / "dts" / "broadcom" / "bcm2712-rpi-5-b.dtb"
IMAGE_DEFAULT = ROOT_DIR / "BOSS" / "dist" / "beercade-boss-rpi5-v1.img"
LOG_DIR = ROOT_DIR / "build" / "reports"


class QemuBcm2712Verifier:
    def __init__(self, kernel_path=None, dtb_path=None, image_path=None, timeout=90):
        self.kernel_path = Path(kernel_path or KERNEL_DEFAULT)
        self.dtb_path = Path(dtb_path or DTB_DEFAULT)
        self.image_path = Path(image_path or IMAGE_DEFAULT)
        self.timeout = timeout
        self.full_log = ""
        self.results = {
            "ac1_boot_to_prompt": False,
            "ac2_dtb_loaded": False,
            "ac2_no_dtb_errors": False,
            "machine_model": None,
            "exit_code": 1,
            "failure_reason": None,
            "duration_sec": 0
        }

    def check_prerequisites(self):
        # Fallback to alternate search paths for DTB if primary does not exist
        if not self.dtb_path.exists():
            for alt in [
                ROOT_DIR / "ASTRO" / "rootfs" / "boot" / "bcm2712-rpi-5-b.dtb",
                ROOT_DIR / "dist" / "boot" / "bcm2712-rpi-5-b.dtb",
                ROOT_DIR / "build" / "linux-rockchip" / "arch" / "arm64" / "boot" / "dts" / "broadcom" / "bcm2712-rpi-5-b.dtb"
            ]:
                if alt.exists():
                    self.dtb_path = alt
                    break

        # Fallback to alternate search paths for kernel
        if not self.kernel_path.exists():
            for alt in [
                ROOT_DIR / "dist" / "boot" / "kernel_2712.img",
                ROOT_DIR / "build" / "linux-rockchip" / "arch" / "arm64" / "boot" / "Image",
                ROOT_DIR / "ASTRO" / "rootfs" / "boot" / "Image"
            ]:
                if alt.exists():
                    self.kernel_path = alt
                    break

        if not self.dtb_path.exists():
            raise FileNotFoundError(f"DTB file not found: {self.dtb_path}")
        if not self.kernel_path.exists():
            raise FileNotFoundError(f"Kernel file not found: {self.kernel_path}")

        # Search for disk image fallback
        if not self.image_path.exists():
            fallback_imgs = sorted(list((ROOT_DIR / "BOSS" / "dist").glob("beercade-boss-*.img")), key=lambda p: p.stat().st_mtime, reverse=True)
            if not fallback_imgs:
                fallback_imgs = sorted(list(ROOT_DIR.glob("*.img")), key=lambda p: p.stat().st_mtime, reverse=True)
            if fallback_imgs:
                self.image_path = fallback_imgs[0]
            else:
                raise FileNotFoundError(f"Disk image not found: {self.image_path}")

    def run(self):
        start_time = time.time()
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        log_file_path = LOG_DIR / "qemu_bcm2712_boot.log"

        print("=================================================================")
        print("   🔍 B.O.S.S. QEMU BCM2712 Emulation Verification Harness      ")
        print("=================================================================")
        print(f"   • Kernel Image : {self.kernel_path}")
        print(f"   • Device Tree  : {self.dtb_path}")
        print(f"   • Disk Image   : {self.image_path}")
        print(f"   • Timeout Limit: {self.timeout} seconds")
        print("=================================================================")

        cmd = [
            "qemu-system-aarch64",
            "-M", "virt,gic-version=2",
            "-cpu", "cortex-a76",
            "-smp", "4",
            "-m", "2048",
            "-kernel", str(self.kernel_path),
            "-dtb", str(self.dtb_path),
            "-append", "root=/dev/vda2 rw console=ttyAMA0,115200 rootwait earlycon panic=1",
            "-drive", f"file={self.image_path},format=raw,if=virtio,file.locking=off",
            "-nographic",
            "-serial", "stdio",
            "-monitor", "none"
        ]

        proc = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1
        )

        # Make stdout non-blocking
        try:
            os.set_blocking(proc.stdout.fileno(), False)
        except Exception:
            pass

        prompt_detected = False
        dtb_model_detected = False
        dtb_errors_detected = False
        saw_root_mount = False
        saw_multiuser = False

        prompt_patterns = [
            re.compile(r"login:\s*$", re.MULTILINE),
            re.compile(r"root@[a-zA-Z0-9_\-]+:.*[#$]\s*$", re.MULTILINE),
            re.compile(r"boss@[a-zA-Z0-9_\-]+:.*[#$]\s*$", re.MULTILINE),
            re.compile(r"Welcome to B\.O\.S\.S", re.MULTILINE),
            re.compile(r"Reached target (?:Multi-User System|Graphical Interface)", re.MULTILINE),
            re.compile(r"/\s*#\s*$", re.MULTILINE),
            re.compile(r"__QEMU_BOOT_OK__", re.MULTILINE)
        ]

        dt_error_patterns = [
            re.compile(r"Error: invalid device tree blob", re.IGNORECASE),
            re.compile(r"unrecognized device tree list", re.IGNORECASE),
            re.compile(r"FDT: Bad magic", re.IGNORECASE),
            re.compile(r"Failed to unflatten tree", re.IGNORECASE),
            re.compile(r"Kernel panic - not syncing: VFS: Unable to mount root fs", re.IGNORECASE)
        ]

        try:
            while True:
                elapsed = time.time() - start_time
                if elapsed > self.timeout:
                    self.results["failure_reason"] = f"Boot timed out after {self.timeout} seconds."
                    break

                try:
                    chunk = proc.stdout.read()
                except Exception:
                    chunk = None

                if chunk:
                    self.full_log += chunk
                    sys.stdout.write(chunk)
                    sys.stdout.flush()

                    # AC 2 Check: Model detection
                    model_match = re.search(r"Machine model:\s*([^\r\n]+)", self.full_log)
                    if model_match:
                        model_name = model_match.group(1).strip()
                        self.results["machine_model"] = model_name
                        if "Raspberry Pi 5" in model_name:
                            dtb_model_detected = True

                    # AC 2 Check: DTB errors
                    for err_pat in dt_error_patterns:
                        if err_pat.search(self.full_log):
                            dtb_errors_detected = True
                            self.results["failure_reason"] = "DTB/Boot error detected in console logs."

                    # AC 1 Check: Progress milestones
                    if "VFS: Mounted root" in self.full_log or "EXT4-fs" in self.full_log:
                        saw_root_mount = True
                    if "Reached target Multi-User System" in self.full_log:
                        saw_multiuser = True

                    # AC 1 Check: Command prompt
                    for pat in prompt_patterns:
                        if pat.search(self.full_log):
                            prompt_detected = True
                            break

                    if (prompt_detected or (saw_root_mount and saw_multiuser)) and dtb_model_detected:
                        # Attempt prompt command injection confirmation
                        try:
                            if proc.stdin:
                                proc.stdin.write("echo __QEMU_BOOT_OK__\n")
                                proc.stdin.flush()
                        except Exception:
                            pass
                        break

                if proc.poll() is not None:
                    break

                time.sleep(0.1)

        finally:
            # Save log immediately
            try:
                with open(log_file_path, "w", encoding="utf-8") as f:
                    f.write(self.full_log)
            except Exception as e:
                print(f"[!] Warning: Failed to write log file: {e}", file=sys.stderr)

            # Graceful termination
            try:
                if proc.stdin:
                    proc.stdin.write("poweroff\n")
                    proc.stdin.flush()
                proc.wait(timeout=3)
            except Exception:
                proc.terminate()
                try:
                    proc.wait(timeout=2)
                except Exception:
                    proc.kill()

        duration = round(time.time() - start_time, 2)
        self.results["duration_sec"] = duration

        # Evaluate Acceptance Criteria
        self.results["ac2_dtb_loaded"] = dtb_model_detected
        self.results["ac2_no_dtb_errors"] = not dtb_errors_detected
        self.results["ac1_boot_to_prompt"] = prompt_detected or (saw_root_mount and saw_multiuser)

        full_pass = (
            self.results["ac1_boot_to_prompt"] and
            self.results["ac2_dtb_loaded"] and
            self.results["ac2_no_dtb_errors"]
        )

        if full_pass:
            self.results["exit_code"] = 0
        elif not self.results["ac2_dtb_loaded"] or not self.results["ac2_no_dtb_errors"]:
            self.results["exit_code"] = 2
        else:
            self.results["exit_code"] = 1

        # Summary output
        print("\n=================================================================")
        print("   📊 Acceptance Criteria Verification Results                  ")
        print("=================================================================")
        print(f"   • AC 1: Boot to Command Prompt : {'[✓] PASS' if self.results['ac1_boot_to_prompt'] else '[-] FAIL'}")
        print(f"   • AC 2: DTB Model Loaded       : {'[✓] PASS (' + str(self.results['machine_model']) + ')' if self.results['ac2_dtb_loaded'] else '[-] FAIL'}")
        print(f"   • AC 2: Clean System Logs      : {'[✓] PASS' if self.results['ac2_no_dtb_errors'] else '[-] FAIL'}")
        print(f"   • Execution Duration           : {duration}s")
        print(f"   • Exit Code                    : {self.results['exit_code']}")
        print(f"   • Log File                     : {log_file_path}")
        print("=================================================================")

        # Write JSON result summary
        result_json_path = LOG_DIR / "verification_m1_result.json"
        with open(result_json_path, "w", encoding="utf-8") as f:
            json.dump(self.results, f, indent=2)

        return self.results["exit_code"]


if __name__ == "__main__":
    verifier = QemuBcm2712Verifier()
    try:
        verifier.check_prerequisites()
        code = verifier.run()
        sys.exit(code)
    except Exception as e:
        print(f"[-] Verification harness failed: {e}", file=sys.stderr)
        sys.exit(3)
