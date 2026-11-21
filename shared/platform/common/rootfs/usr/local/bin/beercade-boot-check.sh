#!/usr/bin/env bash
set -euo pipefail

# Post-boot health gate and dual-slot A/B boot blessing for A.S.T.R.O. and B.O.S.S.

check_astro_health() {
  local i
  for i in $(seq 1 30); do
    if systemctl is-active --quiet astro-daemon.service; then
      # Confirm the daemon stays up past immediate startup rather than crash-looping.
      sleep 2
      if systemctl is-active --quiet astro-daemon.service; then
        return 0
      fi
    fi
    sleep 1
  done
  echo "beercade-boot-check: astro-daemon.service is not active" >&2
  return 1
}

check_boss_health() {
  local i
  for i in $(seq 1 5); do
    if systemctl is-active --quiet boss-server.service \
      && curl -fsS -o /dev/null http://127.0.0.1:8080/api/health; then
      return 0
    fi
    sleep 0.2
  done
  echo "beercade-boot-check: boss-server health check failed" >&2
  return 1
}

active_slot_from_cmdline() {
  local token
  for token in $(cat /proc/cmdline); do
    case "$token" in
      root=PARTLABEL=ROOT_A|root=/dev/disk/by-partlabel/ROOT_A|root=/dev/sda2)
        echo "a"
        return 0
        ;;
      root=PARTLABEL=ROOT_B|root=/dev/disk/by-partlabel/ROOT_B|root=/dev/sda3)
        echo "b"
        return 0
        ;;
    esac
  done
  return 1
}

bless_rpi_tryboot() {
  local active
  if ! active="$(active_slot_from_cmdline)"; then
    echo "beercade-boot-check: could not determine active slot from /proc/cmdline; removing stale tryboot.txt" >&2
    rm -f /boot/tryboot.txt
    sync
    return 1
  fi

  if grep -Eq "^[[:space:]]*os_prefix=slot-${active}/([[:space:]]|$)" /boot/tryboot.txt; then
    cp -f /boot/tryboot.txt /boot/config.txt.tmp
    sync
    mv -f /boot/config.txt.tmp /boot/config.txt
    rm -f /boot/tryboot.txt
    sync
    echo "beercade-boot-check: promoted /boot/tryboot.txt (slot-${active}) to /boot/config.txt"
  else
    # Tryboot rolled back to config.txt; discard stale tryboot.txt without overwriting config.txt.
    rm -f /boot/tryboot.txt
    sync
    echo "beercade-boot-check: cleared stale /boot/tryboot.txt after rollback to slot-${active}"
  fi
}

if [[ -f /usr/lib/systemd/system/astro-daemon.service ]]; then
  check_astro_health
elif [[ -f /usr/lib/systemd/system/boss-server.service ]]; then
  check_boss_health
else
  echo "beercade-boot-check: neither astro-daemon.service nor boss-server.service found" >&2
  exit 1
fi

if [[ -d /efi/loader ]]; then
  systemd-bless-boot good
elif [[ -f /boot/tryboot.txt ]]; then
  bless_rpi_tryboot
elif [[ -f /etc/fw_env.config ]] && ! grep -aqE 'brcm,bcm271[12]' /proc/device-tree/compatible 2>/dev/null; then
  if [[ -f /boot/uboot.env ]] && command -v fw_setenv >/dev/null 2>&1; then
    fw_setenv upgrade_available 0
    fw_setenv bootcount 0
  fi
fi
