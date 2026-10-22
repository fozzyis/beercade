#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="/data/dev/beercade"
UBOOT_BIN="${ROOT_DIR}/dist/u-boot-rockchip.bin"
if [ ! -f "${UBOOT_BIN}" ]; then
    UBOOT_BIN="${ROOT_DIR}/build/u-boot/u-boot-rockchip.bin"
fi
KERNEL_IMAGE="${ROOT_DIR}/build/linux-rockchip/arch/arm64/boot/Image"
DTB_FILE="${ROOT_DIR}/build/linux-rockchip/arch/arm64/boot/dts/rockchip/rk3588s-rock-5c.dtb"
EXTLINUX_CONF="${ROOT_DIR}/build/extlinux.conf"
SQUASHFS_IMG="${ROOT_DIR}/dist/astro-rootfs-slotA.squashfs"

SD_DISK="/dev/sda"
BOOT_PART="/dev/sda1"
ROOT_PART="/dev/sda2"
MNT_DIR="/tmp/beercade_flash_boot"

echo "================================================================="
echo "   🚀 B.E.E.R.C.A.D.E. A.S.T.R.O. Complete SD Card Flasher      "
echo "================================================================="

echo "[1/4] Unmounting existing partitions..."
sudo umount ${SD_DISK}* 2>/dev/null || true

echo "[2/4] Flashing U-Boot to Sector 64 (32KB offset)..."
sudo dd if="${UBOOT_BIN}" of="${SD_DISK}" seek=64 conv=notrunc status=none
echo "[✓] Bootloader written to ${SD_DISK} (Sector 64)."

echo "[3/4] Updating Boot Partition (${BOOT_PART})..."
sudo mkdir -p "${MNT_DIR}"
sudo mount "${BOOT_PART}" "${MNT_DIR}"
sudo mkdir -p "${MNT_DIR}/extlinux" "${MNT_DIR}/overlays"
sudo cp "${KERNEL_IMAGE}" "${MNT_DIR}/Image"
sudo cp "${DTB_FILE}" "${MNT_DIR}/rk3588s-rock-5c.dtb"
sudo cp "${EXTLINUX_CONF}" "${MNT_DIR}/extlinux/extlinux.conf"
sudo umount "${MNT_DIR}"
echo "[✓] Kernel Image, DTB, and extlinux.conf written to ${BOOT_PART}."

echo "[4/4] Flashing Full Appliance RootFS SquashFS to ${ROOT_PART}..."
sudo dd if="${SQUASHFS_IMG}" of="${ROOT_PART}" bs=4M status=progress
echo "[✓] SquashFS RootFS written to ${ROOT_PART}."

echo "[*] Syncing hardware buffers..."
sudo sync

echo "================================================================="
echo "   [✓] B.E.E.R.C.A.D.E. A.S.T.R.O. Flashed Successfully!        "
echo "================================================================="
