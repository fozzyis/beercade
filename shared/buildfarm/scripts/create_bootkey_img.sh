#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="/mnt/d/data/dev/beercade"
DIST_DIR="${ROOT_DIR}/dist"
BUILD_DIR="${ROOT_DIR}/build"
IMG_TMP="/tmp/beercade-bootkey-rock5c.img"
IMG_FINAL="${DIST_DIR}/beercade-bootkey-rock5c.img"
MNT_DIR="/tmp/bootkey_mnt_build"

UBOOT_BIN="${BUILD_DIR}/u-boot/u-boot-rockchip.bin"
KERNEL_IMAGE="${BUILD_DIR}/linux-rockchip/arch/arm64/boot/Image"
DTB_FILE="${BUILD_DIR}/linux-rockchip/arch/arm64/boot/dts/rockchip/rk3588s-rock-5c.dtb"
EXTLINUX_CONF="${BUILD_DIR}/extlinux.conf"
OVERLAY_FILE="${BUILD_DIR}/rock5c-hdmi-audio.dtbo"

echo "[1/6] Creating 256MB raw disk image in /tmp..."
rm -f "${IMG_TMP}"
truncate -s 256M "${IMG_TMP}"

echo "[2/6] Writing partition table..."
parted -s "${IMG_TMP}" mklabel gpt
parted -s "${IMG_TMP}" mkpart primary fat32 16MB 100%
parted -s "${IMG_TMP}" set 1 boot on

echo "[3/6] Flashing U-Boot to Sector 64 (32KB offset)..."
dd if="${UBOOT_BIN}" of="${IMG_TMP}" seek=64 conv=notrunc status=none

echo "[4/6] Setting up loop device and formatting FAT32 filesystem..."
LOOP_DEV=$(losetup -Pf --show "${IMG_TMP}")

mkfs.vfat -F 32 -n "BOOTKEY" "${LOOP_DEV}p1"

echo "[5/6] Populating bootkey filesystem..."
mkdir -p "${MNT_DIR}"
mount "${LOOP_DEV}p1" "${MNT_DIR}"

mkdir -p "${MNT_DIR}/extlinux" "${MNT_DIR}/overlays"
cp "${KERNEL_IMAGE}" "${MNT_DIR}/Image"
cp "${DTB_FILE}" "${MNT_DIR}/rk3588s-rock-5c.dtb"
cp "${EXTLINUX_CONF}" "${MNT_DIR}/extlinux/extlinux.conf"
if [ -f "${OVERLAY_FILE}" ]; then
    cp "${OVERLAY_FILE}" "${MNT_DIR}/overlays/rock5c-hdmi-audio.dtbo"
fi

sync
umount "${MNT_DIR}"
losetup -d "${LOOP_DEV}"

echo "[6/6] Copying final image to ${IMG_FINAL}..."
mkdir -p "${DIST_DIR}"
cp -f "${IMG_TMP}" "${IMG_FINAL}"
rm -f "${IMG_TMP}"

echo "[✓] Boot key image successfully created at: ${IMG_FINAL}"
ls -lh "${IMG_FINAL}"
