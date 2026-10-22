#!/bin/bash
set -e

# A.S.T.R.O. First Boot Resize Script
# Expands the 4th partition (/data) to fill the remainder of the SD card or eMMC.
# Intended to be run as a oneshot systemd service on first boot.

DATA_PART="/dev/mmcblk0p4"
DISK="/dev/mmcblk0"
PART_NUM=4

echo "[*] Expanding partition $PART_NUM on $DISK..."

# Use growpart to expand the partition table entry
# If growpart exits with 1, it means no space was available (which is fine if already expanded)
if growpart $DISK $PART_NUM; then
    echo "[+] Partition table expanded successfully."
    
    echo "[*] Resizing ext4 filesystem on $DATA_PART..."
    resize2fs $DATA_PART
    
    echo "[+] Filesystem resized successfully."
else
    echo "[!] growpart returned non-zero. The partition may already be fully expanded."
fi

# Disable the service so it doesn't run on subsequent boots
echo "[*] Disabling first_boot_resize service..."
systemctl disable astro-resize.service || true

echo "[+] Done."
