#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"

mkdir -p "${BUILD_DIR}" "${ROOTFS_DIR}"

echo "[+] Building ES-DE Frontend (EmulationStation Desktop Edition) for KMSDRM / ALSA..."

"${SCRIPT_DIR}/build_esde_deps.sh"

export PKG_CONFIG_PATH=""
export PKG_CONFIG_LIBDIR="${ROOTFS_DIR}/usr/lib/pkgconfig:${ROOTFS_DIR}/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="${ROOTFS_DIR}"

ESDE_DIR="${BUILD_DIR}/es-de"
if [ ! -d "${ESDE_DIR}" ]; then
    git clone --depth 1 https://gitlab.com/es-de/emulationstation-de.git "${ESDE_DIR}"
fi

# Fix FindPoppler.cmake GET_PREREQUISITES host ldd invocation during cross-compilation
if grep -q "GET_PREREQUISITES" "${ESDE_DIR}/CMake/Packages/FindPoppler.cmake" 2>/dev/null; then
    sed -i '/include(GetPrerequisites)/,/endif()/c\  set(POPPLER_NEEDS_FONTCONFIG FALSE)' "${ESDE_DIR}/CMake/Packages/FindPoppler.cmake"
fi

# Ensure CURL_LIBRARIES is populated when find_package(CURL) finds CURLConfig.cmake
if ! grep -q "set(CURL_LIBRARIES CURL::libcurl)" "${ESDE_DIR}/CMakeLists.txt" 2>/dev/null; then
    sed -i '/find_package(CURL REQUIRED)/a\    if(NOT CURL_LIBRARIES)\n        set(CURL_LIBRARIES CURL::libcurl)\n    endif()' "${ESDE_DIR}/CMakeLists.txt"
fi

cd "${ESDE_DIR}"
rm -rf build_cross
cmake -B build_cross \
  -DCMAKE_TOOLCHAIN_FILE="${ROOT_DIR}/buildfarm/toolchain/aarch64-toolchain.cmake" \
  -DCMAKE_BUILD_TYPE=Release \
  -DGL=OFF \
  -DGLES=ON \
  -DENABLE_VIDEO=ON \
  -DENABLE_PULSE=OFF \
  -DENABLE_ALSA=ON \
  -DCMAKE_INSTALL_PREFIX=/usr

make -C build_cross -j"$(nproc)"
make -C build_cross install DESTDIR="${ROOTFS_DIR}"

# Install Beercade Neon theme and systems configuration
mkdir -p "${ROOTFS_DIR}/etc/emulationstation"
cp -r "${ROOT_DIR}/emu_fe/es_systems.cfg" "${ROOTFS_DIR}/etc/emulationstation/" 2>/dev/null || true

echo "[✓] ES-DE build complete."
