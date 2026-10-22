#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="/data/dev/beercade"
ARCH_TAR="/tmp/beercade_build/ArchLinuxARM-aarch64-latest.tar.gz"
STAGE_DIR="/tmp/astro_rootfs_assembled"
DIST_DIR="${ROOT_DIR}/dist"
TARGET_PART="/dev/sda2"

echo "================================================================="
echo "   B.E.E.R.C.A.D.E. A.S.T.R.O. Master RootFS Assembly & Build   "
echo "================================================================="

echo "[+] Cleaning & recreating staging directory: ${STAGE_DIR}..."
sudo rm -rf "${STAGE_DIR}"

if [ -f "${ARCH_TAR}" ]; then
    echo "[+] Extracting Arch Linux ARM base rootfs from tarball..."
    sudo mkdir -p "${STAGE_DIR}"
    sudo bsdtar -xpf "${ARCH_TAR}" -C "${STAGE_DIR}"
elif [ -f "${DIST_DIR}/astro-rootfs-slotA.squashfs" ]; then
    echo "[+] Unsquashing existing base rootfs image from ${DIST_DIR}/astro-rootfs-slotA.squashfs..."
    sudo unsquashfs -d "${STAGE_DIR}" "${DIST_DIR}/astro-rootfs-slotA.squashfs"
else
    echo "[-] Neither ${ARCH_TAR} nor ${DIST_DIR}/astro-rootfs-slotA.squashfs found."
    exit 1
fi

echo "[+] Overlaying A.S.T.R.O. cross-compiled binaries & libraries..."
sudo rsync -aHAX --keep-dirlinks "${ROOT_DIR}/ASTRO/rootfs/" "${STAGE_DIR}/"

echo "[+] Installing custom arcade daemons and launchers..."
sudo mkdir -p "${STAGE_DIR}/usr/local/bin" "${STAGE_DIR}/etc/beercade" "${STAGE_DIR}/etc/security/limits.d" "${STAGE_DIR}/etc/udev/rules.d"

# 1. astro-daemon
if [ -f "${ROOT_DIR}/astro-daemon/target/aarch64-unknown-linux-gnu/release/astro-daemon" ]; then
    sudo cp "${ROOT_DIR}/astro-daemon/target/aarch64-unknown-linux-gnu/release/astro-daemon" "${STAGE_DIR}/usr/local/bin/"
    sudo chmod +x "${STAGE_DIR}/usr/local/bin/astro-daemon"
fi

# 2. KMS Launcher & Optimizer
if [ -f "${ROOT_DIR}/beercade-kms-frontend/astro-kms-launcher/astro_kms_launcher.sh" ]; then
    sudo cp "${ROOT_DIR}/beercade-kms-frontend/astro-kms-launcher/astro_kms_launcher.sh" "${STAGE_DIR}/usr/local/bin/astro_kms_launcher"
    sudo chmod +x "${STAGE_DIR}/usr/local/bin/astro_kms_launcher"
fi

if [ -f "${ROOT_DIR}/emu_opti/rk3588_optimizer.sh" ]; then
    sudo cp "${ROOT_DIR}/emu_opti/rk3588_optimizer.sh" "${STAGE_DIR}/usr/local/bin/"
    sudo chmod +x "${STAGE_DIR}/usr/local/bin/rk3588_optimizer.sh"
fi

# 3. Udev Rules & RT Security Limits
sudo cp -f "${ROOT_DIR}"/beercade-input-audio/deploy/udev/*.rules "${STAGE_DIR}/etc/udev/rules.d/" 2>/dev/null || true
sudo cp -f "${ROOT_DIR}"/emu_opti/*.rules "${STAGE_DIR}/etc/udev/rules.d/" 2>/dev/null || true
sudo cp -f "${ROOT_DIR}"/beercade-input-audio/deploy/security/limits.d/*.conf "${STAGE_DIR}/etc/security/limits.d/" 2>/dev/null || true

# 4. RetroArch Configurations & Controller Autoconfig
sudo mkdir -p "${STAGE_DIR}/etc/beercade" "${STAGE_DIR}/root/.config/retroarch/autoconfig"
if [ -f "${ROOT_DIR}/ASTRO/rootfs/root/.config/retroarch/retroarch.cfg" ]; then
    sudo cp -f "${ROOT_DIR}/ASTRO/rootfs/root/.config/retroarch/retroarch.cfg" "${STAGE_DIR}/root/.config/retroarch/retroarch.cfg"
    sudo cp -f "${ROOT_DIR}/ASTRO/rootfs/root/.config/retroarch/retroarch.cfg" "${STAGE_DIR}/etc/beercade/retroarch_rk3588.cfg"
else
    sudo cp -f "${ROOT_DIR}/beercade-input-audio/deploy/retroarch_rk3588.cfg" "${STAGE_DIR}/etc/beercade/retroarch_rk3588.cfg" 2>/dev/null || true
    sudo cp -f "${ROOT_DIR}/beercade-input-audio/deploy/retroarch_rk3588.cfg" "${STAGE_DIR}/root/.config/retroarch/retroarch.cfg" 2>/dev/null || true
fi
if [ -d "${ROOT_DIR}/ASTRO/rootfs/root/.config/retroarch/autoconfig" ]; then
    sudo cp -rf "${ROOT_DIR}/ASTRO/rootfs/root/.config/retroarch/autoconfig/"* "${STAGE_DIR}/root/.config/retroarch/autoconfig/" 2>/dev/null || true
fi
if [ -d "${ROOT_DIR}/ASTRO/rootfs/root/ES-DE" ]; then
    sudo cp -rf "${ROOT_DIR}/ASTRO/rootfs/root/ES-DE" "${STAGE_DIR}/root/" 2>/dev/null || true
fi
# Ensure ROMs symlink to /mnt/data/roms
sudo ln -sf /mnt/data/roms "${STAGE_DIR}/root/ROMs"

# 4b. Panthor Mali GPU Firmware
sudo mkdir -p "${STAGE_DIR}/lib/firmware/arm/mali/arch10.8" "${STAGE_DIR}/lib/firmware/arm/mali/arch10.10"
if [ -f "${ROOT_DIR}/ASTRO/rootfs/lib/firmware/arm/mali/arch10.8/mali_csffw.bin" ]; then
    sudo cp -f "${ROOT_DIR}/ASTRO/rootfs/lib/firmware/arm/mali/arch10.8/mali_csffw.bin" "${STAGE_DIR}/lib/firmware/arm/mali/arch10.8/"
    sudo cp -f "${ROOT_DIR}/ASTRO/rootfs/lib/firmware/arm/mali/arch10.8/mali_csffw.bin" "${STAGE_DIR}/lib/firmware/arm/mali/arch10.10/"
fi

# 4c. Patched multimedia libraries (glibc 2.43 compatibility)
if [ -f "${ROOT_DIR}/ASTRO/rootfs/usr/lib/libavutil.so.58.29.100" ]; then
    echo "[+] Installing patched libavutil.so.58 (glibc 2.43 compatible)..."
    sudo cp -f "${ROOT_DIR}/ASTRO/rootfs/usr/lib/libavutil.so.58.29.100" "${STAGE_DIR}/usr/lib/libavutil.so.58.29.100"
    sudo chmod 755 "${STAGE_DIR}/usr/lib/libavutil.so.58.29.100"
    sudo ln -sf libavutil.so.58.29.100 "${STAGE_DIR}/usr/lib/libavutil.so.58"
    sudo ln -sf libavutil.so.58.29.100 "${STAGE_DIR}/usr/lib/libavutil.so"
fi

# 5. Network Configuration & Static Systemd Daemons
echo "[+] Installing wildcard networkd configuration and mDNS resolution..."
sudo mkdir -p "${STAGE_DIR}/etc/systemd/network" "${STAGE_DIR}/etc/systemd/resolved.conf.d"
cat << 'EOF' | sudo tee "${STAGE_DIR}/etc/systemd/network/10-wired.network" > /dev/null
[Match]
Name=eth* en* end* lan*
Type=ether

[Network]
DHCP=yes
DNSSEC=no
LinkLocalAddressing=fallback
MulticastDNS=yes
LLMNR=yes

[DHCPv4]
MaxAttempts=infinity
UseDNS=yes
RouteMetric=100

[DHCPv6]
UseDNS=yes
RouteMetric=100
EOF

# Clean up legacy configs to ensure 10-wired.network takes precedence
sudo rm -f "${STAGE_DIR}/etc/systemd/network/en.network" "${STAGE_DIR}/etc/systemd/network/eth.network"

cat << 'EOF' | sudo tee "${STAGE_DIR}/etc/systemd/resolved.conf.d/mdns.conf" > /dev/null
[Resolve]
MulticastDNS=yes
LLMNR=yes
DNSSEC=no
EOF

# Hostname and local hosts
echo "beercade-astro" | sudo tee "${STAGE_DIR}/etc/hostname" > /dev/null
cat << 'EOF' | sudo tee "${STAGE_DIR}/etc/hosts" > /dev/null
127.0.0.1       localhost
::1             localhost
127.0.1.1       beercade-astro
EOF

# Static systemd wants symlinks for rootfs
WANTS_DIR="${STAGE_DIR}/etc/systemd/system/multi-user.target.wants"
sudo mkdir -p "$WANTS_DIR"
sudo ln -sf /usr/lib/systemd/system/sshd.service "$WANTS_DIR/sshd.service"
sudo ln -sf /usr/lib/systemd/system/avahi-daemon.service "$WANTS_DIR/avahi-daemon.service"
sudo ln -sf /usr/lib/systemd/system/systemd-networkd.service "$WANTS_DIR/systemd-networkd.service"
sudo ln -sf /usr/lib/systemd/system/systemd-resolved.service "$WANTS_DIR/systemd-resolved.service"

# 6. SSH Daemon, Host Keys & Privilege Separation
echo "[+] Configuring SSH host keys, privilege separation and root access..."
sudo mkdir -p "${STAGE_DIR}/etc/ssh"
if [ ! -f "${STAGE_DIR}/etc/ssh/ssh_host_rsa_key" ]; then
    sudo ssh-keygen -q -N "" -t rsa -b 3072 -f "${STAGE_DIR}/etc/ssh/ssh_host_rsa_key"
fi
if [ ! -f "${STAGE_DIR}/etc/ssh/ssh_host_ecdsa_key" ]; then
    sudo ssh-keygen -q -N "" -t ecdsa -b 256 -f "${STAGE_DIR}/etc/ssh/ssh_host_ecdsa_key"
fi
if [ ! -f "${STAGE_DIR}/etc/ssh/ssh_host_ed25519_key" ]; then
    sudo ssh-keygen -q -N "" -t ed25519 -f "${STAGE_DIR}/etc/ssh/ssh_host_ed25519_key"
fi
sudo chmod 600 "${STAGE_DIR}/etc/ssh"/ssh_host_*_key
sudo chmod 644 "${STAGE_DIR}/etc/ssh"/ssh_host_*_key.pub
sudo chown 0:0 "${STAGE_DIR}/etc/ssh"/ssh_host_*

sudo mkdir -p "${STAGE_DIR}/var/empty" "${STAGE_DIR}/run/sshd"
sudo chmod 755 "${STAGE_DIR}/var/empty" "${STAGE_DIR}/run/sshd"
sudo chown 0:0 "${STAGE_DIR}/var/empty" "${STAGE_DIR}/run/sshd"

sudo mkdir -p "${STAGE_DIR}/root/.ssh"
if [ -f "/home/outl4w/.ssh/id_ed25519.pub" ]; then
    sudo cp /home/outl4w/.ssh/id_ed25519.pub "${STAGE_DIR}/root/.ssh/authorized_keys"
    sudo chmod 600 "${STAGE_DIR}/root/.ssh/authorized_keys"
    sudo chown -R 0:0 "${STAGE_DIR}/root/.ssh"
fi

if [ -f "${STAGE_DIR}/etc/ssh/sshd_config" ]; then
    sudo sed -i 's/^#PermitRootLogin.*/PermitRootLogin yes/' "${STAGE_DIR}/etc/ssh/sshd_config"
    sudo sed -i 's/^PermitRootLogin.*/PermitRootLogin yes/' "${STAGE_DIR}/etc/ssh/sshd_config"
    sudo sed -i 's/^#PubkeyAuthentication.*/PubkeyAuthentication yes/' "${STAGE_DIR}/etc/ssh/sshd_config"
fi

# 7. Systemd TTY1 Direct KMS Auto-login & ES-DE Launcher
sudo mkdir -p "${STAGE_DIR}/etc/systemd/system/getty@tty1.service.d"
cat << 'EOF' | sudo tee "${STAGE_DIR}/etc/systemd/system/getty@tty1.service.d/autologin.conf" > /dev/null
[Service]
ExecStart=
ExecStart=-/usr/bin/agetty --autologin root --noclear %I $TERM
EOF

cat << 'EOF' | sudo tee "${STAGE_DIR}/etc/issue" > /dev/null

********************************************************
  🚀 B.E.E.R.C.A.D.E. A.S.T.R.O. (Radxa Rock 5C RK3588S)
********************************************************
  IPv4 Address : \4
  mDNS Name    : beercade-astro.local
  SSH Access   : ssh root@beercade-astro.local (or root@\4)
********************************************************

EOF

cat << 'EOF' | sudo tee "${STAGE_DIR}/root/.bash_profile" > /dev/null
# Disable stalled RK3588 Big Cores (CPU 4-7) for full speed emulation
for c in 4 5 6 7; do echo 0 > /sys/devices/system/cpu/cpu$c/online 2>/dev/null || true; done

if [ -z "$DISPLAY" ] && [ "$(tty)" = "/dev/tty1" ]; then
    export ASTRO_STANDALONE=1
    export BOSS_ENDPOINT="none"

    # Run RK3588 CPU/GPU RT optimizers
    if [ -x /usr/local/bin/rk3588_optimizer.sh ]; then
        /usr/local/bin/rk3588_optimizer.sh || true
    fi
    
    # Start astro-daemon in standalone mode
    if [ -x /usr/local/bin/astro-daemon ]; then
        /usr/local/bin/astro-daemon &
    fi
    
    # KMS DRM Environment
    export SDL_VIDEODRIVER=kmsdrm
    export SDL_AUDIODRIVER=alsa
    export DRM_DEVICE=/dev/dri/card0

    # -----------------------------------------------------------------
    # Network Initialization & IP Telemetry Hook
    # -----------------------------------------------------------------
    echo -n "[*] Initializing network interface & detecting IP..."
    PHYS_IFACE=$(ip -o link show | awk -F': ' '$2 ~ /^(eth|en|end|lan)/ {print $2; exit}')

    CURRENT_IP=""
    if [ -n "$PHYS_IFACE" ]; then
        ip link set "$PHYS_IFACE" up 2>/dev/null || true
        
        # Poll up to 3 seconds for DHCP / Link-Local address assignment
        for i in 1 2 3; do
            CURRENT_IP=$(ip -4 -brief addr show dev "$PHYS_IFACE" scope global 2>/dev/null | awk '{print $3}' | cut -d'/' -f1)
            if [ -n "$CURRENT_IP" ]; then
                break
            fi
            sleep 1
            echo -n "."
        done
        echo ""

        # Fail-safe: If still unassigned (router dead/direct link), assign fallback IP:
        if [ -z "$CURRENT_IP" ]; then
            ip addr add 192.168.50.155/24 dev "$PHYS_IFACE" 2>/dev/null || true
            CURRENT_IP="192.168.50.155 (static fallback)"
        fi
    fi

    # Export IP for astro-daemon / ES-DE HUD telemetry
    ALL_IPS=$(ip -4 -brief addr show scope global 2>/dev/null | awk '{print $1 ": " $3}' | paste -sd ' | ' -)
    echo "${ALL_IPS:-127.0.0.1}" > /tmp/astro_ip.txt

    # Ensure sshd is running
    if ! pgrep -x sshd > /dev/null; then
        systemctl start sshd 2>/dev/null || /usr/sbin/sshd || true
    fi

    echo "================================================================="
    echo "       🚀 B.E.E.R.C.A.D.E. A.S.T.R.O. [STANDALONE CONSOLE]      "
    echo "================================================================="
    echo " [*] Direct KMS environment initialized on /dev/tty1."
    echo " [*] Autologin successful. Shell is permanent and stable."
    echo "-----------------------------------------------------------------"
    echo " 🌐 Active IP(s)  : ${ALL_IPS:-None detected}"
    echo " 🏷️  mDNS Host     : beercade-astro.local"
    echo " 🔑 SSH Access    : ssh root@${CURRENT_IP:-beercade-astro.local}"
    echo "-----------------------------------------------------------------"
    echo " [*] Commands available:"
    echo "     - 'es-de'           : Launch EmulationStation Desktop Edition"
    echo "     - 'retroarch'       : Launch RetroArch KMS frontend"
    echo "     - 'astro_triage.sh' : Run full hardware & DRM diagnostics"
    echo "================================================================="

    # Persistent home directory on /mnt/data if available
    if [ -d /mnt/data ] && ! mountpoint -q /root; then
        mkdir -p /mnt/data/root /mnt/data/root/.ssh /mnt/data/root/.config
        cp -a /root/.bash_profile /mnt/data/root/.bash_profile
        [ -d /root/.ssh ] && cp -an /root/.ssh/* /mnt/data/root/.ssh/ 2>/dev/null || true
        [ -d /root/ES-DE ] && cp -an /root/ES-DE /mnt/data/root/ 2>/dev/null || true
        [ -d /root/.config ] && cp -an /root/.config/* /mnt/data/root/.config/ 2>/dev/null || true
        ln -sf /mnt/data/roms /mnt/data/root/ROMs 2>/dev/null || true
        mount --bind /mnt/data/root /root 2>/dev/null || true
    fi

    if [ "${ASTRO_AUTOLAUNCH:-1}" = "1" ]; then
        echo ""
        echo "[*] Launching ES-DE in 3 seconds (press any key to stay in root shell)..."
        if read -t 3 -n 1 -r; then
            echo "[*] Autolaunch cancelled. Welcome to interactive root shell."
        else
            echo ""
            if [ -x /usr/bin/es-de ]; then
                echo "[+] Starting ES-DE in foreground on /dev/tty1..."
                /usr/bin/es-de || echo "[!] ES-DE exited ($?). Dropping to interactive root shell."
            elif [ -x /usr/bin/retroarch ]; then
                echo "[+] Starting RetroArch in KMS/DRM mode (OpenGL ES)..."
                /usr/bin/retroarch -c /etc/beercade/retroarch_rk3588.cfg || echo "[!] RetroArch exited ($?). Dropping to interactive root shell."
            fi
        fi
    fi

    # Prevent agetty loop on exit
    exec /bin/bash
fi
EOF

# 7. Create Dedicated Direct-KMS Frontend Systemd Service (Disabled or rate-limited during triage)
sudo mkdir -p "${STAGE_DIR}/etc/systemd/system/multi-user.target.wants"
sudo tee "${STAGE_DIR}/etc/systemd/system/beercade-frontend.service" > /dev/null << 'EOF'
[Unit]
Description=B.E.E.R.C.A.D.E. A.S.T.R.O. Direct KMS Game Frontend
After=systemd-user-sessions.service rc-local.service dev-dri-card0.device
Wants=dev-dri-card0.device
Conflicts=getty@tty1.service

[Service]
Type=simple
User=root
WorkingDirectory=/root
Environment=SDL_VIDEODRIVER=kmsdrm
Environment=SDL_AUDIODRIVER=alsa
Environment=DRM_DEVICE=/dev/dri/card0
Environment=XDG_RUNTIME_DIR=/run/user/0
ExecStartPre=-/usr/local/bin/rk3588_optimizer.sh
ExecStart=/usr/bin/es-de
Restart=on-failure
RestartSec=10s
StartLimitIntervalSec=60
StartLimitBurst=3
StandardInput=tty
StandardOutput=journal+console
StandardError=journal+console
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes

[Install]
WantedBy=multi-user.target
EOF

# 8. Automated On-Boot SD Card Diagnostics Harvester
# Runs early, stops the flapping frontend, gathers full hardware/DRM/Panthor state,
# writes logs to /boot/ (SD card partition 1), and syncs disk.
sudo cp -f "${ROOT_DIR}/ASTRO/scripts/triage.sh" "${STAGE_DIR}/usr/local/bin/astro_triage.sh" 2>/dev/null || true
sudo chmod +x "${STAGE_DIR}/usr/local/bin/astro_triage.sh" 2>/dev/null || true

sudo tee "${STAGE_DIR}/etc/systemd/system/beercade-autotriage.service" > /dev/null << 'EOF'
[Unit]
Description=B.E.E.R.C.A.D.E. A.S.T.R.O. Autonomous Boot Triage & SD Logger
After=local-fs.target systemd-udevd.service
Before=multi-user.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/bash -c '\
    DATA_MNT="/mnt/data"; \
    mkdir -p "$DATA_MNT"; \
    mount /dev/mmcblk1p4 "$DATA_MNT" 2>/dev/null || \
    mount /dev/mmcblk0p4 "$DATA_MNT" 2>/dev/null || \
    mount /dev/sda4 "$DATA_MNT" 2>/dev/null || true; \
    \
    LOG_DIR="$DATA_MNT/astro_bootlogs"; \
    mkdir -p "$LOG_DIR"; \
    \
    echo "[*] Collecting early boot dmesg..." > "$LOG_DIR/status.txt"; \
    dmesg > "$LOG_DIR/dmesg.log" 2>&1; \
    journalctl -b 0 --no-pager > "$LOG_DIR/journal.log" 2>&1 || true; \
    cat /proc/cmdline > "$LOG_DIR/cmdline.txt" 2>&1; \
    \
    if [ -x /usr/local/bin/astro_triage.sh ]; then \
        echo "[*] Executing deep DRM/KMS hardware triage..." >> "$LOG_DIR/status.txt"; \
        /usr/local/bin/astro_triage.sh > "$LOG_DIR/triage.log" 2>&1 || true; \
    fi; \
    \
    echo "[*] DRM Connector status:" >> "$LOG_DIR/status.txt"; \
    for c in /sys/class/drm/card*-*; do \
        [ -d "$c" ] && echo "$(basename $c): $(cat $c/status 2>/dev/null || echo unknown)" >> "$LOG_DIR/status.txt"; \
    done; \
    \
    echo "[✓] Boot triage complete at $(date -Iseconds)" >> "$LOG_DIR/status.txt"; \
    sync; sync; \
'

[Install]
WantedBy=multi-user.target
EOF

sudo ln -sf /etc/systemd/system/beercade-autotriage.service "${STAGE_DIR}/etc/systemd/system/multi-user.target.wants/beercade-autotriage.service"

# 9. PAM Fix for Read-Only RootFS: Disable pam_lastlog2 SQLite database
if [ -f "${STAGE_DIR}/etc/pam.d/system-login" ]; then
    echo "[+] Disabling pam_lastlog2 SQLite database lock in PAM system-login..."
    sudo sed -i 's/.*pam_lastlog2.so.*/# &/' "${STAGE_DIR}/etc/pam.d/system-login"
fi

# 10. Writable Tmpfs Mounts in /etc/fstab for Read-Only Appliance
echo "[+] Configuring volatile tmpfs mounts in /etc/fstab and pre-creating mountpoints..."
sudo mkdir -p "${STAGE_DIR}/mnt/data" "${STAGE_DIR}/var/lib/lastlog" "${STAGE_DIR}/var/log" \
              "${STAGE_DIR}/var/lib/systemd" "${STAGE_DIR}/var/lib/private" \
              "${STAGE_DIR}/var/lib/machines" "${STAGE_DIR}/var/lib/portables"
cat << 'EOF' | sudo tee "${STAGE_DIR}/etc/fstab" > /dev/null
# B.E.E.R.C.A.D.E. A.S.T.R.O. Appliance Filesystems
tmpfs   /tmp             tmpfs   defaults,nosuid,nodev,noatime,mode=1777 0 0
tmpfs   /var/log         tmpfs   defaults,nosuid,nodev,noatime,mode=0755 0 0
tmpfs   /var/tmp         tmpfs   defaults,nosuid,nodev,noatime,mode=1777 0 0
tmpfs   /var/lib/lastlog tmpfs   defaults,nosuid,nodev,noatime,mode=0755 0 0
tmpfs   /var/lib/systemd tmpfs   defaults,nosuid,nodev,noatime,mode=0755 0 0
tmpfs   /var/lib/private tmpfs   defaults,nosuid,nodev,noatime,mode=0700 0 0
tmpfs   /var/lib/machines tmpfs  defaults,nosuid,nodev,noatime,mode=0700 0 0
tmpfs   /var/lib/portables tmpfs defaults,nosuid,nodev,noatime,mode=0700 0 0
EOF

# Set hostname
echo "beercade-astro" | sudo tee "${STAGE_DIR}/etc/hostname" > /dev/null

echo "[+] Packaging full LZ4-compressed Read-Only SquashFS image..."
mkdir -p "${DIST_DIR}"
sudo rm -f "${DIST_DIR}/astro-rootfs-slotA.squashfs"
sudo mksquashfs "${STAGE_DIR}" "${DIST_DIR}/astro-rootfs-slotA.squashfs" -comp lz4 -noappend -wildcards

echo "================================================================="
echo "   [✓] Full Appliance RootFS Built: ${DIST_DIR}/astro-rootfs-slotA.squashfs"
echo "================================================================="

if [ -b "${TARGET_PART}" ]; then
    echo "[+] Target SD partition ${TARGET_PART} detected. Flashing..."
    sudo dd if="${DIST_DIR}/astro-rootfs-slotA.squashfs" of="${TARGET_PART}" bs=4M status=progress
    sudo sync
    echo "[✓] RootFS Flashed to ${TARGET_PART}."
else
    echo "[!] Target ${TARGET_PART} not currently mounted. Ready to flash when SD card is attached."
fi
