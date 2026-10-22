#!/usr/bin/env bash
# ==============================================================================
# B.O.S.S. (B.E.E.R.C.A.D.E. Operations & Supervisory System) Native Linux Installer
# Installs boss-server daemon and optionally the Wayland Cage touchscreen kiosk.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOSS_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

INSTALL_KIOSK=0

for arg in "$@"; do
    case $arg in
        --with-kiosk)
            INSTALL_KIOSK=1
            shift
            ;;
        --help|-h)
            echo "Usage: sudo ./install_boss.sh [options]"
            echo "Options:"
            echo "  --with-kiosk    Install Wayland Cage compositor + Chromium kiosk mode"
            echo "  --help, -h      Show this help message"
            exit 0
            ;;
    esac
done

if [ "$EUID" -ne 0 ]; then
    echo "[-] Error: Please run this script with sudo or as root."
    exit 1
fi

echo "================================================================="
echo "   🚀 Installing B.E.E.R.C.A.D.E. B.O.S.S. on Native Linux       "
echo "================================================================="

# 1. Package Dependencies
echo "[*] Detecting package manager and installing dependencies..."
if command -v apt-get &>/dev/null; then
    apt-get update -qq
    apt-get install -y --no-install-recommends curl ca-certificates libssl3
    if [ "$INSTALL_KIOSK" -eq 1 ]; then
        apt-get install -y --no-install-recommends cage chromium libinput-bin
    fi
elif command -v pacman &>/dev/null; then
    pacman -S --needed --noconfirm curl ca-certificates openssl
    if [ "$INSTALL_KIOSK" -eq 1 ]; then
        pacman -S --needed --noconfirm cage chromium libinput
    fi
elif command -v dnf &>/dev/null; then
    dnf install -y curl ca-certificates openssl-libs
    if [ "$INSTALL_KIOSK" -eq 1 ]; then
        dnf install -y cage chromium
    fi
fi

# 2. Build or verify binary
BIN_PATH="${BOSS_ROOT}/target/release/boss-server"
if [ ! -f "$BIN_PATH" ]; then
    echo "[*] Binary not found at $BIN_PATH. Attempting to build with cargo..."
    if command -v cargo &>/dev/null; then
        (cd "$BOSS_ROOT" && cargo build --release)
    else
        echo "[-] Error: boss-server binary not found and cargo is not installed."
        echo "[-] Please build boss-server first: (cd BOSS && cargo build --release)"
        exit 1
    fi
fi

# 3. Build or verify Kiosk UI static assets
UI_BUILD_PATH="${BOSS_ROOT}/kiosk_ui/build"
if [ ! -d "$UI_BUILD_PATH" ] || [ ! -f "$UI_BUILD_PATH/index.html" ]; then
    echo "[*] UI assets not found at $UI_BUILD_PATH. Attempting to build with npm..."
    if command -v npm &>/dev/null; then
        (cd "${BOSS_ROOT}/kiosk_ui" && npm install && npm run build)
    else
        echo "[-] Error: UI assets not built and npm is not installed."
        echo "[-] Please build kiosk_ui first: (cd BOSS/kiosk_ui && npm install && npm run build)"
        exit 1
    fi
fi

# 4. Install files
echo "[*] Installing boss-server binary to /usr/local/bin..."
install -m 755 "$BIN_PATH" /usr/local/bin/boss-server

echo "[*] Installing Kiosk UI static assets to /usr/share/boss/kiosk_ui..."
mkdir -p /usr/share/boss/kiosk_ui
cp -rf "${UI_BUILD_PATH}"/* /usr/share/boss/kiosk_ui/

echo "[*] Setting up persistent storage at /data/beercade/sync..."
mkdir -p /data/beercade/sync
chmod 777 /data/beercade/sync

# 5. Install systemd services
echo "[*] Installing systemd service units..."
cp -f "${SCRIPT_DIR}/boss-server.service" /etc/systemd/system/boss-server.service

if [ "$INSTALL_KIOSK" -eq 1 ]; then
    echo "[*] Installing boss-kiosk.service..."
    cp -f "${SCRIPT_DIR}/boss-kiosk.service" /etc/systemd/system/boss-kiosk.service
fi

systemctl daemon-reload
systemctl enable --now boss-server.service

if [ "$INSTALL_KIOSK" -eq 1 ]; then
    systemctl enable --now boss-kiosk.service
fi

echo "================================================================="
echo "   [✓] B.O.S.S. Installation Complete!                          "
echo "   - Server Status: systemctl status boss-server                "
echo "   - Web Endpoint : http://localhost:8989                       "
if [ "$INSTALL_KIOSK" -eq 1 ]; then
    echo "   - Touchscreen Kiosk: Active on display (boss-kiosk.service)  "
fi
echo "================================================================="
