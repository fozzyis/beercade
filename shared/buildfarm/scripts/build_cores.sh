#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
CORES_INSTALL_DIR="${ROOT_DIR}/ASTRO/rootfs/usr/lib/libretro"

mkdir -p "${BUILD_DIR}" "${CORES_INSTALL_DIR}"

export ARCH="arm64"
export CC="aarch64-linux-gnu-gcc"
export CXX="aarch64-linux-gnu-g++"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"
export CFLAGS="-O3 -march=armv8.2-a+crypto+fp16+dotprod -mtune=cortex-a76 -ffunction-sections -fdata-sections -fomit-frame-pointer -I${ROOTFS_DIR}/usr/include"
export CXXFLAGS="$CFLAGS"
export LDFLAGS="-Wl,--gc-sections -Wl,-O1 -Wl,--as-needed -L${ROOTFS_DIR}/usr/lib -Wl,-rpath-link=${ROOTFS_DIR}/usr/lib"

echo "=========================================================="
echo "   Compiling RK3588S Optimized Emulation Core Suite       "
echo "=========================================================="

# 1. FBNeo (Arcade / NeoGeo)
echo "[+] Building FBNeo..."
FBNEO_DIR="${BUILD_DIR}/fbneo"
if [ ! -d "${FBNEO_DIR}" ]; then
    git clone --depth 1 https://github.com/libretro/FBNeo.git "${FBNEO_DIR}"
fi
cd "${FBNEO_DIR}/src/burner/libretro"
make -j"$(nproc)" platform=unix ARCH=arm64 HAVE_GLES=1
cp fbneo_libretro.so "${CORES_INSTALL_DIR}/"

# 2. Snes9x
echo "[+] Building Snes9x..."
SNES_DIR="${BUILD_DIR}/snes9x"
if [ ! -d "${SNES_DIR}" ]; then
    git clone --depth 1 https://github.com/libretro/snes9x.git "${SNES_DIR}"
fi
cd "${SNES_DIR}/libretro"
make -j"$(nproc)" platform=unix ARCH=arm64
cp snes9x_libretro.so "${CORES_INSTALL_DIR}/"

# 3. Genesis Plus GX
echo "[+] Building Genesis Plus GX..."
GEN_DIR="${BUILD_DIR}/Genesis-Plus-GX"
if [ ! -d "${GEN_DIR}" ]; then
    git clone --depth 1 https://github.com/libretro/Genesis-Plus-GX.git "${GEN_DIR}"
fi
cd "${GEN_DIR}"
make -f Makefile.libretro -j"$(nproc)" platform=unix ARCH=arm64
cp genesis_plus_gx_libretro.so "${CORES_INSTALL_DIR}/"

# 4. Mupen64Plus-Next (N64)
#    NOTE: LTO must be disabled here. -flto=auto causes the linker to strip
#    New Dynarec assembly symbols (dynarec_jump_to, dyna_jump) from
#    linkage_arm64.S because raw .S objects are opaque to GCC's GIMPLE IR.
#    Wrapped in a subshell so LTO is preserved for subsequent cores.
echo "[+] Building Mupen64Plus-Next (LTO disabled for dynarec asm)..."
MUPEN_DIR="${BUILD_DIR}/mupen64plus-libretro-nx"
if [ ! -d "${MUPEN_DIR}" ]; then
    git clone --depth 1 https://github.com/libretro/mupen64plus-libretro-nx.git "${MUPEN_DIR}"
fi
(
    export CFLAGS="${CFLAGS//-flto=auto/}"
    export CXXFLAGS="${CXXFLAGS//-flto=auto/}"
    export LDFLAGS="${LDFLAGS} -fno-lto"
    cd "${MUPEN_DIR}"
    make -j"$(nproc)" platform=unix ARCH=aarch64 HAVE_PARALLEL=1 HAVE_PARALLEL_RSP=1 FORCE_GLES3=1 WITH_DYNAREC=aarch64
    cp mupen64plus_next_libretro.so "${CORES_INSTALL_DIR}/"
)

# 5. Flycast (Dreamcast)
echo "[+] Building Flycast (Vulkan PanVK / GLES)..."
FLYCAST_DIR="${BUILD_DIR}/flycast"
if [ ! -d "${FLYCAST_DIR}" ]; then
    git clone --depth 1 --recurse-submodules --shallow-submodules \
        https://github.com/flyinghead/flycast.git "${FLYCAST_DIR}"
else
    # Ensure submodules are populated (fixes prior --depth 1 clones without --recurse-submodules)
    git -C "${FLYCAST_DIR}" submodule update --init --recursive --depth 1
fi
cd "${FLYCAST_DIR}"
rm -rf build_cross
cmake -B build_cross \
  -DCMAKE_TOOLCHAIN_FILE="${ROOT_DIR}/buildfarm/toolchain/aarch64-toolchain.cmake" \
  -DCMAKE_BUILD_TYPE=Release \
  -DUSE_GLES=ON \
  -DUSE_VULKAN=ON \
  -DLIBRETRO=ON
make -C build_cross -j"$(nproc)"
cp build_cross/flycast_libretro.so "${CORES_INSTALL_DIR}/"

echo "[✓] Emulation cores compiled successfully."
