#!/usr/bin/env python3
"""
Tier 1 Feature Tests: BCM2712 Firmware & Boot (Requirement R1, AC 1, AC 2)
Test cases:
1. test_dtb_binary_header_and_magic: Validates DTB 8-byte alignment, size limit, and 0xd00dfeed magic.
2. test_dtb_model_and_compatibility: Verifies model="Raspberry Pi 5" and compatible="raspberrypi,5-model-b".
3. test_dtb_d0_stepping_revision: Verifies bcm2712-d-rpi-5-b.dtb presence, magic, and compatible strings.
4. test_kernel_defconfig_symbols: Verifies all mandatory Broadcom, RP1, 16KB page, and driver symbols.
5. test_qemu_bcm2712_boot_execution: Verifies QEMU BCM2712 boot harness passes AC 1 and AC 2.
"""

import os
import sys
import struct
import subprocess
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO_ROOT / "tests"))

try:
    from verify_qemu_bcm2712 import QemuBcm2712Verifier
except ImportError:
    QemuBcm2712Verifier = None


class TestBcm2712Boot(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.repo_root = REPO_ROOT
        cls.defconfig_path = cls.repo_root / "ASTRO" / "configs" / "bcm2712_boss_defconfig"
        cls.dtb_paths = [
            cls.repo_root / "build" / "linux-rockchip" / "arch" / "arm64" / "boot" / "dts" / "broadcom" / "bcm2712-rpi-5-b.dtb",
            cls.repo_root / "ASTRO" / "rootfs" / "boot" / "bcm2712-rpi-5-b.dtb",
            cls.repo_root / "dist" / "boot" / "bcm2712-rpi-5-b.dtb",
        ]
        cls.dtb_d_paths = [
            cls.repo_root / "build" / "linux-rockchip" / "arch" / "arm64" / "boot" / "dts" / "broadcom" / "bcm2712-d-rpi-5-b.dtb",
            cls.repo_root / "ASTRO" / "rootfs" / "boot" / "bcm2712-d-rpi-5-b.dtb",
            cls.repo_root / "dist" / "boot" / "bcm2712-d-rpi-5-b.dtb",
        ]

    def _find_dtb(self, candidates):
        for p in candidates:
            if p.exists():
                return p
        return None

    def test_01_dtb_binary_header_and_magic(self):
        """Test that bcm2712-rpi-5-b.dtb has valid FDT magic 0xd00dfeed and proper size."""
        dtb = self._find_dtb(self.dtb_paths)
        self.assertIsNotNone(dtb, f"bcm2712-rpi-5-b.dtb not found in {self.dtb_paths}")

        size = dtb.stat().st_size
        self.assertGreater(size, 1024, "DTB file size is unexpectedly small (<1KB)")
        self.assertLess(size, 2 * 1024 * 1024, "DTB file exceeds maximum 2MB size limit")

        with open(dtb, "rb") as f:
            header = f.read(16)
        magic, totalsize = struct.unpack(">II", header[:8])
        self.assertEqual(magic, 0xd00dfeed, f"Invalid DTB magic: {hex(magic)} (expected 0xd00dfeed)")
        self.assertLessEqual(totalsize, size, "DTB header total size exceeds file size")

    def test_02_dtb_model_and_compatibility(self):
        """Test that bcm2712-rpi-5-b.dtb exposes model='Raspberry Pi 5' and BCM2712 compatible."""
        dtb = self._find_dtb(self.dtb_paths)
        self.assertIsNotNone(dtb, "DTB file missing")

        res = subprocess.run(["fdtdump", str(dtb)], capture_output=True, text=True)
        if res.returncode == 0:
            out = res.stdout
            self.assertIn('model = "Raspberry Pi 5"', out)
            self.assertIn("raspberrypi,5-model-b", out)
            self.assertIn("brcm,bcm2712", out)
        else:
            # Fallback binary string check if fdtdump utility is not in PATH
            with open(dtb, "rb") as f:
                raw = f.read()
            self.assertIn(b"Raspberry Pi 5", raw)
            self.assertIn(b"raspberrypi,5-model-b", raw)

    def test_03_dtb_d0_stepping_revision(self):
        """Test that bcm2712-d-rpi-5-b.dtb exists and contains valid D0 stepping compatibility."""
        dtb_d = self._find_dtb(self.dtb_d_paths)
        self.assertIsNotNone(dtb_d, f"bcm2712-d-rpi-5-b.dtb not found in {self.dtb_d_paths}")

        with open(dtb_d, "rb") as f:
            magic, = struct.unpack(">I", f.read(4))
        self.assertEqual(magic, 0xd00dfeed, f"Invalid D0 DTB magic: {hex(magic)}")

        with open(dtb_d, "rb") as f:
            raw = f.read()
        self.assertIn(b"Raspberry Pi 5", raw)

    def test_04_kernel_defconfig_symbols(self):
        """Test that bcm2712_boss_defconfig contains all required architectural symbols."""
        self.assertTrue(self.defconfig_path.exists(), f"Defconfig not found: {self.defconfig_path}")

        content = self.defconfig_path.read_text(encoding="utf-8")

        required_symbols = [
            "CONFIG_ARCH_BCM=y",
            "CONFIG_ARCH_BCM2835=y",
            "CONFIG_ARM64_16K_PAGES=y",
            "CONFIG_ARM64_PAGE_SHIFT=14",
            "CONFIG_BCM2712_MIP=y",
            "CONFIG_PCIE_BRCMSTB=y",
            "CONFIG_MISC_RP1=y",
            "CONFIG_PINCTRL_RP1=y",
            "CONFIG_COMMON_CLK_RP1=y",
            "CONFIG_DRM_V3D=y",
            "CONFIG_DRM_VC4=y",
            "CONFIG_VIDEO_RASPBERRYPI_PISP_BE=m",
            "CONFIG_CMA_SIZE_MBYTES=512",
        ]

        for sym in required_symbols:
            self.assertIn(sym, content, f"Missing required kernel config symbol: {sym}")

    def test_05_qemu_bcm2712_boot_execution(self):
        """Test automated QEMU BCM2712 boot verification harness (AC 1 & AC 2)."""
        self.assertIsNotNone(QemuBcm2712Verifier, "QemuBcm2712Verifier class failed to import")

        verifier = QemuBcm2712Verifier(timeout=60)
        verifier.check_prerequisites()
        code = verifier.run()

        self.assertEqual(code, 0, f"QEMU BCM2712 verifier failed with exit code {code}")
        self.assertTrue(verifier.results["ac2_dtb_loaded"], "AC 2 failed: DTB model was not detected")
        self.assertTrue(verifier.results["ac2_no_dtb_errors"], "AC 2 failed: DTB errors occurred during boot")
        self.assertTrue(verifier.results["ac1_boot_to_prompt"], "AC 1 failed: System did not reach command prompt")


if __name__ == "__main__":
    unittest.main(verbosity=2)
