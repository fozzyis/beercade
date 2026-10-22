#!/bin/sh
# =============================================================================
# Beercade A.S.T.R.O. - Direct-to-KMS libretro launcher (RK3588 Panthor)
# -----------------------------------------------------------------------------
# Strips every X11 / Wayland dependency, pins DRM/GBM to /dev/dri/card0 and
# hands off to RetroArch built with KMSDRM.  Real-time (SCHED_FIFO) priority
# is granted by the beercade udev/limits package ships with the input-audio worktree.

ASTRO_DEV=${ASTRO_DEV:-/dev/dri/card0}

test -e "${ASTRO_DEV}" || {
   echo "beercade-launcher: ${ASTRO_DEV} not present" >&2
   exit 1
}

# Belt-and-braces: force DRM backend and drop any inherited window server.
unset DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE XAUTHORITY 2>/dev/null
export SDL_VIDEODRIVER=drm
export SDL_AUDIODRIVER=alsa
export DRM_DEVICE="${ASTRO_DEV}"

exec "$@"