#!/bin/bash
set -e

# A.S.T.R.O. Automated RootFS & Kernel Build Pipeline
# This script downloads U-Boot, stages the Arch Linux ARM rootfs, 
# cross-compiles the Rockchip kernel with our RT config, and injects our daemon.

ROOTFS_DIR="/tmp/beercade_rootfs"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &> /dev/null && pwd)"
ASTRO_ROOT="$(dirname "$SCRIPT_DIR")"
WORKSPACE_ROOT="$(dirname "$ASTRO_ROOT")"

echo "[*] ========================================="
echo "[*] A.S.T.R.O. RootFS Build Pipeline"
echo "[*] ========================================="

# 1. Check/Install Toolchains
echo "[*] Step 1: Checking cross-compilation toolchains..."
if ! command -v aarch64-linux-gnu-gcc &> /dev/null; then
    echo "[-] Missing aarch64-linux-gnu-gcc. Installing..."
    if command -v pacman &> /dev/null; then
        sudo pacman -S --needed --noconfirm aarch64-linux-gnu-gcc aarch64-linux-gnu-binutils aarch64-linux-gnu-glibc bc bison flex base-devel libarchive
    elif command -v apt-get &> /dev/null; then
        sudo apt-get update && sudo apt-get install -y gcc-aarch64-linux-gnu bc bison flex libssl-dev make libc6-dev-arm64-cross curl bsdtar
    else
        echo "[-] Unsupported package manager. Please manually install the AArch64 GCC cross-compiler."
        exit 1
    fi
fi

# 2. Fetch U-Boot for Rock 5C
echo "[*] Step 2: Fetching Radxa U-Boot binaries..."
cd "$SCRIPT_DIR"
if [ ! -f "idbloader.img" ] || [ ! -f "u-boot.itb" ]; then
    # Download Radxa's pre-compiled U-Boot for RK3588 (Rock 5B/5C)
    # Using generic placeholders here for the latest stable build
    wget -qO idbloader.img https://github.com/radxa-repo/bsp/releases/download/latest/idbloader.img || true
    wget -qO u-boot.itb https://github.com/radxa-repo/bsp/releases/download/latest/u-boot.itb || true
    
    # Check if they downloaded successfully
    if [ ! -f "idbloader.img" ]; then
        echo "[-] WARNING: Could not automatically download idbloader.img. Please place it manually."
    fi
else
    echo "[+] U-Boot binaries already present."
fi

# 3. Stage Arch Linux Rootfs
echo "[*] Step 3: Staging Arch Linux ARM rootfs..."
if [ ! -d "$ROOTFS_DIR" ]; then
    sudo mkdir -p "$ROOTFS_DIR"
fi

if [ -z "$(ls -A $ROOTFS_DIR 2>/dev/null)" ]; then
    cd /tmp
    if [ ! -f "ArchLinuxARM-aarch64-latest.tar.gz" ]; then
        echo "[*] Downloading Arch Linux ARM rootfs..."
        wget http://os.archlinuxarm.org/os/ArchLinuxARM-aarch64-latest.tar.gz
    fi
    echo "[*] Extracting rootfs (this will take a while)..."
    sudo bsdtar -xpf ArchLinuxARM-aarch64-latest.tar.gz -C "$ROOTFS_DIR"
    echo "[+] Rootfs extracted to $ROOTFS_DIR"
else
    echo "[+] Rootfs already staged at $ROOTFS_DIR"
fi

# 4. Cross-compile Custom Kernel
echo "[*] Step 4: Compiling custom Rockchip Kernel (linux-6.1-stan-rkr1)..."
KERNEL_DIR="/tmp/rockchip-linux"
if [ ! -d "$KERNEL_DIR" ]; then
    echo "[*] Cloning Rockchip kernel..."
    git clone --depth 1 -b linux-6.1-stan-rkr1 https://github.com/radxa/kernel.git "$KERNEL_DIR"
fi

cd "$KERNEL_DIR"
# Copy our custom defconfig
cp "$ASTRO_ROOT/configs/rk3588_astro_defconfig" arch/arm64/configs/
echo "[*] Configuring kernel..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- rk3588_astro_defconfig

echo "[*] Building kernel (Image and dtbs)..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j$(nproc) Image dtbs modules

echo "[*] Installing kernel modules to rootfs..."
sudo make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- INSTALL_MOD_PATH="$ROOTFS_DIR" modules_install

echo "[*] Copying kernel Image and DTB to rootfs /boot..."
sudo cp arch/arm64/boot/Image "$ROOTFS_DIR/boot/"
# Note: Specific DTB for Rock 5C might be rk3588s-rock-5c.dtb
sudo cp arch/arm64/boot/dts/rockchip/rk3588*.dtb "$ROOTFS_DIR/boot/"

# 5. Inject Custom Scripts and Daemon
echo "[*] Step 5: Injecting A.S.T.R.O. daemon and init scripts..."
DAEMON_DIR="$WORKSPACE_ROOT/astro-daemon"
cd "$DAEMON_DIR"
echo "[*] Cross-compiling astro-daemon (requires rustup target add aarch64-unknown-linux-gnu)..."
cargo build --target aarch64-unknown-linux-gnu --release
sudo cp target/aarch64-unknown-linux-gnu/release/astro-daemon "$ROOTFS_DIR/usr/local/bin/"

echo "[*] Injecting overlay_init.sh..."
sudo cp "$SCRIPT_DIR/overlay_init.sh" "$ROOTFS_DIR/usr/local/bin/overlay_init.sh"
sudo chmod +x "$ROOTFS_DIR/usr/local/bin/overlay_init.sh"

echo "[*] Injecting first_boot_resize.sh..."
sudo cp "$SCRIPT_DIR/first_boot_resize.sh" "$ROOTFS_DIR/usr/local/bin/first_boot_resize.sh"
sudo chmod +x "$ROOTFS_DIR/usr/local/bin/first_boot_resize.sh"

# 6. Enable WiFi Stack (wpa_supplicant for SSID autoconnect)
echo "[*] Step 6: Enabling wireless stack..."
sudo mkdir -p "$ROOTFS_DIR/etc/systemd/system/multi-user.target.wants"
sudo ln -sf /usr/lib/systemd/system/wpa_supplicant@.service \
    "$ROOTFS_DIR/etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service"
echo "[+] wpa_supplicant@wlan0.service enabled"

# 7. Configure systemd-resolved symlink
echo "[*] Step 7: Linking resolv.conf for systemd-resolved..."
sudo ln -sf /run/systemd/resolve/stub-resolv.conf "$ROOTFS_DIR/etc/resolv.conf"
echo "[+] resolv.conf symlinked to stub-resolv.conf"

# 8. Validate critical GPU libraries
echo "[*] Step 8: Validating GPU library chain..."
VALIDATION_FAILED=0
for lib in libdrm.so libEGL.so libGLESv2.so libgbm.so; do
    if [ ! -f "$ROOTFS_DIR/usr/lib/$lib" ]; then
        echo "[-] FATAL: Missing $lib in rootfs!"
        VALIDATION_FAILED=1
    else
        echo "[+] Found $lib"
    fi
done

# Check for Rockchip DRM userspace library
if [ ! -f "$ROOTFS_DIR/usr/lib/libdrm_rockchip.so" ]; then
    echo "[!] WARNING: libdrm_rockchip.so missing — Rockchip DRM/KMS may not initialise"
    echo "[!] Install rockchip-patched libdrm or copy from vendor SDK"
fi

# Check for Mesa DRI driver (Panthor or Panfrost)
if [ ! -f "$ROOTFS_DIR/usr/lib/dri/panthor_dri.so" ] && \
   [ ! -f "$ROOTFS_DIR/usr/lib/dri/panfrost_dri.so" ] && \
   [ ! -f "$ROOTFS_DIR/usr/lib/dri/rockchip_dri.so" ]; then
    echo "[!] WARNING: No Mesa DRI driver for Panthor/Panfrost GPU found in /usr/lib/dri/"
    echo "[!] GPU-accelerated rendering will not work without a DRI driver"
fi

if [ "$VALIDATION_FAILED" -eq 1 ]; then
    echo "[-] GPU library validation FAILED. Aborting."
    exit 1
fi

echo "[+] ========================================="
echo "[+] Rootfs staged successfully!"
echo "[+] You can now run package_image.sh"
echo "[+] ========================================="
