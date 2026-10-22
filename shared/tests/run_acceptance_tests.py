#!/usr/bin/env python3
# ==============================================================================
# B.O.S.S. OS - Milestone 5: End-to-End Acceptance Test Runner
# Orchestrates all milestone verification scripts and produces a final
# pass/fail verdict against the 8 acceptance criteria.
#
# Usage:
#   python3 tests/run_acceptance_tests.py [--qemu] [--hardware]
#
# Acceptance Criteria (from ORIGINAL_REQUEST.md):
#   AC1:  OS boots to command prompt using kernel_2712.img (QEMU)
#   AC2:  bcm2712-rpi-5-b.dtb loaded without errors
#   AC3:  RP1 enumerated as PCIe device (lspci shows 1de4:0001)
#   AC4:  RP1 interrupts routed to BCM2712 GIC
#   AC5:  getconf PAGE_SIZE returns 16384
#   AC6:  User-space test suite passes (no segfault / alignment errors)
#   AC7:  Mesa V3D driver initializes VideoCore VII GPU
#   AC8:  pisp_be probes as V4L2 device
# ==============================================================================
import subprocess
import sys
import json
import argparse
import os
from pathlib import Path
from datetime import datetime, timezone

ROOT_DIR = Path(__file__).parent.parent
TESTS_DIR = ROOT_DIR / "tests"
LOG_DIR = TESTS_DIR / "logs"
LOG_DIR.mkdir(parents=True, exist_ok=True)

TIMESTAMP = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")

VERIFICATION_SCRIPTS = [
    ("M1 BCM2712 Boot",       TESTS_DIR / "verify_qemu_bcm2712.py"),
    ("M2 RP1 PCIe/MSI-X",    TESTS_DIR / "verify_rp1_pcie_m2.py"),
    ("M3 16KB Page Size",     TESTS_DIR / "verify_16kb_pages_m3.py"),
    ("M4 VideoCore VII/PiSP", TESTS_DIR / "verify_videocore_pisp_m4.py"),
]

RESULTS = {
    "timestamp": TIMESTAMP,
    "milestones": {},
    "acceptance_criteria": {
        "AC1_boot_to_prompt":      {"milestone": "M1", "pass": False},
        "AC2_dtb_loaded":          {"milestone": "M1", "pass": False},
        "AC3_rp1_pcie_enum":       {"milestone": "M2", "pass": False},
        "AC4_rp1_irq_routing":     {"milestone": "M2", "pass": False},
        "AC5_page_size_16384":     {"milestone": "M3", "pass": False},
        "AC6_userspace_no_fault":  {"milestone": "M3", "pass": False},
        "AC7_v3d_initialized":     {"milestone": "M4", "pass": False},
        "AC8_pisp_be_v4l2":        {"milestone": "M4", "pass": False},
    },
    "total_pass": 0,
    "total_fail": 0,
    "verdict": "FAIL",
    "exit_code": 1,
}

def run_script(label, script_path, extra_args=None):
    """Run a verification script and capture its exit code and JSON result."""
    if not script_path.exists():
        print(f"  [!] Script not found: {script_path}")
        return None, 3

    cmd = [sys.executable, str(script_path)] + (extra_args or [])
    print(f"\n{'='*65}")
    print(f"  Running: {label}")
    print(f"{'='*65}")

    result = subprocess.run(cmd, cwd=str(ROOT_DIR))
    return None, result.returncode

def load_json_result(json_path):
    if json_path.exists():
        with open(json_path) as f:
            return json.load(f)
    return {}

def main():
    parser = argparse.ArgumentParser(description="B.O.S.S. Pi5 E2E Acceptance Test Runner")
    parser.add_argument("--qemu", action="store_true", help="Include QEMU boot verification (M1)")
    parser.add_argument("--hardware", action="store_true", help="Running on physical Pi 5 hardware")
    parser.add_argument("--skip-m1", action="store_true", help="Skip M1 QEMU boot test")
    args = parser.parse_args()

    print("=================================================================")
    print("   B.O.S.S. OS — Raspberry Pi 5 Migration Acceptance Test Suite  ")
    print(f"   Timestamp : {TIMESTAMP}                                       ")
    print(f"   Mode      : {'Hardware' if args.hardware else 'QEMU Emulation'}")
    print("=================================================================")

    # ── Run each milestone verification script ─────────────────────────
    for label, script in VERIFICATION_SCRIPTS:
        if args.skip_m1 and "M1" in label:
            print(f"\n  [~] Skipping {label} (--skip-m1 flag set)")
            RESULTS["milestones"][label] = {"skipped": True, "exit_code": 0}
            continue

        _, exit_code = run_script(label, script)
        RESULTS["milestones"][label] = {"exit_code": exit_code, "passed": exit_code == 0}

    # ── Load JSON results and map to acceptance criteria ───────────────
    m1 = load_json_result(LOG_DIR / "verification_m1_result.json")
    m2 = load_json_result(LOG_DIR / "verification_m2_rp1_result.json")
    m3 = load_json_result(LOG_DIR / "verification_m3_16kb_result.json")
    m4 = load_json_result(LOG_DIR / "verification_m4_gpu_pisp_result.json")

    RESULTS["acceptance_criteria"]["AC1_boot_to_prompt"]["pass"] = \
        bool(m1.get("ac1_boot_to_prompt", False))
    RESULTS["acceptance_criteria"]["AC2_dtb_loaded"]["pass"] = \
        bool(m1.get("ac2_dtb_loaded", False)) and bool(m1.get("ac2_no_dtb_errors", False))
    RESULTS["acceptance_criteria"]["AC3_rp1_pcie_enum"]["pass"] = \
        bool(m2.get("ac_rp1_1_pcie_enumerated", False))
    RESULTS["acceptance_criteria"]["AC4_rp1_irq_routing"]["pass"] = \
        bool(m2.get("ac_rp1_2_msix_routing", False))
    RESULTS["acceptance_criteria"]["AC5_page_size_16384"]["pass"] = \
        bool(m3.get("ac_16k_1_page_size", False))
    RESULTS["acceptance_criteria"]["AC6_userspace_no_fault"]["pass"] = \
        bool(m3.get("ac_16k_2_mprotect_alignment", False))
    RESULTS["acceptance_criteria"]["AC7_v3d_initialized"]["pass"] = \
        bool(m4.get("ac_gpu_1_v3d_initialized", False))
    RESULTS["acceptance_criteria"]["AC8_pisp_be_v4l2"]["pass"] = \
        bool(m4.get("ac_gpu_2_pisp_be_v4l2", False))

    # ── Tally results ──────────────────────────────────────────────────
    passed = sum(1 for v in RESULTS["acceptance_criteria"].values() if v["pass"])
    failed = len(RESULTS["acceptance_criteria"]) - passed
    RESULTS["total_pass"] = passed
    RESULTS["total_fail"] = failed
    RESULTS["verdict"] = "PASS" if failed == 0 else "FAIL"
    RESULTS["exit_code"] = 0 if failed == 0 else 1

    # ── Final Report ───────────────────────────────────────────────────
    print("\n")
    print("=================================================================")
    print("   B.O.S.S. Pi5 Migration — Final Acceptance Criteria Report     ")
    print("=================================================================")

    AC_LABELS = {
        "AC1_boot_to_prompt":     "AC1  Boot to prompt (kernel_2712.img)",
        "AC2_dtb_loaded":         "AC2  bcm2712-rpi-5-b.dtb loaded clean",
        "AC3_rp1_pcie_enum":      "AC3  RP1 PCIe enumerated (1de4:0001)",
        "AC4_rp1_irq_routing":    "AC4  RP1 MSI-X → BCM2712 GIC routing",
        "AC5_page_size_16384":    "AC5  PAGE_SIZE == 16384",
        "AC6_userspace_no_fault": "AC6  mprotect/JIT — no alignment faults",
        "AC7_v3d_initialized":    "AC7  Mesa V3D 7.1 driver initialized",
        "AC8_pisp_be_v4l2":       "AC8  pisp_be V4L2 device probed",
    }

    for key, meta in RESULTS["acceptance_criteria"].items():
        status = "[✓] PASS" if meta["pass"] else "[✗] FAIL"
        print(f"   {status}   {AC_LABELS.get(key, key)}")

    print("─" * 65)
    print(f"   Result : {RESULTS['verdict']}  ({passed}/{len(RESULTS['acceptance_criteria'])} criteria passed)")
    print("=================================================================")

    # Write master result JSON
    result_path = LOG_DIR / f"boss_pi5_acceptance_{TIMESTAMP}.json"
    with open(result_path, "w") as f:
        json.dump(RESULTS, f, indent=2)
    print(f"   Full report: {result_path}")

    return RESULTS["exit_code"]

if __name__ == "__main__":
    sys.exit(main())
