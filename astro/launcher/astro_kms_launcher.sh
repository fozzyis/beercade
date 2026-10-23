#!/bin/sh
# =============================================================================
# Beercade A.S.T.R.O. - Direct-to-KMS libretro launcher (ARM and x86_64)
# -----------------------------------------------------------------------------
# Strips every X11 / Wayland dependency, picks the DRM card that drives a
# display and hands off to RetroArch built with KMSDRM.  Real-time (SCHED_FIFO)
# priority is granted by the beercade udev/limits package.
#
# ASTRO_DEV    override the DRM device node (e.g. /dev/dri/card1)
# ASTRO_SYSFS  sysfs root, only for testing the probe (default /sys)

SYSFS=${ASTRO_SYSFS:-/sys}

# Driver name of a card, from the cardN/device/driver symlink.
card_driver() {
    basename "$(readlink "${SYSFS}/class/drm/$1/device/driver" 2>/dev/null)"
}

is_fb_driver() {
    case "$1" in simple-framebuffer | simpledrm | efifb | vesafb) return 0 ;; esac
    return 1
}

# Print the best card name: first real card with a connected connector, else
# the first real card, else card0.
probe_card() {
    first=
    for c in "${SYSFS}"/class/drm/card[0-9]*; do
        [ -e "$c" ] || continue
        name=${c##*/}
        case "$name" in *-*) continue ;; esac
        is_fb_driver "$(card_driver "$name")" && continue
        [ -n "$first" ] || first=$name
        for s in "$c/$name"-*/status; do
            [ -r "$s" ] || continue
            [ "$(cat "$s")" = connected ] && { echo "$name"; return; }
        done
    done
    echo "${first:-card0}"
}

if [ -n "${ASTRO_DEV}" ]; then
    card=${ASTRO_DEV##*/}
else
    card=$(probe_card)
    ASTRO_DEV=/dev/dri/${card}
fi
idx=${card#card}
drv=$(card_driver "${card}")

# (the node check is skipped when ASTRO_SYSFS points at a fake tree)
test -e "${ASTRO_DEV}" || [ -n "${ASTRO_SYSFS}" ] || {
    echo "beercade-launcher: ${ASTRO_DEV} not present" >&2
    exit 1
}
echo "beercade-launcher: using ${ASTRO_DEV} (driver: ${drv:-unknown})" >&2

# Belt-and-braces: force KMSDRM backend and drop any inherited window server.
unset DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE XAUTHORITY 2>/dev/null
export SDL_VIDEODRIVER=kmsdrm
export SDL_AUDIODRIVER=alsa
export DRM_DEVICE="${ASTRO_DEV}"
export SDL_KMSDRM_DEVICE_INDEX="${idx}"

# amdgpu, i915 and xe need nothing extra; NVIDIA needs its GBM backend.
case "${drv}" in
    nvidia*)
        export GBM_BACKEND=nvidia-drm
        export __GLX_VENDOR_LIBRARY_NAME=nvidia
        ;;
esac

exec "$@"
