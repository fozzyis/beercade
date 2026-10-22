#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"

export ARCH="arm64"
export CROSS_COMPILE="aarch64-linux-gnu-"
export CFLAGS="-O3 -march=armv8.2-a+crypto+fp16+dotprod -mtune=cortex-a76 -flto=auto -ffunction-sections -fdata-sections -fomit-frame-pointer -funroll-loops"
export CXXFLAGS="$CFLAGS"
export LDFLAGS="-Wl,--gc-sections -Wl,-O1 -Wl,--as-needed"

mkdir -p "${BUILD_DIR}" "${ROOTFS_DIR}"

echo "================================================================="
echo "   B.E.E.R.C.A.D.E. A.S.T.R.O. RK3588S Master Build Pipeline   "
echo "================================================================="

KERNEL_SRC="${BUILD_DIR}/linux-rockchip"
if [ ! -d "${KERNEL_SRC}" ]; then
    echo "[+] Cloning Collabora rockchip-devel kernel tree..."
    git clone --depth 1 -b rockchip-devel https://gitlab.collabora.com/hardware-enablement/rockchip-3588/linux.git "${KERNEL_SRC}" || \
    git clone --depth 1 https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git "${KERNEL_SRC}"
fi

echo "[+] Applying A.S.T.R.O. defconfig..."
cp "${ROOT_DIR}/ASTRO/configs/rk3588_astro_defconfig" "${KERNEL_SRC}/arch/arm64/configs/rk3588_astro_defconfig"

cd "${KERNEL_SRC}"
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- rk3588_astro_defconfig
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j"$(nproc)" Image dtbs modules

echo "[+] Compiling HDMI Audio Device Tree Overlay..."
dtc -@ -I dts -O dtb -o "${BUILD_DIR}/rock5c-hdmi-audio.dtbo" "${ROOT_DIR}/ASTRO/patches/0001-rock5c-hdmi-audio.dts"

echo "[+] Installing kernel & modules to rootfs staging..."
mkdir -p "${ROOTFS_DIR}/boot/overlays"
cp "${KERNEL_SRC}/arch/arm64/boot/Image" "${ROOTFS_DIR}/boot/Image"
cp "${KERNEL_SRC}/arch/arm64/boot/dts/rockchip/rk3588s-rock-5c.dtb" "${ROOTFS_DIR}/boot/"
cp "${BUILD_DIR}/rock5c-hdmi-audio.dtbo" "${ROOTFS_DIR}/boot/overlays/"
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- modules_install INSTALL_MOD_PATH="${ROOTFS_DIR}"

echo "[✓] Kernel & DTS build complete."
