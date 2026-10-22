#!/bin/busybox sh

# Ultra-fast minimal init to bypass systemd overhead on boot
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev

# Initialize the overlayfs environment (simulated via mkinitcpio run_hook usually, but this acts as fallback/standalone)
modprobe overlay
mkdir -p /lower /overlay /overlay/upper /overlay/work /new_root

mount -o ro /dev/mmcblk0p2 /lower
mount -t tmpfs tmpfs /overlay
mkdir -p /overlay/upper /overlay/work

mount -t overlay overlay -o lowerdir=/lower,upperdir=/overlay/upper,workdir=/overlay/work /new_root

# Switch root and immediately launch the KMS frontend bypassing normal init
exec switch_root /new_root /sbin/astro_launcher
