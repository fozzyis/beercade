#!/bin/bash
# =====================================================================
# B.E.E.R.C.A.D.E. A.S.T.R.O. Direct KMSDRM Fast-Boot Loader for ES-DE
# Target: Sub-3.5s Cold Boot directly to Arcade UI without X11/Wayland
# =====================================================================

set -e

echo "[+] Initializing A.S.T.R.O. KMSDRM Display Pipeline..."

# 1. Ensure performance mode on RK3588S SoC
/data/dev/beercade/emu_opti/rk3588_optimizer.sh >/dev/null 2>&1 || true

# 2. Configure DRM/KMS environment variables
export SDL_VIDEODRIVER=kmsdrm
export SDL_AUDIODRIVER=alsa
export AUDIODEV=hw:0,0
export MESA_LOADER_DRIVER_OVERRIDE=panthor

# 3. Fast launch ES-DE pinned to Cortex-A76 cores
echo "[+] Starting EmulationStation-DE on KMSDRM Framebuffer..."
exec taskset -c 4-7 emulationstation \
    --home /beercade \
    --resolution 1920 1080 \
    --vsync 1
