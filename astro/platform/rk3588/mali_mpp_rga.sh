#!/usr/bin/env bash
# ==============================================================================
# O.R.I.O.N. RK3588 Mali G610, MPP & RGA Hardware Acceleration Bindings
# Configures permissions and device node access for low-latency graphics & decoding.
# ==============================================================================
set -euo pipefail

echo "[+] Initializing RK3588 hardware accelerator nodes (Mali G610, MPP, RGA)..."

# 1. DRM / Panthor Mali G610 MP4 permissions
for dev in /dev/dri/card* /dev/dri/renderD*; do
    if [ -e "$dev" ]; then
        chmod 0666 "$dev"
    fi
done

# 2. Rockchip 2D Raster Graphic Acceleration (RGA)
for dev in /dev/rga /dev/rga_service; do
    if [ -e "$dev" ]; then
        chmod 0666 "$dev"
    fi
done

# 3. Rockchip Media Process Platform (MPP / VPU)
for dev in /dev/mpp_service /dev/vpu_service /dev/hevc_service /dev/rkvdec; do
    if [ -e "$dev" ]; then
        chmod 0666 "$dev"
    fi
done

# 4. DMA-BUF heap permissions for zero-copy buffer sharing between MPP/RGA and DRM
for dev in /dev/dma_heap/*; do
    if [ -e "$dev" ]; then
        chmod 0666 "$dev"
    fi
done

echo "[✓] RK3588 hardware acceleration nodes initialized."
