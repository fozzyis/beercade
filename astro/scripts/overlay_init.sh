#!/bin/busybox sh

# A.S.T.R.O. Early Boot OverlayFS Script
# This replaces the standard init to mount the root as read-only, 
# and overlay it with a tmpfs or the writable /data partition.

# Mount essential pseudo-filesystems
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev

# Mount the real Root A/B (passed in via root= on cmdline) as read-only
REAL_ROOT=$(cat /proc/cmdline | grep -o 'root=[^ ]*' | cut -d= -f2)
if [ -z "$REAL_ROOT" ]; then
    REAL_ROOT="/dev/mmcblk0p2" # Fallback
fi

mkdir -p /ro_root
mount -t ext4 -o ro $REAL_ROOT /ro_root

# Mount the persistent data partition (Partition 4)
mkdir -p /rw_data
# We assume mmcblk0p4 is the data partition. In reality, we'd use UUID/PARTLABEL.
mount -t ext4 /dev/mmcblk0p4 /rw_data

# Set up the OverlayFS directories
mkdir -p /rw_data/upper
mkdir -p /rw_data/work

# Mount the OverlayFS as the new root
mount -t overlay overlay -o lowerdir=/ro_root,upperdir=/rw_data/upper,workdir=/rw_data/work /

# Move the pseudo-filesystems to the new root
mount --move /proc /proc
mount --move /sys /sys
mount --move /dev /dev

# Execute the real init (systemd)
exec switch_root / /sbin/init
