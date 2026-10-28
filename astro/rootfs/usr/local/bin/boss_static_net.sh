#!/bin/sh
# Fallback static network setup for end0 via Netlink
# Runs independently of systemd-networkd
IFACE="end0"
if [ ! -d "/sys/class/net/$IFACE" ]; then
    IFACE=$(ls -1 /sys/class/net | grep -E '^(en|eth|end)' | head -n 1 || true)
fi

if [ -n "$IFACE" ] && [ -d "/sys/class/net/$IFACE" ]; then
    echo "[*] Fallback: Bringing up link on $IFACE..."
    ip link set "$IFACE" up || true
fi
exit 0
