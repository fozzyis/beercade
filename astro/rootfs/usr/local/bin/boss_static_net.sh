#!/bin/sh
# Fallback static network setup for end0 via Netlink
# Runs independently of systemd-networkd
IFACE="end0"
if [ ! -d "/sys/class/net/$IFACE" ]; then
    IFACE=$(ls -1 /sys/class/net | grep -E '^(en|eth|end)' | head -n 1 || true)
fi

if [ -n "$IFACE" ] && [ -d "/sys/class/net/$IFACE" ]; then
    echo "[*] Fallback: Configuring static IP on $IFACE..."
    ip link set "$IFACE" up || true
    ip addr replace 192.168.100.2/24 dev "$IFACE" || true
    ip route replace default via 192.168.100.1 dev "$IFACE" || true
fi
exit 0
