#!/bin/sh
# =============================================================================
# B.E.E.R.C.A.D.E. A.S.T.R.O. - deploy the RT input + locked-audio latency pack
# -----------------------------------------------------------------------------
# Installs the 1000 Hz evdev udev rules, the SCHED_FIFO(90) limits and the
# rockchipdp0 ALSA hardware endpoint lock onto the target cabinet, then
# reloads the rule set.  Run as root on the Rock 5C.
# =============================================================================
set -e

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

install -m 0644 "$SCRIPT_DIR/udev/92-beercade-input-rt.rules" \
   /etc/udev/rules.d/92-beercade-input-rt.rules
install -m 0644 "$SCRIPT_DIR/udev/93-beercade-audio.rules" \
   /etc/udev/rules.d/93-beercade-audio.rules
install -m 0644 "$SCRIPT_DIR/security/limits.d/50-beercade-rt.conf" \
   /etc/security/limits.d/50-beercade-rt.conf
install -m 0644 "$SCRIPT_DIR/../../rootfs/root/.config/retroarch/retroarch.cfg" \
   /etc/beercade/retroarch_rk3588.cfg

groupadd -f audio >/dev/null 2>&1 || true
getent group beercade >/dev/null 2>&1 || useradd -r -M -G audio beercade

udevadm control --reload-rules
udevadm trigger --subsystem-match=input
udevadm trigger --subsystem-match=sound

echo "beercade: RT input + locked rockchipdp0 audio deployed."