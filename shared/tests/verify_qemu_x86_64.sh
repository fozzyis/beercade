#!/usr/bin/env bash
# Boot-verify the x86_64 A.S.T.R.O. image under QEMU + OVMF (TCG) in a Docker container.
# Usage: verify_qemu_x86_64.sh [image]   (default out/beercade-astro-x86_64.raw)
# Env:   VERIFY_TIMEOUT (seconds, default 300), QEMU_IMAGE (base image, default debian:bookworm)
# Exit:  0 pass, 1 fail, 2 timeout
#
# systemd stops printing unit status once a getty owns the serial console, so
# the log alone cannot prove multi-user.target. A systemd credential passed via
# SMBIOS (test VM only, the image is unchanged) adds a root autologin drop-in
# for serial-getty@ttyS0; the script then asks systemctl over the serial line.
set -euo pipefail
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL='*'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

IMG="${1:-out/beercade-astro-x86_64.raw}"
TIMEOUT="${VERIFY_TIMEOUT:-300}"
BASE_IMAGE="${QEMU_IMAGE:-debian:bookworm}"
TAG="beercade-qemu-x86"
OUT_DIR="$REPO_ROOT/out"
LOG="$OUT_DIR/qemu-serial-x86_64.log"
CLEAN="$OUT_DIR/qemu-serial-x86_64.clean.log"
CNAME="beercade-verify-x86-$$-$(date +%s)"

PASS_RE='VERIFY active,active END'
FAIL_RE='Kernel panic|Failed to start astro-daemon|astro-daemon\.service: Failed|astro-daemon\.service: Main process exited|^ERROR: |dropped into a recovery shell|You are now being dropped into an emergency shell'

# Host paths for docker bind mounts (pwd -W gives D:/... under Git Bash).
hostpath() {
  local d; d="$(cd "$(dirname "$1")" && { pwd -W 2>/dev/null || pwd; })"
  printf '%s/%s' "$d" "$(basename "$1")"
}

if [ ! -f "$IMG" ]; then
  echo "FAIL: image not found: $IMG (run 'make image-x86' first)" >&2
  exit 1
fi
command -v docker >/dev/null || { echo "FAIL: docker not found" >&2; exit 1; }

mkdir -p "$OUT_DIR"
: > "$LOG"
H_IMG="$(hostpath "$IMG")"
H_OUT="$(hostpath "$OUT_DIR")"

cleanup() {
  docker rm -f "$CNAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Build (once) a small cached image with qemu + ovmf.
if ! docker image inspect "$TAG" >/dev/null 2>&1; then
  echo "Building helper image $TAG from $BASE_IMAGE ..."
  printf 'FROM %s\nRUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends qemu-system-x86 ovmf ca-certificates && rm -rf /var/lib/apt/lists/*\n' "$BASE_IMAGE" \
    | docker build -q -t "$TAG" -
fi

DROPIN_B64="$(base64 -w0 <<'EOF'
[Service]
ExecStart=
ExecStart=-/sbin/agetty -o '-p -f -- \\u' --noclear --autologin root --keep-baud 115200 %I $TERM
EOF
)"

# shellcheck disable=SC2016
INNER='set -e
cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/vars.fd
mkfifo /tmp/ser.in /tmp/ser.out
cat /tmp/ser.out > /dev/null &
exec qemu-system-x86_64 -machine q35 -m 2048 -smp 2 -accel tcg \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file=/tmp/vars.fd \
  -drive file=/image.raw,if=virtio,format=raw,snapshot=on \
  -nic user,model=virtio-net-pci \
  -smbios type=11,value=io.systemd.credential.binary:systemd.unit-dropin.serial-getty@ttyS0.service=$DROPIN_B64 \
  -chardev pipe,id=s0,path=/tmp/ser,logfile=/out/qemu-serial-x86_64.log   -nographic -monitor none -serial chardev:s0'

echo "Booting $IMG (timeout ${TIMEOUT}s, container $CNAME) ..."
docker run -d --name "$CNAME" -e DROPIN_B64="$DROPIN_B64" -v "$H_IMG:/image.raw:ro" -v "$H_OUT:/out" "$TAG" bash -c "$INNER" >/dev/null

clean_log() { sed -e 's/\x1b\[[0-9;?]*[A-Za-z]//g' -e 's/\r//g' "$LOG" > "$CLEAN" 2>/dev/null || : > "$CLEAN"; }
has_fail() { grep -E "$FAIL_RE" "$CLEAN" 2>/dev/null | head -5; }
has_pass() { grep -qF "$PASS_RE" "$CLEAN"; }
send() { docker exec "$CNAME" sh -c "printf '%s\r' \"\$1\" > /tmp/ser.in" sh "$1"; }
running() { [ "$(docker inspect -f '{{.State.Running}}' "$CNAME" 2>/dev/null || echo false)" = true ]; }
finish() { # code message
  echo "-------- last 40 lines of $LOG --------"
  tail -n 40 "$CLEAN" || true
  echo "--------"
  echo "$2"
  exit "$1"
}

START=$SECONDS
LOGGED_IN=0 LAST_ASK=0
while :; do
  sleep 2
  clean_log
  f="$(has_fail || true)"
  if [ -n "$f" ]; then
    finish 1 "FAIL: failure marker in serial log:
$f"
  fi
  if [ "$LOGGED_IN" = 0 ] && grep -q 'automatic login' "$CLEAN"; then
    echo "Root autologin on ttyS0 after $((SECONDS-START))s; querying systemctl ..."
    LOGGED_IN=1; LAST_ASK=0
  elif [ "$LOGGED_IN" = 1 ] && [ $((SECONDS-LAST_ASK)) -ge 8 ]; then
    # The echoed command line itself never matches PASS_RE.
    # shellcheck disable=SC2016
    send 'echo "VERIFY $(systemctl is-active multi-user.target astro-daemon.service | paste -sd,) END"'
    LAST_ASK=$SECONDS
  fi
  if has_pass; then
    echo "systemctl reports both units active after $((SECONDS-START))s; waiting 15s for late failures ..."
    sleep 15
    clean_log
    f="$(has_fail || true)"
    if [ -n "$f" ]; then
      finish 1 "FAIL: failure after boot:
$f"
    fi
    finish 0 "PASS: multi-user.target and astro-daemon.service active"
  fi
  if ! running; then
    echo "QEMU container exited early:"; docker logs --tail 20 "$CNAME" 2>&1 || true
    clean_log
    finish 1 "FAIL: QEMU exited before boot markers appeared"
  fi
  if [ $((SECONDS-START)) -ge "$TIMEOUT" ]; then
    finish 2 "TIMEOUT: units not reported active within ${TIMEOUT}s"
  fi
done
