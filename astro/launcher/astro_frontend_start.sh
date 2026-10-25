#!/bin/sh
# =============================================================================
# Beercade A.S.T.R.O. - Frontend & Attraction Mode Session Starter
# -----------------------------------------------------------------------------
# If ASTRO_ATTRACT_ROM is set (or left default "random"), boots into a curated
# SNES attraction game directly via RetroArch KMS. When RetroArch exits (e.g.,
# coin/service/escape), falls through to ES-DE. If set to "none" or "esde",
# launches ES-DE directly.
# =============================================================================

ROM_DIR="/data/beercade/roms/snes"
DEFAULT_CORE="/usr/lib/libretro/snes9x_libretro.so"
LAUNCH_TARGET="${ASTRO_ATTRACT_ROM:-random}"

# Ensure directories exist
mkdir -p /data/beercade/cache /data/beercade/saves /data/beercade/states /data/beercade/screenshots

# Check if random or specific attract game requested
if [ "$LAUNCH_TARGET" = "random" ]; then
    if [ -d "$ROM_DIR" ]; then
        # Pick a random zip/sfc/smc ROM file from snes ROM directory
        TARGET_ROM=$(find "$ROM_DIR" -maxdepth 1 -type f \( -name "*.zip" -o -name "*.sfc" -o -name "*.smc" \) 2>/dev/null | shuf -n 1)
    fi
elif [ -n "$LAUNCH_TARGET" ] && [ "$LAUNCH_TARGET" != "none" ] && [ "$LAUNCH_TARGET" != "esde" ] && [ -f "$LAUNCH_TARGET" ]; then
    TARGET_ROM="$LAUNCH_TARGET"
fi

if [ -n "$TARGET_ROM" ] && [ -f "$TARGET_ROM" ] && [ -f "$DEFAULT_CORE" ]; then
    echo "beercade-frontend: Launching random attraction mode ROM: $TARGET_ROM" >&2
    /bin/sh /usr/lib/beercade/astro_kms_launcher.sh /usr/bin/retroarch -L "$DEFAULT_CORE" "$TARGET_ROM"
    echo "beercade-frontend: Attraction mode finished, transitioning to ES-DE" >&2
fi

# Fallback or subsequent launch to ES-DE
exec /bin/sh /usr/lib/beercade/astro_kms_launcher.sh /usr/bin/es-de --home /data/beercade --no-splash
