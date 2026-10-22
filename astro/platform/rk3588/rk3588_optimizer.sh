#!/bin/bash
# ==============================================================================
# RK3588S / Radxa Rock 5C Low-Latency Arcade Emulation Runtime Optimizer
# ==============================================================================

set -e

echo "[+] Applying RK3588S hardware latency optimizations..."

# 1. Enforce performance governor on all 8 cores (4x A55 + 4x A76)
for gov in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
    if [ -f "$gov" ]; then
        echo "performance" > "$gov"
    fi
done

# 2. Lock GPU frequency to maximum (Mali-G610 MP4 @ 1.0 GHz)
if [ -d /sys/class/devfreq/fb000000.gpu ]; then
    echo "performance" > /sys/class/devfreq/fb000000.gpu/governor 2>/dev/null || true
fi

# 3. Kernel memory tuning for zero frame jitter
sysctl -w vm.swappiness=0 >/dev/null
sysctl -w vm.dirty_ratio=10 >/dev/null
sysctl -w vm.dirty_background_ratio=5 >/dev/null

# 4. CPU Core Shielding:
# Create cgroup slice for background daemons on Cortex-A55 (Cores 0-3)
if [ -d /sys/fs/cgroup ]; then
    mkdir -p /sys/fs/cgroup/system_daemons 2>/dev/null || true
    if [ -f /sys/fs/cgroup/system_daemons/cpuset.cpus ]; then
        echo "0-3" > /sys/fs/cgroup/system_daemons/cpuset.cpus 2>/dev/null || true
    fi
fi

# 5. Launch Target Command pinned strictly to Cortex-A76 performance cluster (Cores 4-7)
if [ $# -gt 0 ]; then
    echo "[+] Launching $1 on Cortex-A76 performance cluster (Cores 4-7)..."
    exec taskset -c 4-7 "$@"
fi
