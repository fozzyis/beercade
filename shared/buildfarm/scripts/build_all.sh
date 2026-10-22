#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"
DIST_DIR="${ROOT_DIR}/dist"

mkdir -p "${DIST_DIR}"

echo "======================================================================"
echo "   Starting B.E.E.R.C.A.D.E. A.S.T.R.O. Full Appliance Build Pipeline   "
echo "======================================================================"

# Step 1: Kernel & DTS
"${SCRIPT_DIR}/build_kernel.sh"

# Step 2: Mesa Panfrost / PanVK
"${SCRIPT_DIR}/build_mesa.sh"

# Step 3: RetroArch KMSDRM
"${SCRIPT_DIR}/build_retroarch.sh"

# Step 4: Emulation Cores
"${SCRIPT_DIR}/build_cores.sh"

# Step 5: Frontend (ES-DE)
"${SCRIPT_DIR}/build_frontend.sh"

# Step 6: Assemble & Package Read-Only SquashFS Appliance
echo "[+] Assembling full appliance rootfs & packaging LZ4 SquashFS..."
"${SCRIPT_DIR}/assemble_and_flash_rootfs.sh"

echo "======================================================================"
echo "   [✓] B.E.E.R.C.A.D.E. A.S.T.R.O. Full Build Pipeline Complete!     "
echo "======================================================================"
