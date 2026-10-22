#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"

export PKG_CONF_PATH="pkg-config"
export PKG_CONFIG_LIBDIR="${ROOTFS_DIR}/usr/lib/pkgconfig:${ROOTFS_DIR}/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="${ROOTFS_DIR}"
export CC="aarch64-linux-gnu-gcc"
export CXX="aarch64-linux-gnu-g++"
export CFLAGS="-O3 -march=armv8.2-a+crypto+fp16+dotprod -mtune=cortex-a76 -ffunction-sections -fdata-sections -fomit-frame-pointer -funroll-loops -I${ROOTFS_DIR}/usr/include -I${ROOTFS_DIR}/usr/include/libdrm -D__ARM_NEON__=1"
export CXXFLAGS="$CFLAGS"
export LDFLAGS="-Wl,--gc-sections -Wl,-O1 -Wl,--as-needed -Wl,-rpath-link=${ROOTFS_DIR}/usr/lib -L${ROOTFS_DIR}/usr/lib"

mkdir -p "${BUILD_DIR}" "${ROOTFS_DIR}"

echo "[+] Building Minimal RetroArch (KMSDRM / ALSA direct)..."

ALSA_SRC="${BUILD_DIR}/alsa-lib"
if [ ! -d "${ALSA_SRC}" ]; then
    echo "[+] Building alsa-lib..."
    git clone --depth 1 -b v1.2.11 https://github.com/alsa-project/alsa-lib.git "${ALSA_SRC}"
    cd "${ALSA_SRC}"
    autoreconf -fi
    ./configure --host=aarch64-linux-gnu --prefix=/usr
    make -j"$(nproc)"
    make install DESTDIR="${ROOTFS_DIR}"
fi

# Vulkan Headers
VULKAN_HEADERS_SRC="${BUILD_DIR}/vulkan-headers"
if [ ! -d "${VULKAN_HEADERS_SRC}" ]; then
    echo "[+] Building vulkan-headers..."
    git clone --depth 1 -b v1.3.268 https://github.com/KhronosGroup/Vulkan-Headers.git "${VULKAN_HEADERS_SRC}"
    cd "${VULKAN_HEADERS_SRC}"
    cmake -B build -DCMAKE_INSTALL_PREFIX=/usr
    make -C build install DESTDIR="${ROOTFS_DIR}"
fi

# Vulkan Loader
VULKAN_LOADER_SRC="${BUILD_DIR}/vulkan-loader"
if [ ! -d "${VULKAN_LOADER_SRC}" ]; then
    echo "[+] Building vulkan-loader..."
    git clone --depth 1 -b v1.3.268 https://github.com/KhronosGroup/Vulkan-Loader.git "${VULKAN_LOADER_SRC}"
    cd "${VULKAN_LOADER_SRC}"
    cmake -B build -DCMAKE_SYSTEM_NAME=Linux \
      -DCMAKE_SYSTEM_PROCESSOR=aarch64 \
      -DCMAKE_C_COMPILER=aarch64-linux-gnu-gcc \
      -DCMAKE_CXX_COMPILER=aarch64-linux-gnu-g++ \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DBUILD_WSI_XCB_SUPPORT=OFF \
      -DBUILD_WSI_XLIB_SUPPORT=OFF \
      -DBUILD_WSI_WAYLAND_SUPPORT=OFF \
      -DBUILD_WSI_DIRECTFB_SUPPORT=OFF \
      -DCMAKE_C_FLAGS="-I${ROOTFS_DIR}/usr/include" \
      -DCMAKE_CXX_FLAGS="-I${ROOTFS_DIR}/usr/include" \
      -DCMAKE_EXE_LINKER_FLAGS="-L${ROOTFS_DIR}/usr/lib" \
      -DCMAKE_SHARED_LINKER_FLAGS="-L${ROOTFS_DIR}/usr/lib"
    make -C build -j"$(nproc)"
    make -C build install DESTDIR="${ROOTFS_DIR}"
fi

RA_SRC="${BUILD_DIR}/retroarch"
if [ ! -d "${RA_SRC}" ]; then
    git clone --depth 1 https://github.com/libretro/RetroArch.git "${RA_SRC}"
fi

cd "${RA_SRC}"
./configure \
  --prefix=/usr \
  --host=aarch64-linux-gnu \
  --disable-qt \
  --disable-discord \
  --disable-materialui \
  --disable-xmb \
  --disable-ozone \
  --disable-cheevos \
  --disable-networking \
  --disable-online_updater \
  --disable-ssl \
  --enable-kms \
  --enable-egl \
  --enable-vulkan \
  --enable-opengles \
  --enable-opengles3 \
  --enable-alsa \
  --disable-pulse \
  --disable-jack \
  --disable-wayland \
  --disable-x11

make -j"$(nproc)"
make install DESTDIR="${ROOTFS_DIR}"

echo "[✓] RetroArch KMSDRM build complete."
