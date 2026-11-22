#!/usr/bin/env python3
"""Static & structural tests for x86_64 payload firmware coverage and Hyper-V VHDX verifiers."""

from __future__ import annotations

import re
import shutil
import subprocess
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
PAYLOAD_DIR = REPO_ROOT / "shared" / "buildfarm" / "x86_64" / "payloads"
MKINITCPIO_CONF = REPO_ROOT / "astro" / "platform" / "x86_64" / "mkinitcpio.conf"
VERIFY_QEMU = REPO_ROOT / "shared" / "tests" / "verify_qemu_x86_64.sh"
VERIFY_HYPERV = REPO_ROOT / "shared" / "tests" / "verify_hyperv_x86_64.ps1"

REQUIRED_FIRMWARE_PACKAGES = {
    "linux-firmware-amdgpu",
    "linux-firmware-intel",
    "linux-firmware-radeon",
    "linux-firmware-realtek",
    "linux-firmware-mediatek",
    "linux-firmware-atheros",
    "linux-firmware-broadcom",
    "linux-firmware-marvell",
    "linux-firmware-other",
}

REQUIRED_GPU_AND_HYPERV_PACKAGES = {
    "mesa",
    "vulkan-radeon",
    "vulkan-intel",
    "vulkan-icd-loader",
    "curl",
    "hyperv",
}

REQUIRED_COMMON_UNITS = {
    "beercade-grow-data",
    "beercade-boot-check",
    "hv_kvp_daemon",
}


def parse_bash_array(text: str, var_name: str) -> list[str]:
    lines = text.splitlines()
    start_re = re.compile(rf"^{re.escape(var_name)}=\((.*)$")
    collecting = False
    tokens: list[str] = []
    for raw in lines:
        if not collecting:
            m = start_re.match(raw)
            if not m:
                continue
            collecting = True
            body = m.group(1)
        else:
            body = raw
        code = body.split("#", 1)[0]
        if ")" in code:
            before_close = code.split(")", 1)[0].strip()
            if before_close:
                tokens.extend(before_close.split())
            return tokens
        stripped = code.strip()
        if stripped:
            tokens.extend(stripped.split())
    raise AssertionError(f"Could not find closing ')' for bash array {var_name}=(...)")


def parse_bash_scalar(text: str, var_name: str) -> str:
    match = re.search(rf"^{re.escape(var_name)}=([^\s#]+)", text, re.MULTILINE)
    if not match:
        raise AssertionError(f"Could not find scalar {var_name}=...")
    return match.group(1)


class TestX86PayloadAndVhdx(unittest.TestCase):
    def test_astro_payload_firmware_gpu_and_hyperv(self) -> None:
        text = (PAYLOAD_DIR / "astro.conf").read_text(encoding="utf-8")
        pkgs = set(parse_bash_array(text, "PACMAN_PACKAGES"))
        units = set(parse_bash_array(text, "ENABLE_UNITS"))
        root_mib = int(parse_bash_scalar(text, "ROOT_MIB"))

        self.assertGreaterEqual(root_mib, 4096)
        self.assertEqual(set(), REQUIRED_FIRMWARE_PACKAGES - pkgs)
        self.assertEqual(set(), REQUIRED_GPU_AND_HYPERV_PACKAGES - pkgs)
        self.assertIn("nvidia-open", pkgs)
        self.assertIn("nvidia-utils", pkgs)
        self.assertEqual(set(), REQUIRED_COMMON_UNITS - units)

    def test_boss_payload_firmware_gpu_and_hyperv(self) -> None:
        text = (PAYLOAD_DIR / "boss.conf").read_text(encoding="utf-8")
        pkgs = set(parse_bash_array(text, "PACMAN_PACKAGES"))
        units = set(parse_bash_array(text, "ENABLE_UNITS"))
        root_mib = int(parse_bash_scalar(text, "ROOT_MIB"))

        self.assertGreaterEqual(root_mib, 3072)
        self.assertEqual(set(), REQUIRED_FIRMWARE_PACKAGES - pkgs)
        self.assertEqual(set(), REQUIRED_GPU_AND_HYPERV_PACKAGES - pkgs)
        self.assertEqual(set(), REQUIRED_COMMON_UNITS - units)

    def test_x86_mkinitcpio_has_hyperv_modules(self) -> None:
        text = MKINITCPIO_CONF.read_text(encoding="utf-8")
        modules = set(parse_bash_array(text, "MODULES"))
        for mod in ("hv_vmbus", "hv_storvsc", "hv_netvsc", "hyperv_keyboard"):
            self.assertIn(mod, modules)

    def test_verify_qemu_supports_vhdx_and_astro_shell(self) -> None:
        text = VERIFY_QEMU.read_text(encoding="utf-8")
        self.assertIn("*.vhdx", text)
        self.assertIn("qemu-img check -f vhdx", text)
        self.assertIn("astro-shell", text)
        self.assertIn("hv_vmbus", text)
        self.assertNotIn("es-de", text)

    def test_verify_hyperv_script_structure_and_syntax(self) -> None:
        text = VERIFY_HYPERV.read_text(encoding="utf-8")
        for token in (
            "-Generation 2",
            "-EnableSecureBoot Off",
            "Resize-VHD",
            "vhdxfile",
            "-DryRun",
            "Heartbeat",
        ):
            self.assertIn(token, text)

        ps_exe = shutil.which("pwsh") or shutil.which("powershell")
        if ps_exe:
            ps_script = (
                "$errs=$null;"
                "[void][System.Management.Automation.Language.Parser]::ParseFile("
                f"'{str(VERIFY_HYPERV)}',[ref]$null,[ref]$errs);"
                "if($errs.Count -gt 0){$errs | ForEach-Object{Write-Error $_}; exit 1}"
            )
            proc = subprocess.run(
                [ps_exe, "-NoProfile", "-Command", ps_script],
                capture_output=True,
                text=True,
            )
            self.assertEqual(
                proc.returncode,
                0,
                f"PowerShell parser failed: {proc.stderr or proc.stdout}",
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)
