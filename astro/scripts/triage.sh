#!/bin/bash
# A.S.T.R.O. DRM/KMS Triage Script — Rock 5C (RK3588S / Panthor)
# Usage: scp triage.sh root@<ORION_IP>:/tmp/ && ssh root@<ORION_IP> bash /tmp/triage.sh
set -euo pipefail

OUT="/tmp/astro_triage_$(date +%Y%m%d_%H%M%S).log"
exec > >(tee "$OUT") 2>&1

echo "╔══════════════════════════════════════════════════════╗"
echo "║  A.S.T.R.O. DRM/KMS TRIAGE — $(date -Iseconds)     ║"
echo "╚══════════════════════════════════════════════════════╝"

# ── Section 1: Kernel & Boot ────────────────────────────────
echo -e "\n═══ [1] KERNEL RELEASE & CMDLINE ═══"
uname -a
cat /proc/cmdline

# ── Section 2: Panthor / Mali / DRM Driver Probe ────────────
echo -e "\n═══ [2] DRM / PANTHOR / MALI DRIVER STATUS ═══"
dmesg | grep -Ei "panthor|mali|drm|rockchip-drm|vop2|hdmi|dp|cma|gpu|firmware" | tail -80

echo -e "\n--- Panthor module loaded? ---"
lsmod | grep -i panthor || echo "[!] panthor module NOT loaded"

echo -e "\n--- GPU firmware files ---"
ls -la /lib/firmware/arm/mali/arch10.8/ 2>/dev/null || \
  echo "[!] Panthor firmware directory missing! Expected: /lib/firmware/arm/mali/arch10.8/"
ls -la /lib/firmware/panthor/ 2>/dev/null || true

# ── Section 3: DRM Device Nodes & Permissions ───────────────
echo -e "\n═══ [3] DRM DEVICE NODES ═══"
ls -la /dev/dri/ 2>/dev/null || echo "[!] /dev/dri/ does not exist"
stat /dev/dri/card0 2>/dev/null || echo "[!] /dev/dri/card0 missing"
stat /dev/dri/renderD128 2>/dev/null || echo "[!] /dev/dri/renderD128 missing"

echo -e "\n--- DRM master holder (fuser) ---"
fuser -v /dev/dri/card0 2>&1 || echo "[i] No process holds DRM master"

# ── Section 4: Connector / CRTC / Encoder Enumeration ──────
echo -e "\n═══ [4] DRM CONNECTOR / CRTC ENUMERATION ═══"
if command -v modetest &>/dev/null; then
    modetest -M rockchip -c 2>&1 | head -60
    echo -e "\n--- Encoders ---"
    modetest -M rockchip -e 2>&1 | head -30
    echo -e "\n--- CRTCs ---"
    modetest -M rockchip -p 2>&1 | head -30
else
    echo "[!] modetest not installed. Install: pacman -S libdrm (or apt install libdrm-tests)"
    for conn in /sys/class/drm/card*-*; do
        echo "$(basename $conn): status=$(cat $conn/status 2>/dev/null) enabled=$(cat $conn/enabled 2>/dev/null) modes=$(head -1 $conn/modes 2>/dev/null)"
    done
fi

# ── Section 5: EDID Readback ───────────────────────────────
echo -e "\n═══ [5] EDID STATUS ═══"
for edid in /sys/class/drm/card*-*/edid; do
    conn="$(dirname "$edid")"
    sz=$(stat -c%s "$edid" 2>/dev/null || echo 0)
    echo "$(basename $conn): edid_bytes=$sz"
    if [ "$sz" -gt 0 ] && command -v edid-decode &>/dev/null; then
        edid-decode "$edid" 2>&1 | head -20
    fi
done

# ── Section 6: CMA Memory Pool ─────────────────────────────
echo -e "\n═══ [6] CMA MEMORY POOL ═══"
grep -i cma /proc/meminfo 2>/dev/null || echo "[!] No CMA info in meminfo"
cat /proc/buddyinfo 2>/dev/null | head -5
cat /sys/kernel/debug/dma_buf/bufinfo 2>/dev/null | head -20 || true

# ── Section 7: Systemd Crash Analysis ──────────────────────
echo -e "\n═══ [7] SYSTEMD FAILED UNITS & CRASH JOURNAL ═══"
systemctl --failed --no-pager 2>/dev/null || true

echo -e "\n--- Priority 0-3 (emerg/alert/crit/err) journal ---"
journalctl -b 0 -p 3 --no-pager -n 50 2>/dev/null || \
  journalctl -p 3 --no-pager -n 50 2>/dev/null || \
  echo "[!] journalctl unavailable"

echo -e "\n--- Coredumps ---"
coredumpctl list --no-pager 2>/dev/null | tail -10 || echo "[i] No coredumps found"

# ── Section 8: VT & Framebuffer State ──────────────────────
echo -e "\n═══ [8] VT / FRAMEBUFFER STATE ═══"
cat /sys/class/vtconsole/*/name 2>/dev/null || true
cat /sys/class/graphics/fb0/name 2>/dev/null || echo "[!] fb0 missing"
cat /sys/class/graphics/fb0/virtual_size 2>/dev/null || true

echo -e "\n--- Active VT ---"
fgconsole 2>/dev/null || cat /sys/class/tty/tty0/active 2>/dev/null || echo "[i] Cannot determine active VT"

# ── Section 9: Mesa Driver Identity ────────────────────────
echo -e "\n═══ [9] MESA DRIVER IDENTITY ═══"
if command -v glxinfo &>/dev/null; then
    DISPLAY= WAYLAND_DISPLAY= MESA_LOADER_DRIVER_OVERRIDE=panthor glxinfo -B 2>&1 | head -10 || true
fi
find /usr/lib -name 'libvulkan_*.so' -o -name '*_dri.so' 2>/dev/null | sort || true

# ── Section 10: GPU Thermal ─────────────────────────────────
echo -e "\n═══ [10] THERMAL ZONES ═══"
for tz in /sys/class/thermal/thermal_zone*/; do
    type=$(cat ${tz}type 2>/dev/null)
    temp=$(cat ${tz}temp 2>/dev/null)
    echo "$(basename $tz): type=$type temp=${temp}m°C"
done

# ── Section 11: Process Audit ───────────────────────────────
echo -e "\n═══ [11] PROCESS AUDIT ═══"
ps aux | grep -Ei "retroarch|emulationstation|es-de|astro-daemon|weston|kmscon|plymouth|agetty" | grep -v grep || echo "[i] No display processes running"

echo -e "\n═══════════════════════════════════════════════════════"
echo "Triage complete. Output saved to: $OUT"
echo "═══════════════════════════════════════════════════════"
