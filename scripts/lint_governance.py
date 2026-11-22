#!/usr/bin/env python3
"""M4 Governance, Path-Ownership, and Crate-Boundary Linter for B.E.E.R.C.A.D.E.

Checks:
1. Root CODEOWNERS syntax, default wildcard rule, literal path existence, and
   coverage of every top-level tracked directory.
2. Cargo workspace crate membership and strict internal/external dependency
   boundaries via `cargo metadata --no-deps --format-version 1`.
3. Platform-specific boot/arch files stay confined to `*/platform/<arch>/`,
   `shared/buildfarm/<arch>/`, or `shared/tests/`, and `beercade-shared` stays
   free of platform-specific I/O and target_arch probes.

Exit codes:
  0 - All checks passed
  1 - One or more governance/boundary violations detected
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

EXPECTED_WORKSPACE_CRATES = {
    "beercade-shared",
    "beercade-hal",
    "astro-daemon",
    "astro-shell",
    "astro-tune",
    "boss-server",
}

ALLOWED_INTERNAL_DEPS = {
    "beercade-shared": set(),
    "beercade-hal": set(),
    "astro-daemon": {"beercade-shared", "beercade-hal"},
    "astro-shell": {"beercade-shared"},
    "astro-tune": {"beercade-hal"},
    "boss-server": {"beercade-shared", "beercade-hal"},
}

FORBIDDEN_EXTERNAL_DEPS = {
    "beercade-shared": {"libc", "tokio", "nix", "sdl2", "axum"},
}

PLATFORM_BASENAME_RE = re.compile(
    r"^(config\.txt|boot\.cmd|mkinitcpio\.conf|loader\.conf|beercade-slot-.*\.conf)$|\.dtb$",
    re.IGNORECASE,
)

ALLOWED_PLATFORM_PREFIXES = (
    "astro/platform/x86_64/",
    "astro/platform/aarch64/",
    "boss/platform/x86_64/",
    "boss/platform/aarch64/",
    "shared/platform/x86_64/",
    "shared/platform/aarch64/",
    "shared/platform/universal-aarch64/",
    "shared/buildfarm/x86_64/",
    "shared/buildfarm/aarch64/",
    "shared/tests/",
)

SHARED_FORBIDDEN_TOKENS = (
    "target_arch",
    "/sys/class/",
    "/proc/device-tree",
    "efivars",
)


def run_cmd(args: list[str]) -> str:
    proc = subprocess.run(
        args,
        cwd=str(REPO_ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8-sig",
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"command {' '.join(args)} failed ({proc.returncode}): {proc.stderr.strip()}"
        )
    return proc.stdout


def check_codeowners(failures: list[str]) -> list[str]:
    codeowners_path = REPO_ROOT / "CODEOWNERS"
    if not codeowners_path.is_file():
        failures.append("CODEOWNERS file is missing at repository root")
        return []

    lines = codeowners_path.read_text(encoding="utf-8").splitlines()
    rules: list[tuple[str, list[str], int]] = []
    has_wildcard = False

    for idx, raw in enumerate(lines, start=1):
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        parts = stripped.split()
        if len(parts) < 2:
            failures.append(f"CODEOWNERS:{idx}: missing owner(s) for pattern '{stripped}'")
            continue
        pattern, owners = parts[0], parts[1:]
        for owner in owners:
            if not (owner.startswith("@") or "@" in owner):
                failures.append(
                    f"CODEOWNERS:{idx}: invalid owner '{owner}' (expected @handle or email)"
                )
        if pattern == "*":
            has_wildcard = True
        if not any(ch in pattern for ch in "*?["):
            rel = pattern.lstrip("/").rstrip("/")
            if rel and not (REPO_ROOT / rel).exists():
                failures.append(
                    f"CODEOWNERS:{idx}: literal path '{pattern}' does not exist in repo"
                )
        rules.append((pattern, owners, idx))

    if not has_wildcard:
        failures.append("CODEOWNERS: missing default wildcard '*' ownership rule")

    # Verify every top-level non-dot tracked directory is matched by CODEOWNERS
    tracked_top = run_cmd(["git", "ls-tree", "--name-only", "HEAD"]).splitlines()
    top_dirs = sorted(
        name for name in tracked_top if not name.startswith(".") and (REPO_ROOT / name).is_dir()
    )
    # Also cover working-tree non-dot directories that contain tracked files
    for d in top_dirs:
        matched = has_wildcard or any(
            pat.lstrip("/").rstrip("/") == d or pat.lstrip("/").startswith(f"{d}/")
            for pat, _, _ in rules
        )
        if not matched:
            failures.append(f"CODEOWNERS: top-level directory '/{d}/' is not covered")

    return [
        f"CODEOWNERS validated ({len(rules)} rules, {len(top_dirs)} top-level dirs covered)"
    ]


def check_cargo_boundaries(failures: list[str]) -> list[str]:
    raw = run_cmd(["cargo", "metadata", "--no-deps", "--format-version", "1"])
    meta = json.loads(raw)
    ws_ids = set(meta.get("workspace_members", []))
    ws_packages = [pkg for pkg in meta.get("packages", []) if pkg.get("id") in ws_ids]
    found_names = {pkg["name"] for pkg in ws_packages}

    missing = sorted(EXPECTED_WORKSPACE_CRATES - found_names)
    unexpected = sorted(found_names - EXPECTED_WORKSPACE_CRATES)
    if missing:
        failures.append(f"Cargo workspace missing expected crates: {', '.join(missing)}")
    if unexpected:
        failures.append(f"Cargo workspace has unexpected crates: {', '.join(unexpected)}")

    for pkg in ws_packages:
        name = pkg["name"]
        allowed_internal = ALLOWED_INTERNAL_DEPS.get(name)
        forbidden_ext = FORBIDDEN_EXTERNAL_DEPS.get(name, set())
        for dep in pkg.get("dependencies", []):
            dep_name = dep["name"]
            kind = dep.get("kind")  # None = normal, "build", "dev"
            if dep_name in EXPECTED_WORKSPACE_CRATES:
                if allowed_internal is not None and dep_name not in allowed_internal:
                    failures.append(
                        f"Crate boundary violation: '{name}' depends on '{dep_name}' (kind={kind or 'normal'})"
                    )
            if kind in (None, "build") and dep_name in forbidden_ext:
                failures.append(
                    f"Forbidden dependency: '{name}' must not depend on '{dep_name}'"
                )

    return [
        f"Cargo crate boundaries validated across {len(ws_packages)} workspace crates"
    ]


def check_platform_boundaries(failures: list[str]) -> list[str]:
    tracked_files = run_cmd(["git", "ls-files"]).splitlines()
    checked_platform = 0
    for rel in tracked_files:
        norm = rel.replace("\\", "/")
        base = norm.rsplit("/", 1)[-1]
        if PLATFORM_BASENAME_RE.search(base):
            checked_platform += 1
            if not norm.startswith(ALLOWED_PLATFORM_PREFIXES):
                failures.append(
                    f"Platform file boundary violation: '{norm}' is outside allowed platform/buildfarm dirs"
                )

    shared_src = REPO_ROOT / "shared" / "crates" / "beercade-shared" / "src"
    checked_shared = 0
    if not shared_src.is_dir():
        failures.append(f"Missing directory: {shared_src}")
    else:
        for rs_file in sorted(shared_src.rglob("*.rs")):
            checked_shared += 1
            text = rs_file.read_text(encoding="utf-8", errors="replace")
            rel_rs = rs_file.relative_to(REPO_ROOT).as_posix()
            for token in SHARED_FORBIDDEN_TOKENS:
                if token in text:
                    failures.append(
                        f"beercade-shared purity violation: '{rel_rs}' contains forbidden token '{token}'"
                    )

    return [
        f"Platform file & beercade-shared purity validated ({checked_platform} platform configs, {checked_shared} shared Rust files)"
    ]


def main() -> int:
    failures: list[str] = []
    passes: list[str] = []

    for check in (check_codeowners, check_cargo_boundaries, check_platform_boundaries):
        try:
            passes.extend(check(failures))
        except Exception as exc:  # pragma: no cover - fatal tooling error
            failures.append(f"{check.__name__} error: {exc}")

    for msg in passes:
        if not failures:
            print(f"[PASS] {msg}")
        else:
            print(f"[INFO] {msg}")

    if failures:
        for err in failures:
            print(f"[FAIL] {err}", file=sys.stderr)
        return 1

    print("[PASS] M4 governance & crate-boundary lint passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
