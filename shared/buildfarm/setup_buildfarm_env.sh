#!/usr/bin/env bash
set -e

echo "=========================================================="
echo "   Setting up B.E.E.R.C.A.D.E. Build Farm Environment     "
echo "=========================================================="

echo "[*] Installing Host Build Tools & Cross-Compilers for CachyOS/Arch Linux..."

sudo pacman -S --needed --noconfirm \
    base-devel \
    aarch64-linux-gnu-gcc \
    aarch64-linux-gnu-binutils \
    aarch64-linux-gnu-glibc \
    aarch64-linux-gnu-linux-api-headers \
    meson \
    ninja \
    pkgconf \
    python-mako \
    dtc \
    bc \
    bison \
    flex \
    libarchive \
    wget \
    curl \
    git \
    cmake \
    glslang \
    vulkan-headers

echo "[*] Setting up Rust Cross-Compilation Toolchain..."
if ! command -v rustup &> /dev/null; then
    echo "[-] rustup not found. Please install rustup (sudo pacman -S rustup) first."
else
    rustup target add aarch64-unknown-linux-gnu
fi

echo "[*] Creating aarch64-linux-gnu-pkg-config wrapper if missing..."
if ! command -v aarch64-linux-gnu-pkg-config &> /dev/null; then
    sudo bash -c 'cat > /usr/local/bin/aarch64-linux-gnu-pkg-config << "EOF"
#!/bin/sh
export PKG_CONFIG_DIR=
export PKG_CONFIG_LIBDIR=/usr/aarch64-linux-gnu/lib/pkgconfig:/usr/aarch64-linux-gnu/share/pkgconfig
export PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu
exec pkg-config "$@"
EOF'
    sudo chmod +x /usr/local/bin/aarch64-linux-gnu-pkg-config
fi

echo "[+] =========================================================="
echo "[+] Environment Setup Complete! You are ready to build."
echo "[+] Run: ./buildfarm/scripts/build_all.sh"
echo "[+] =========================================================="
