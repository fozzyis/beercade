#!/usr/bin/env bash
# ==============================================================================
# B.O.S.S. SBC QEMU ARM64 Emulator (Raspberry Pi 5 BCM2712 & Rockchip RK3588S)
# Supports both graphical display (via noVNC web browser on port 6080 / VNC 5900)
# and direct headless/CI automation.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Defaults
PLATFORM="rpi5"
HEADLESS=false
CUSTOM_IMG=""
CUSTOM_KERNEL=""
CUSTOM_DTB=""

# Parse command line options
while [[ $# -gt 0 ]]; do
    case "$1" in
        --platform|-p)
            PLATFORM="$2"
            shift 2
            ;;
        --headless|-n)
            HEADLESS=true
            shift
            ;;
        --image|-i)
            CUSTOM_IMG="$2"
            shift 2
            ;;
        --kernel|-k)
            CUSTOM_KERNEL="$2"
            shift 2
            ;;
        --dtb|-d)
            CUSTOM_DTB="$2"
            shift 2
            ;;
        --help|-h)
            echo "Usage: $0 [--platform rpi5|rock5c] [--headless] [--image <path>] [--kernel <path>] [--dtb <path>]"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [ "$PLATFORM" = "rpi5" ]; then
    IMG_FILE="${CUSTOM_IMG:-${SCRIPT_DIR}/dist/beercade-boss-rpi5-v1.img}"
    KERNEL_IMAGE="${CUSTOM_KERNEL:-${ROOT_DIR}/ASTRO/rootfs/boot/kernel_2712.img}"
    
    # Locate device tree
    DTB_FILE="${CUSTOM_DTB:-}"
    if [ -z "$DTB_FILE" ]; then
        for cand in \
            "${ROOT_DIR}/ASTRO/rootfs/boot/bcm2712-rpi-5-b.dtb" \
            "${ROOT_DIR}/dist/boot/bcm2712-rpi-5-b.dtb" \
            "${ROOT_DIR}/build/linux-rockchip/arch/arm64/boot/dts/broadcom/bcm2712-rpi-5-b.dtb"; do
            if [ -f "$cand" ]; then
                DTB_FILE="$cand"
                break
            fi
        done
    fi
    
    QEMU_MACHINE="-M virt,gic-version=2"
    QEMU_CPU="-cpu cortex-a76"
    APPEND_EXTRA="console=ttyAMA0,115200 rootwait earlycon panic=1"
else
    IMG_FILE="${CUSTOM_IMG:-${SCRIPT_DIR}/dist/beercade-boss-rock5c-v1.img}"
    KERNEL_IMAGE="${CUSTOM_KERNEL:-${ROOT_DIR}/ASTRO/rootfs/boot/Image}"
    DTB_FILE="${CUSTOM_DTB:-}"
    QEMU_MACHINE="-M virt,gic-version=3"
    QEMU_CPU="-cpu max"
    APPEND_EXTRA="console=ttyAMA0 console=tty1 quiet splash"
fi

# Fallback image search if primary target not found
if [ ! -f "$IMG_FILE" ]; then
    FALLBACK=$(ls -t "${SCRIPT_DIR}/dist/beercade-boss-"*.img 2>/dev/null | head -n 1 || true)
    if [ -z "$FALLBACK" ]; then
        FALLBACK=$(ls -t "${ROOT_DIR}/"*.img 2>/dev/null | head -n 1 || true)
    fi
    if [ -n "$FALLBACK" ]; then
        echo "[!] Notice: Primary image $IMG_FILE not found; using fallback $FALLBACK"
        IMG_FILE="$FALLBACK"
    else
        echo "[-] Error: Disk image not found at $IMG_FILE"
        exit 1
    fi
fi

# Fallback kernel search
if [ ! -f "$KERNEL_IMAGE" ]; then
    for cand in \
        "${ROOT_DIR}/ASTRO/rootfs/boot/kernel_2712.img" \
        "${ROOT_DIR}/dist/boot/kernel_2712.img" \
        "${ROOT_DIR}/build/linux-rockchip/arch/arm64/boot/Image" \
        "${ROOT_DIR}/ASTRO/rootfs/boot/Image"; do
        if [ -f "$cand" ]; then
            echo "[!] Notice: Kernel $KERNEL_IMAGE not found; using fallback $cand"
            KERNEL_IMAGE="$cand"
            break
        fi
    done
fi

DTB_ARG=()
if [ -n "$DTB_FILE" ] && [ -f "$DTB_FILE" ]; then
    DTB_ARG=("-dtb" "$DTB_FILE")
fi

echo "================================================================="
echo "   🚀 B.E.E.R.C.A.D.E. B.O.S.S. ARM64 SBC Emulator (${PLATFORM^^}) "
echo "================================================================="
echo "   • CPU          : ${QEMU_CPU} (${PLATFORM^^})"
echo "   • Machine      : ${QEMU_MACHINE}"
echo "   • RAM          : 2 GB"
echo "   • Kernel       : ${KERNEL_IMAGE}"
[ -n "$DTB_FILE" ] && echo "   • DTB          : ${DTB_FILE}"
echo "   • Disk         : ${IMG_FILE}"
echo "   • Headless     : ${HEADLESS}"
echo "================================================================="

if [ "$HEADLESS" = false ]; then
    # Start noVNC HTML5 web bridge in background
    NOVNC_DIR=""
    for d in /usr/share/novnc /usr/share/novnc/utils; do
        if [ -d "$d" ]; then
            NOVNC_DIR="$d"
            break
        fi
    done
    if command -v websockify &>/dev/null && [ -n "$NOVNC_DIR" ]; then
        killall -9 websockify 2>/dev/null || true
        websockify --web /usr/share/novnc 6080 localhost:5900 >/dev/null 2>&1 &
        WEBSOCK_PID=$!
        trap "kill -9 $WEBSOCK_PID 2>/dev/null || true" EXIT
    fi

    echo " [*] SBC Virtual Machine is booting... Direct browser to http://localhost:6080/vnc.html"
    echo " [*] Web Kiosk UI: http://localhost:8989"
    qemu-system-aarch64 \
        ${QEMU_MACHINE} \
        ${QEMU_CPU} \
        -smp 4 \
        -m 2048 \
        -kernel "${KERNEL_IMAGE}" \
        "${DTB_ARG[@]}" \
        -append "root=/dev/vda2 rw ${APPEND_EXTRA}" \
        -drive file="${IMG_FILE}",format=raw,if=virtio,file.locking=off \
        -netdev user,id=net0,hostfwd=tcp::8989-:8989,hostfwd=tcp::2222-:22 \
        -device virtio-net-pci,netdev=net0 \
        -device virtio-gpu-pci \
        -device usb-ehci \
        -device usb-kbd \
        -device usb-tablet \
        -vnc 0.0.0.0:0 \
        -serial mon:stdio
else
    # Non-interactive / headless CI mode
    qemu-system-aarch64 \
        ${QEMU_MACHINE} \
        ${QEMU_CPU} \
        -smp 4 \
        -m 2048 \
        -kernel "${KERNEL_IMAGE}" \
        "${DTB_ARG[@]}" \
        -append "root=/dev/vda2 rw ${APPEND_EXTRA}" \
        -drive file="${IMG_FILE}",format=raw,if=virtio,file.locking=off \
        -netdev user,id=net0,hostfwd=tcp::8989-:8989,hostfwd=tcp::2222-:22 \
        -device virtio-net-pci,netdev=net0 \
        -nographic \
        -serial stdio \
        -monitor none
fi
