#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"
CROSS_FILE="${ROOT_DIR}/buildfarm/toolchain/aarch64-linux-gnu.txt"

mkdir -p "${BUILD_DIR}" "${ROOTFS_DIR}"

echo "[+] Building Mesa (Panfrost / PanVK Vulkan 1.4 for RK3588S Panthor)..."

DRM_SRC="${BUILD_DIR}/drm"
if [ ! -d "${DRM_SRC}" ]; then
    git clone --depth 1 https://gitlab.freedesktop.org/mesa/drm.git "${DRM_SRC}"
fi

cd "${DRM_SRC}"
rm -rf build_cross
meson setup build_cross \
  --cross-file "${CROSS_FILE}" \
  --prefix=/usr \
  -Dbuildtype=release

ninja -C build_cross -j"$(nproc)"
DESTDIR="${ROOTFS_DIR}" ninja -C build_cross install

MESA_SRC="${BUILD_DIR}/mesa"
if [ ! -d "${MESA_SRC}" ]; then
    git clone --depth 1 -b main https://gitlab.freedesktop.org/mesa/mesa.git "${MESA_SRC}"
fi

cd "${MESA_SRC}"

# Patch nir_algebraic.py to prevent KeyError on sparse transition table lookup
if grep -q 'automaton\.table\[op\]\[indices\]' "${MESA_SRC}/src/compiler/nir/nir_algebraic.py" 2>/dev/null; then
    sed -i 's/automaton\.table\[op\]\[indices\]/automaton.table[op].get(indices, 0)/g' "${MESA_SRC}/src/compiler/nir/nir_algebraic.py"
fi

echo "[+] Building native mesa_clc and vtn_bindgen2..."
if [ ! -f "build_native/src/panfrost/clc/panfrost_compile" ]; then
    rm -rf build_native
    meson setup build_native \
      -Dgallium-drivers=panfrost \
      -Dvulkan-drivers=panfrost \
      -Dmesa-clc=enabled \
      -Dinstall-mesa-clc=true \
      -Dinstall-precomp-compiler=true \
      -Dglx=disabled \
      -Dopengl=false \
      -Dllvm=enabled \
      -Dshared-glapi=disabled \
      -Dplatforms= \
      -Dbuildtype=release
    
    ninja -C build_native
fi

export PATH="${MESA_SRC}/build_native/src/compiler/clc:${MESA_SRC}/build_native/src/compiler/spirv:${MESA_SRC}/build_native/src/panfrost/clc:${PATH}"

rm -rf build_cross
meson setup build_cross \
  --cross-file "${CROSS_FILE}" \
  --prefix=/usr \
  -Dbuildtype=release \
  -Doptimization=3 \
  -Dstrip=true \
  -Dgallium-drivers=panfrost \
  -Dvulkan-drivers=panfrost \
  -Dglx=disabled \
  -Dopengl=false \
  -Dgles1=disabled \
  -Dgles2=enabled \
  -Degl=enabled \
  -Dplatforms= \
  -Dshared-glapi=enabled \
  -Dllvm=disabled \
  -Dmesa-clc=system \
  -Dprecomp-compiler=system \
  -Dzstd=disabled \
  -Dxmlconfig=disabled

# Pre-generate nir_opt_algebraic.c serially to avoid Python multiprocessing / memory contention
mkdir -p "${MESA_SRC}/build_cross/src/compiler/nir"
if [ ! -f "${MESA_SRC}/build_cross/src/compiler/nir/nir_opt_algebraic.c" ]; then
    python3 "${MESA_SRC}/src/compiler/nir/nir_opt_algebraic.py" --out "${MESA_SRC}/build_cross/src/compiler/nir/nir_opt_algebraic.c"
fi

ninja -C build_cross -j"$(nproc)"
DESTDIR="${ROOTFS_DIR}" ninja -C build_cross install

echo "[✓] Mesa Panfrost / PanVK build complete."
