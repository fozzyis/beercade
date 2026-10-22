#!/usr/bin/env bash
# ==============================================================================
# B.E.E.R.C.A.D.E. Unified Arch Linux ARM Bootstrapper
# Stages rootfs, configures users, networking, and systemd units for SBC targets
# ==============================================================================
set -euo pipefail

TARGET_DIR="${1:-/tmp/arch_bootstrap}"
PROFILE="${2:-rock5c}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARCH_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=== Bootstrapping Arch Linux ARM for profile: ${PROFILE} at ${TARGET_DIR} ==="

mkdir -p "${TARGET_DIR}/etc/systemd/network"
mkdir -p "${TARGET_DIR}/etc/systemd/resolved.conf.d"
mkdir -p "${TARGET_DIR}/etc/sysctl.d"
mkdir -p "${TARGET_DIR}/etc/security/limits.d"

# 1. Install unified networking
cp "${ARCH_DIR}/systemd/10-wired.network" "${TARGET_DIR}/etc/systemd/network/"
cp "${ARCH_DIR}/systemd/mdns.conf" "${TARGET_DIR}/etc/systemd/resolved.conf.d/"

# 2. Install unified sysctl
cp "${ARCH_DIR}/sysctl/99-beercade-latency.conf" "${TARGET_DIR}/etc/sysctl.d/"

# 3. Enable core services via systemd wants symlinks
WANTS_DIR="${TARGET_DIR}/etc/systemd/system/multi-user.target.wants"
mkdir -p "${WANTS_DIR}"
ln -sf /usr/lib/systemd/system/systemd-networkd.service "${WANTS_DIR}/systemd-networkd.service"
ln -sf /usr/lib/systemd/system/systemd-resolved.service "${WANTS_DIR}/systemd-resolved.service"
ln -sf /usr/lib/systemd/system/sshd.service "${WANTS_DIR}/sshd.service"
ln -sf /usr/lib/systemd/system/avahi-daemon.service "${WANTS_DIR}/avahi-daemon.service"

echo "=== Arch Linux ARM bootstrap complete for ${PROFILE} ==="
