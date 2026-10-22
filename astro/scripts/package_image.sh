#!/bin/bash
set -e

# Package script for Beercade OS Image for Rock 5C (RK3588)
# Embeds the bootloader and packages the rootfs into a bootable .img for SD/eMMC

OUTPUT_IMG="beercade-redfern-v1.img"
ROOTFS_DIR="/tmp/beercade_rootfs"
UBOOT_IDBLOADER="idbloader.img"
UBOOT_ITB="u-boot.itb"

# We create a 6.5GB sparse image (256MB boot + 3GB Root A + 3GB Root B + ~250MB Data)
# The Data partition will be expanded on first boot to fill the rest of the disk.
echo "[*] Validating prerequisites..."
if [ ! -f "$UBOOT_IDBLOADER" ] || [ ! -f "$UBOOT_ITB" ]; then
    echo "[-] FATAL ERROR: Bootloader files ($UBOOT_IDBLOADER, $UBOOT_ITB) not found."
    echo "[-] You MUST provide the compiled U-Boot binaries to create a bootable image."
    exit 1
fi

if [ ! -d "$ROOTFS_DIR" ] || [ -z "$(ls -A $ROOTFS_DIR)" ]; then
    echo "[-] FATAL ERROR: Root filesystem directory ($ROOTFS_DIR) is missing or empty."
    echo "[-] The rootfs must be built and staged before packaging."
    exit 1
fi

echo "[*] Creating raw image file: $OUTPUT_IMG"
dd if=/dev/zero of=$OUTPUT_IMG bs=1M count=0 seek=6656

echo "[*] Partitioning the image..."
parted -s $OUTPUT_IMG mklabel gpt
# Partition 1: Boot (ext4) - 256MB
parted -s $OUTPUT_IMG mkpart primary ext4 16MB 256MB
# Partition 2: Root A (ext4/squashfs) - 3GB
parted -s $OUTPUT_IMG mkpart primary ext4 256MB 3328MB
# Partition 3: Root B (ext4/squashfs) - 3GB
parted -s $OUTPUT_IMG mkpart primary ext4 3328MB 6400MB
# Partition 4: Data (ext4 rw) - remaining space
parted -s $OUTPUT_IMG mkpart primary ext4 6400MB 100%

# Set legacy boot flag on boot partition
parted -s $OUTPUT_IMG set 1 legacy_boot on

echo "[*] Flashing Bootloader to Sector 64 (32KB offset) and Sector 16384 (8MB offset)..."
dd if=$UBOOT_IDBLOADER of=$OUTPUT_IMG seek=64 conv=notrunc
dd if=$UBOOT_ITB of=$OUTPUT_IMG seek=16384 conv=notrunc
echo "[+] Bootloader embedded successfully."

echo "[*] Formatting partitions..."
# Use losetup to map the partitions
LOOP_DEV=$(sudo losetup -fP --show $OUTPUT_IMG)
echo "Mapped image to $LOOP_DEV"

sudo mkfs.ext4 ${LOOP_DEV}p1 -L BOOT
sudo mkfs.ext4 ${LOOP_DEV}p2 -L ROOTA
sudo mkfs.ext4 ${LOOP_DEV}p3 -L ROOTB
sudo mkfs.ext4 ${LOOP_DEV}p4 -L DATA

echo "[*] Mounting partitions and syncing rootfs..."
MNT_DIR="/tmp/beercade_mnt"
sudo mkdir -p $MNT_DIR
sudo mount ${LOOP_DEV}p2 $MNT_DIR

echo "[*] Rsyncing base system to Root A..."
sudo rsync -aHAX --info=progress2 $ROOTFS_DIR/ $MNT_DIR/

echo "[*] Syncing filesystem buffers..."
sync
sudo umount $MNT_DIR
sudo rm -rf $MNT_DIR

sudo losetup -d $LOOP_DEV

echo "[*] Image structure complete: $OUTPUT_IMG"
