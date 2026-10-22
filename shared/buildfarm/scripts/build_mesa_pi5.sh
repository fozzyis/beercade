#!/usr/bin/env bash
# ==============================================================================
# B.O.S.S. Mesa Build Pipeline for Raspberry Pi 5 (VideoCore VII — V3D 7.1)
# Builds Mesa Gallium v3d + vc4 and Vulkan broadcom drivers for BCM2712.
#
# Milestone 4 — VideoCore VII & PiSP Pipeline
#
# Drivers produced:
#   - libGL / libEGL via Gallium v3d (OpenGL ES 3.1+, OpenGL 3.3)
#   - libGL / libEGL via Gallium vc4 (display KMS/scanout pipeline)
#   - libvulkan_broadcom.so (Vulkan 1.2 via broadcom driver)
#   - DRI/GBM backend for Wayland/KMS display composition
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"
CROSS_FILE="${ROOT_DIR}/buildfarm/toolchain/aarch64-linux-gnu.txt"

mkdir -p "${BUILD_DIR}" "${ROOTFS_DIR}"

echo "================================================================="
echo "   B.O.S.S. Mesa / VideoCore VII Build Pipeline (Pi 5 BCM2712)  "
echo "================================================================="

# ── 1. libdrm (dependency for Mesa DRM backend) ──────────────────────
DRM_SRC="${BUILD_DIR}/drm"
if [ ! -d "${DRM_SRC}" ]; then
    echo "[+] Cloning libdrm..."
    git clone --depth 1 https://gitlab.freedesktop.org/mesa/drm.git "${DRM_SRC}"
fi

cd "${DRM_SRC}"
rm -rf build_cross_pi5
meson setup build_cross_pi5 \
  --cross-file "${CROSS_FILE}" \
  --prefix=/usr \
  -Dbuildtype=release \
  -Dvc4=enabled \
  -Damdgpu=disabled \
  -Dradeon=disabled \
  -Dintel=disabled \
  -Dnouveau=disabled \
  -Dfreedreno=disabled

ninja -C build_cross_pi5 -j"$(nproc)"
DESTDIR="${ROOTFS_DIR}" ninja -C build_cross_pi5 install
echo "[✓] libdrm (vc4 backend) installed."

# ── 2. Mesa (V3D + VC4 Gallium + Broadcom Vulkan) ────────────────────
MESA_SRC="${BUILD_DIR}/mesa"
if [ ! -d "${MESA_SRC}" ]; then
    echo "[+] Cloning Mesa..."
    git clone --depth 1 -b main https://gitlab.freedesktop.org/mesa/mesa.git "${MESA_SRC}"
fi

cd "${MESA_SRC}"

# Apply upstream nir_algebraic sparse table fix (safe no-op if already patched)
if grep -q 'automaton\.table\[op\]\[indices\]' "${MESA_SRC}/src/compiler/nir/nir_algebraic.py" 2>/dev/null; then
    sed -i 's/automaton\.table\[op\]\[indices\]/automaton.table[op].get(indices, 0)/g' \
        "${MESA_SRC}/src/compiler/nir/nir_algebraic.py"
    echo "[+] Applied nir_algebraic sparse table fix."
fi

# Build native host tools (mesa_clc / vtn_bindgen2 needed for cross-compile)
if [ ! -f "build_native_pi5/src/broadcom/clif/clif_dump" ] && \
   [ ! -f "build_native_pi5/build.ninja" ]; then
    echo "[+] Configuring Mesa native build (host tools for cross-compilation)..."
    rm -rf build_native_pi5
    meson setup build_native_pi5 \
      -Dgallium-drivers=v3d,vc4 \
      -Dvulkan-drivers=broadcom \
      -Dglx=disabled \
      -Dopengl=false \
      -Dllvm=disabled \
      -Dshared-glapi=disabled \
      -Dplatforms= \
      -Dbuildtype=release
    ninja -C build_native_pi5 src/broadcom/clif/clif_dump 2>/dev/null || true
fi

# Cross-compile Mesa for AArch64 target (BCM2712 Pi 5)
echo "[+] Configuring Mesa cross-build for BCM2712 (v3d + vc4 + broadcom Vulkan)..."
rm -rf build_cross_pi5
meson setup build_cross_pi5 \
  --cross-file "${CROSS_FILE}" \
  --prefix=/usr \
  -Dbuildtype=release \
  -Dgallium-drivers=v3d,vc4,virgl \
  -Dvulkan-drivers=broadcom \
  -Dplatforms=wayland,drm \
  -Degl=enabled \
  -Dgles1=enabled \
  -Dgles2=enabled \
  -Dopengl=true \
  -Dglx=disabled \
  -Dllvm=disabled \
  -Dshared-glapi=enabled \
  -Dgbm=enabled \
  -Ddri3=enabled \
  -Dvc4-simulator=false

echo "[+] Building Mesa (VideoCore VII V3D + VC4 + Broadcom Vulkan)..."
ninja -C build_cross_pi5 -j"$(nproc)"

echo "[+] Installing Mesa to ASTRO rootfs..."
DESTDIR="${ROOTFS_DIR}" ninja -C build_cross_pi5 install

echo "================================================================="
echo "   [✓] Mesa VideoCore VII Build Complete!                        "
echo "   ● Gallium v3d  : OpenGL ES 3.1 / OpenGL 3.3 (V3D 7.1)       "
echo "   ● Gallium vc4  : KMS display pipeline (micro-HDMI)           "
echo "   ● Vulkan       : libvulkan_broadcom.so                        "
echo "================================================================="

# ── 3. Verify Mesa installation integrity ────────────────────────────
echo "[*] Verifying Mesa installation..."
V3D_DRI="${ROOTFS_DIR}/usr/lib/aarch64-linux-gnu/dri/v3d_dri.so"
VC4_DRI="${ROOTFS_DIR}/usr/lib/aarch64-linux-gnu/dri/vc4_dri.so"
BROADCOM_VK="${ROOTFS_DIR}/usr/share/vulkan/icd.d/broadcom_icd.aarch64.json"

for f in "${V3D_DRI}" "${VC4_DRI}" "${BROADCOM_VK}"; do
    if [ -f "${f}" ]; then
        echo "   [✓] Found: ${f}"
    else
        echo "   [!] Missing: ${f} (check Mesa build logs)"
    fi
done
