#!/usr/bin/env bash
# ==============================================================================
# B.O.S.S. Kernel & DTB Build Pipeline for Raspberry Pi 5 (Broadcom BCM2712)
# Compiles 64-bit kernel_2712.img, bcm2712-rpi-5-b.dtb, bcm2712-d-rpi-5-b.dtb,
# and stages all boot assets to ASTRO/rootfs/boot and dist/boot.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"
DIST_BOOT_DIR="${ROOT_DIR}/dist/boot"
KERNEL_SRC="${BUILD_DIR}/linux-rockchip"

export ARCH="arm64"
export CROSS_COMPILE="aarch64-linux-gnu-"
export CFLAGS="-O3 -march=armv8.2-a+crypto+fp16+dotprod -mtune=cortex-a76 -flto=auto -ffunction-sections -fdata-sections -fomit-frame-pointer -funroll-loops"
export CXXFLAGS="$CFLAGS"
export LDFLAGS="-Wl,--gc-sections -Wl,-O1 -Wl,--as-needed"

mkdir -p "${BUILD_DIR}" "${ROOTFS_DIR}/boot/overlays" "${DIST_BOOT_DIR}/overlays"

echo "================================================================="
echo "   🚀 B.E.E.R.C.A.D.E. B.O.S.S. Raspberry Pi 5 Kernel Pipeline  "
echo "================================================================="

if [ ! -d "${KERNEL_SRC}" ]; then
    echo "[-] Error: Kernel source directory ${KERNEL_SRC} does not exist!"
    exit 1
fi

echo "[+] Staging bcm2712_boss_defconfig..."
cp -f "${ROOT_DIR}/ASTRO/configs/bcm2712_boss_defconfig" "${KERNEL_SRC}/arch/arm64/configs/bcm2712_boss_defconfig"

cd "${KERNEL_SRC}"

echo "[+] Generating kernel configuration from bcm2712_boss_defconfig..."
rm -f .config
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- bcm2712_boss_defconfig

echo "[+] Compiling BCM2712 Device Tree Blobs..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j"$(nproc)" broadcom/bcm2712-rpi-5-b.dtb broadcom/bcm2712-d-rpi-5-b.dtb

echo "[+] Compiling 64-bit kernel image (Image) and modules..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j"$(nproc)" Image modules

echo "[+] Staging kernel, modules, and DTBs to rootfs and dist/boot..."
# Install modules directly into the rootfs staging area
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- INSTALL_MOD_PATH="${ROOTFS_DIR}" modules_install
# Raspberry Pi 5 SPI EEPROM bootloader expects kernel_2712.img
cp -f "${KERNEL_SRC}/arch/arm64/boot/Image" "${ROOTFS_DIR}/boot/kernel_2712.img"
cp -f "${KERNEL_SRC}/arch/arm64/boot/Image" "${DIST_BOOT_DIR}/kernel_2712.img"

# Stage C0 and D0 DTB blobs
for dtb in bcm2712-rpi-5-b.dtb bcm2712-d-rpi-5-b.dtb; do
    if [ -f "${KERNEL_SRC}/arch/arm64/boot/dts/broadcom/${dtb}" ]; then
        cp -f "${KERNEL_SRC}/arch/arm64/boot/dts/broadcom/${dtb}" "${ROOTFS_DIR}/boot/${dtb}"
        cp -f "${KERNEL_SRC}/arch/arm64/boot/dts/broadcom/${dtb}" "${DIST_BOOT_DIR}/${dtb}"
        echo "    [✓] Staged ${dtb}"
    fi
done

# Check if vc4-kms-v3d-pi5.dtbo exists in repo or firmware cache, and stage it
if [ -f "${ROOT_DIR}/build/overlays/vc4-kms-v3d-pi5.dtbo" ]; then
    cp -f "${ROOT_DIR}/build/overlays/vc4-kms-v3d-pi5.dtbo" "${ROOTFS_DIR}/boot/overlays/vc4-kms-v3d-pi5.dtbo"
    cp -f "${ROOT_DIR}/build/overlays/vc4-kms-v3d-pi5.dtbo" "${DIST_BOOT_DIR}/overlays/vc4-kms-v3d-pi5.dtbo"
    echo "    [✓] Staged vc4-kms-v3d-pi5.dtbo from build/overlays"
elif [ -f "${ROOTFS_DIR}/boot/overlays/vc4-kms-v3d-pi5.dtbo" ]; then
    cp -f "${ROOTFS_DIR}/boot/overlays/vc4-kms-v3d-pi5.dtbo" "${DIST_BOOT_DIR}/overlays/vc4-kms-v3d-pi5.dtbo"
    echo "    [✓] Staged existing vc4-kms-v3d-pi5.dtbo to dist/boot"
fi

echo "================================================================="
echo "   [✓] BCM2712 Kernel & Device Trees Built and Staged!           "
echo "   • Kernel : ${ROOTFS_DIR}/boot/kernel_2712.img                 "
echo "   • DTB C0 : ${ROOTFS_DIR}/boot/bcm2712-rpi-5-b.dtb             "
echo "   • DTB D0 : ${ROOTFS_DIR}/boot/bcm2712-d-rpi-5-b.dtb           "
echo "================================================================="
