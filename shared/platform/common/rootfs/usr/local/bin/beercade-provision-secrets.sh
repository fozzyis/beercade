#!/usr/bin/env bash
# First-boot CSPRNG secret provisioning for B.O.S.S. and A.S.T.R.O.
# Populates blank secret keys in /data/beercade/{boss,astro}.env atomically
# without writing secrets to stdout/journald.
set -euo pipefail

rand_hex32() {
  od -An -tx1 -N32 /dev/urandom | tr -d ' \n'
}

rand_pin6() {
  local pin=""
  while [[ ${#pin} -lt 6 ]]; do
    pin+=$(head -c 64 /dev/urandom | tr -dc '0-9' || true)
  done
  printf '%s' "${pin:0:6}"
}

provision_kv() {
  local file="$1"
  local key="$2"
  local val="$3"
  local tmp="${file}.tmp.$$"

  awk -v k="$key" -v v="$val" '
    BEGIN { found = 0 }
    $0 ~ ("^" k "=") {
      found = 1
      cur = substr($0, length(k) + 2)
      if (cur == "" || cur == "beercade-dev-token" || cur == "898900") {
        print k "=" v
      } else {
        print $0
      }
      next
    }
    { print $0 }
    END { if (!found) print k "=" v }
  ' "$file" >"$tmp"
  sync "$tmp" 2>/dev/null || true
  mv -f "$tmp" "$file"
}

if [[ -f /data/beercade/boss.env ]]; then
  provision_kv /data/beercade/boss.env BOSS_RPC_TOKEN "$(rand_hex32)"
  provision_kv /data/beercade/boss.env BOSS_NODE_TOKEN "$(rand_hex32)"
  provision_kv /data/beercade/boss.env BOSS_OPERATOR_PIN "$(rand_pin6)"
  provision_kv /data/beercade/boss.env BOSS_DISCOVERY_KEY "$(rand_hex32)"
  if getent group boss >/dev/null 2>&1; then
    chown root:boss /data/beercade/boss.env
    chmod 0640 /data/beercade/boss.env
  else
    chown root:root /data/beercade/boss.env
    chmod 0600 /data/beercade/boss.env
  fi
fi

if [[ -f /data/beercade/astro.env ]]; then
  provision_kv /data/beercade/astro.env ASTRO_DISCOVERY_KEY "$(rand_hex32)"
  chown root:root /data/beercade/astro.env
  chmod 0600 /data/beercade/astro.env
fi
