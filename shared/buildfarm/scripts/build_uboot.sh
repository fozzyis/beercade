#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
DIST_DIR="${ROOT_DIR}/dist"

mkdir -p "${BUILD_DIR}" "${DIST_DIR}"

echo "================================================================="
echo "   B.E.E.R.C.A.D.E. A.S.T.R.O. Rock 5C U-Boot Build Pipeline   "
echo "================================================================="

RKBIN_DIR="${BUILD_DIR}/rkbin"
if [ ! -d "${RKBIN_DIR}" ]; then
    echo "[+] Cloning Rockchip firmware binary blobs (rkbin)..."
    git clone --depth 1 https://github.com/rockchip-linux/rkbin.git "${RKBIN_DIR}"
fi

UBOOT_DIR="${BUILD_DIR}/u-boot"
if [ ! -d "${UBOOT_DIR}" ]; then
    echo "[+] Cloning mainline U-Boot..."
    git clone --depth 1 https://github.com/u-boot/u-boot.git "${UBOOT_DIR}"
fi

BL31_BIN="$(ls -1 ${RKBIN_DIR}/bin/rk35/rk3588_bl31_v*.elf | sort -V | tail -n 1)"
TPL_BIN="$(ls -1 ${RKBIN_DIR}/bin/rk35/rk3588_ddr_lp4_2112MHz_lp5_2400MHz_v*.bin | sort -V | tail -n 1)"

echo "[+] Using BL31: ${BL31_BIN}"
echo "[+] Using TPL:  ${TPL_BIN}"

cd "${UBOOT_DIR}"
export CROSS_COMPILE="aarch64-linux-gnu-"
export BL31="${BL31_BIN}"
export ROCKCHIP_TPL="${TPL_BIN}"

echo "[+] Configuring U-Boot for Radxa Rock 5C (rock-5c-rk3588s_defconfig)..."
make rock-5c-rk3588s_defconfig

echo "[+] Compiling U-Boot..."
make -j"$(nproc)"

echo "[+] Staging bootloader binaries..."
cp "${UBOOT_DIR}/u-boot-rockchip.bin" "${DIST_DIR}/"
cp "${UBOOT_DIR}/idbloader.img" "${DIST_DIR}/"
cp "${UBOOT_DIR}/u-boot.itb" "${DIST_DIR}/"

echo "================================================================="
echo "   [✓] U-Boot Build Complete: ${DIST_DIR}/u-boot-rockchip.bin   "
echo "================================================================="
