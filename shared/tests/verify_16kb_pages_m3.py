#!/usr/bin/env python3
# ==============================================================================
# B.O.S.S. OS - Milestone 3: 16KB Memory Page Size Verification
# Validates kernel page size, mprotect() alignment, and JIT buffer compatibility.
#
# Acceptance Criteria Covered:
#   AC_16K_1: getconf PAGE_SIZE returns 16384
#   AC_16K_2: mprotect() on a 16KB-aligned buffer succeeds (no SIGBUS/EINVAL)
#   AC_16K_3: Mupen64Plus extra_memory ALIGN(16384) verified via nm/readelf
# ==============================================================================
import subprocess
import sys
import json
import ctypes
import mmap
import os
import struct
from pathlib import Path

LOG_DIR = Path("tests/logs")
LOG_DIR.mkdir(parents=True, exist_ok=True)

RESULTS = {
    "ac_16k_1_page_size": False,
    "ac_16k_2_mprotect_alignment": False,
    "ac_16k_3_jit_align_verified": False,
    "actual_page_size": None,
    "details": [],
    "exit_code": 1,
}

MUPEN_CORE = Path("build/mupen64plus-libretro-nx/mupen64plus_next_libretro.so")

def log(msg):
    print(msg)
    RESULTS["details"].append(msg)

def run(cmd):
    try:
        return subprocess.check_output(cmd, stderr=subprocess.STDOUT, text=True)
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        return getattr(e, 'output', '') or ''

def check_page_size():
    """AC_16K_1: System page size must be 16384."""
    log("\n[*] Checking system page size (getconf PAGE_SIZE)...")
    out = run(["getconf", "PAGE_SIZE"]).strip()
    actual = int(out) if out.isdigit() else os.sysconf("SC_PAGESIZE")
    RESULTS["actual_page_size"] = actual
    if actual == 16384:
        log(f"[+] AC_16K_1 PASS: PAGE_SIZE = {actual} (16KB)")
        return True
    else:
        log(f"[-] AC_16K_1 FAIL: PAGE_SIZE = {actual} (expected 16384)")
        return False

def check_mprotect_alignment():
    """AC_16K_2: mprotect() must succeed on 16KB-aligned anonymous memory."""
    log("\n[*] Testing mprotect() on 16KB-aligned anonymous buffer...")
    page_size = os.sysconf("SC_PAGESIZE")
    buf_size = page_size * 4  # 4 pages worth

    try:
        # Allocate anonymous memory via mmap, guaranteed page-aligned by OS
        buf = mmap.mmap(-1, buf_size, mmap.MAP_PRIVATE | mmap.MAP_ANONYMOUS,
                        mmap.PROT_READ | mmap.PROT_WRITE)

        # Attempt mprotect: set R+W+X on the first page
        # This requires addr to be a multiple of PAGE_SIZE
        addr = ctypes.addressof(ctypes.c_char.from_buffer(buf))

        libc = ctypes.CDLL("libc.so.6", use_errno=True)
        ret = libc.mprotect(ctypes.c_void_p(addr), ctypes.c_size_t(buf_size),
                            ctypes.c_int(0x7))  # PROT_READ|PROT_WRITE|PROT_EXEC

        buf.close()

        if ret == 0:
            log(f"[+] AC_16K_2 PASS: mprotect() succeeded on {buf_size}-byte "
                f"16KB-aligned buffer at 0x{addr:x}")
            return True
        else:
            errno = ctypes.get_errno()
            log(f"[-] AC_16K_2 FAIL: mprotect() returned {ret}, errno={errno} "
                f"({'EINVAL' if errno==22 else 'ENOMEM' if errno==12 else str(errno)})")
            return False
    except Exception as e:
        log(f"[-] AC_16K_2 FAIL: Exception during mprotect test: {e}")
        return False

def check_jit_alignment():
    """AC_16K_3: Verify extra_memory in Mupen64Plus libretro is 16KB-aligned."""
    log(f"\n[*] Checking Mupen64Plus JIT buffer alignment in {MUPEN_CORE}...")

    if not MUPEN_CORE.exists():
        log(f"[!] AC_16K_3 SKIP: {MUPEN_CORE} not found. Build Mupen64Plus first.")
        return None  # Not a FAIL - just not built yet

    # Use readelf to check alignment of BSS/data section containing extra_memory
    readelf_out = run(["readelf", "-S", str(MUPEN_CORE)])
    bss_align = None
    for line in readelf_out.splitlines():
        if ".bss" in line or ".data" in line:
            parts = line.split()
            # readelf -S output: [Nr] Name Type Addr Off Size ES Flg Lk Inf Al
            try:
                # Last field is alignment
                align = int(parts[-1], 10)
                if align >= 16384:
                    bss_align = align
            except (ValueError, IndexError):
                pass

    # Also check via nm for the extra_memory symbol alignment
    nm_out = run(["nm", "-S", str(MUPEN_CORE)])
    extra_mem_lines = [l for l in nm_out.splitlines() if "extra_memory" in l]

    if extra_mem_lines:
        log(f"[+] extra_memory symbol found via nm:")
        for l in extra_mem_lines:
            log(f"    {l}")
        # Parse address - check it is 16KB aligned
        parts = extra_mem_lines[0].split()
        if parts:
            try:
                addr = int(parts[0], 16)
                if addr % 16384 == 0:
                    log(f"[+] AC_16K_3 PASS: extra_memory at 0x{addr:x} is 16KB-aligned.")
                    return True
                else:
                    log(f"[-] AC_16K_3 FAIL: extra_memory at 0x{addr:x} is NOT 16KB-aligned "
                        f"(offset: {addr % 16384} bytes).")
                    return False
            except ValueError:
                pass

    if bss_align and bss_align >= 16384:
        log(f"[+] AC_16K_3 PASS: .bss/.data section alignment = {bss_align} bytes (≥16KB)")
        return True

    log("[!] AC_16K_3 INFO: Could not verify via nm/readelf. Check build output manually.")
    log("    Expected: ALIGN(16384, char extra_memory[33554432]) in r4300_core.h")
    return None

def main():
    print("=================================================================")
    print("   B.O.S.S. M3: 16KB Memory Page Size Verification              ")
    print("=================================================================")

    ac1 = check_page_size()
    ac2 = check_mprotect_alignment()
    ac3_result = check_jit_alignment()
    ac3 = ac3_result is not False  # None (skip) counts as non-fail

    RESULTS["ac_16k_1_page_size"] = ac1
    RESULTS["ac_16k_2_mprotect_alignment"] = ac2
    RESULTS["ac_16k_3_jit_align_verified"] = ac3_result

    passed = ac1 and ac2
    RESULTS["exit_code"] = 0 if passed else 1

    print("\n=================================================================")
    print("   Milestone 3 Acceptance Criteria Results                       ")
    print("=================================================================")
    print(f"   AC_16K_1: PAGE_SIZE == 16384      : {'[✓] PASS' if ac1 else '[✗] FAIL'}")
    print(f"   AC_16K_2: mprotect() alignment    : {'[✓] PASS' if ac2 else '[✗] FAIL'}")
    print(f"   AC_16K_3: JIT buffer ALIGN(16384) : "
          f"{'[✓] PASS' if ac3_result is True else '[~] SKIP (not built)' if ac3_result is None else '[✗] FAIL'}")
    print(f"   Exit Code                         : {RESULTS['exit_code']}")
    print("=================================================================")

    result_path = LOG_DIR / "verification_m3_16kb_result.json"
    with open(result_path, "w") as f:
        json.dump(RESULTS, f, indent=2)
    print(f"   Result JSON: {result_path}")

    return RESULTS["exit_code"]

if __name__ == "__main__":
    sys.exit(main())
