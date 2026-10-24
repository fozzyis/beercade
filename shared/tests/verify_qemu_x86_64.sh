#!/usr/bin/env bash
# Boot-verify an x86_64 image under QEMU + OVMF (TCG) in a Docker container.
# Usage: verify_qemu_x86_64.sh [payload] [image]
#        payload astro (default) or boss; image defaults to out/beercade-<payload>-x86_64.raw.
#        A single argument ending in .raw is taken as an astro image (old usage).
# Env:   VERIFY_TIMEOUT (seconds for the whole run), QEMU_IMAGE (default debian:bookworm)
# Exit:  0 pass, 1 fail, 2 timeout
#
# systemd stops printing unit status once a getty owns the serial console, so
# the log alone cannot prove multi-user.target. A systemd credential passed via
# SMBIOS (test VM only, the image is unchanged) adds a root autologin drop-in
# for serial-getty@ttyS0; the script then runs commands over the serial line
# and greps their output for a sentinel.
#
# The images target x86-64-v2 (SSE4.2/POPCNT); TCG's default qemu64 CPU is
# v1 and would SIGILL, so the VM gets -cpu max.
#
# A run is a list of phases, one per boot. Each phase sends CHECK until EXPECT
# appears, then optionally sends ACTION (which may reboot) and waits for
# ACTION_EXPECT. The next phase starts at the next root autologin. The OVMF
# vars file lives for the whole run, so bootctl set-default survives reboots.
# The VM disk is a qcow2 overlay on the read-only image, DISK_GB large, so the
# first-boot DATA growth (beercade-grow-data.service) has room and is checked
# (DATA must reach 5G; astro leaves ~7G of 16G after its two 4G root slots).
set -euo pipefail
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL='*'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

PAYLOAD="${1:-astro}"
if [[ "$PAYLOAD" == *.raw ]]; then set -- astro "$PAYLOAD"; PAYLOAD=astro; fi
IMG="${2:-out/beercade-$PAYLOAD-x86_64.raw}"
BASE_IMAGE="${QEMU_IMAGE:-debian:bookworm}"
TAG="beercade-qemu-x86-2"
OUT_DIR="$REPO_ROOT/out"
LOG="$OUT_DIR/qemu-serial-x86_64-$PAYLOAD.log"
CLEAN="$OUT_DIR/qemu-serial-x86_64-$PAYLOAD.clean.log"
CNAME="beercade-verify-x86-$$-$(date +%s)"
DISK_GB=16
NONCE="persist-$$-$RANDOM$RANDOM"

FAIL_RE='Kernel panic|^ERROR: |dropped into a recovery shell|You are now being dropped into an emergency shell'
# Sentinels are built with $(...) so the echoed command line never matches.
CHECK=() EXPECT=() ACTION=() ACTION_EXPECT=()
case "$PAYLOAD" in
astro)
  TIMEOUT="${VERIFY_TIMEOUT:-480}" MEM=2048
  FAIL_RE="$FAIL_RE|Failed to start astro-daemon|astro-daemon\.service: Failed|astro-daemon\.service: Main process exited"
  CHECK[0]='echo "VERIFY $(systemctl is-active multi-user.target astro-tune.service astro-daemon.service astro-frontend.service | paste -sd,) es=$(pgrep -cx es-de) restarts=$(systemctl show -p NRestarts --value astro-frontend.service) data=$(df -BG --output=size /data | awk "NR==2{print ($1+0>=5)?\"grown\":\$1}") ra=$(retroarch --version >/dev/null 2>&1 && echo ok) cores=$(ls /usr/lib/libretro/*_libretro.so 2>/dev/null | wc -l | awk "{print (\$1>=10)?\"ok\":\$1}") END"'
  EXPECT[0]='VERIFY active,active,active,active es=1 restarts=0 data=grown ra=ok cores=ok END'
  ;;
boss)
  TIMEOUT="${VERIFY_TIMEOUT:-1200}" MEM=3072
  FAIL_RE="$FAIL_RE|Failed to start boss|boss-server\.service: Main process exited"
  boss_check='echo "VERIFY$(echo @N@) $(systemctl is-active multi-user.target seatd.service boss-server.service boss-kiosk.service | paste -sd,) http=$(curl -fsS -m5 http://127.0.0.1:8080/api/health) ui=$(curl -sS -m5 -o /dev/null -w %{http_code} http://127.0.0.1:8080/) cage=$(pgrep -cx cage) restarts=$(systemctl show -p NRestarts --value boss-kiosk.service) $(grep -o "root=PARTLABEL=[A-Z_]*" /proc/cmdline) data=$(df -BG --output=size /data | awk "NR==2{print ($1+0>=5)?\"grown\":\$1}") persist=$(cat /data/verify-persist 2>/dev/null || echo none) END"'
  # Boot 1 (slot A): services, HTTP, kiosk; then write /data, switch to B, reboot.
  CHECK[0]="${boss_check//@N@/1}"
  EXPECT[0]='VERIFY1 active,active,active,active http=ok ui=200 cage=1 restarts=0 root=PARTLABEL=ROOT_A data=grown persist=none END'
  ACTION[0]="echo $NONCE > /data/verify-persist && sync && bootctl set-default beercade-slot-b.conf && echo \"SWITCH \$(tail -c +5 /sys/firmware/efi/efivars/LoaderEntryDefault-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f | tr -d '\\0') END\" && systemctl reboot"
  ACTION_EXPECT[0]='SWITCH beercade-slot-b.conf END'
  # Boot 2 (slot B): same checks, plus the /data file from boot 1.
  CHECK[1]="${boss_check//@N@/2}"
  EXPECT[1]="VERIFY2 active,active,active,active http=ok ui=200 cage=1 restarts=0 root=PARTLABEL=ROOT_B data=grown persist=$NONCE END"
  ;;
*) echo "FAIL: unknown payload $PAYLOAD" >&2; exit 1 ;;
esac

# Host paths for docker bind mounts (pwd -W gives D:/... under Git Bash).
hostpath() {
  local d; d="$(cd "$(dirname "$1")" && { pwd -W 2>/dev/null || pwd; })"
  printf '%s/%s' "$d" "$(basename "$1")"
}

if [ ! -f "$IMG" ]; then
  echo "FAIL: image not found: $IMG (build it with build_image.sh $PAYLOAD)" >&2
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
  printf 'FROM %s\nRUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends qemu-system-x86 qemu-utils ovmf ca-certificates && rm -rf /var/lib/apt/lists/*\n' "$BASE_IMAGE" \
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
qemu-img create -q -f qcow2 -b /image.raw -F raw /tmp/disk.qcow2 ${DISK_GB}G
mkfifo /tmp/ser.in /tmp/ser.out
cat /tmp/ser.out > /dev/null &
exec qemu-system-x86_64 -machine q35 -cpu max -m $MEM -smp 2 -accel tcg \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file=/tmp/vars.fd \
  -drive file=/tmp/disk.qcow2,if=virtio,format=qcow2 \
  -nic user,model=virtio-net-pci \
  -smbios type=11,value=io.systemd.credential.binary:systemd.unit-dropin.serial-getty@ttyS0.service=$DROPIN_B64 \
  -chardev pipe,id=s0,path=/tmp/ser,logfile=/out/$LOGNAME \
  -vga std -display none -monitor none -serial chardev:s0'

echo "Booting $IMG (timeout ${TIMEOUT}s, container $CNAME) ..."
docker run -d --name "$CNAME" -e DROPIN_B64="$DROPIN_B64" -e MEM="$MEM" -e DISK_GB="$DISK_GB" -e LOGNAME="$(basename "$LOG")" -v "$H_IMG:/image.raw:ro" -v "$H_OUT:/out" "$TAG" bash -c "$INNER" >/dev/null

clean_log() { sed -e 's/\x1b\[[0-9;?]*[A-Za-z]//g' -e 's/\r//g' "$LOG" > "$CLEAN" 2>/dev/null || : > "$CLEAN"; }
has_fail() { grep -E "$FAIL_RE" "$CLEAN" 2>/dev/null | head -5; }
send() { docker exec "$CNAME" sh -c "printf '%s\r' \"\$1\" > /tmp/ser.in" sh "$1"; }
running() { [ "$(docker inspect -f '{{.State.Running}}' "$CNAME" 2>/dev/null || echo false)" = true ]; }
finish() { # code message
  echo "-------- last 40 lines of $LOG --------"
  tail -n 40 "$CLEAN" || true
  echo "--------"
  echo "$2"
  exit "$1"
}

logins() { grep -c 'automatic login' "$CLEAN" 2>/dev/null || true; }

START=$SECONDS
PHASE=0 STAGE=wait LAST_ASK=0
while :; do
  sleep 2
  clean_log
  f="$(has_fail || true)"
  if [ -n "$f" ]; then
    finish 1 "FAIL: failure marker in serial log:
$f"
  fi
  case $STAGE in
  wait)
    if [ "$(logins)" -gt "$PHASE" ]; then
      echo "boot $((PHASE+1)): root autologin on ttyS0 after $((SECONDS-START))s; checking ..."
      STAGE=check LAST_ASK=0
    fi ;;
  check)
    if grep -qF "${EXPECT[$PHASE]}" "$CLEAN"; then
      echo "boot $((PHASE+1)): checks passed after $((SECONDS-START))s: ${EXPECT[$PHASE]}"
      if [ -n "${ACTION[$PHASE]:-}" ]; then
        send "${ACTION[$PHASE]}"; STAGE=action LAST_ASK=$SECONDS
      elif [ $((PHASE+1)) -lt "${#CHECK[@]}" ]; then
        finish 1 "FAIL: phase $((PHASE+1)) has no action to reach the next boot"
      else
        echo "waiting 15s for late failures ..."
        sleep 15
        clean_log
        f="$(has_fail || true)"
        if [ -n "$f" ]; then
          finish 1 "FAIL: failure after boot:
$f"
        fi
        finish 0 "PASS: $PAYLOAD, ${#CHECK[@]} boot(s)"
      fi
    elif [ $((SECONDS-LAST_ASK)) -ge 8 ]; then
      send "${CHECK[$PHASE]}"; LAST_ASK=$SECONDS
    fi ;;
  action)
    if grep -qF "${ACTION_EXPECT[$PHASE]}" "$CLEAN"; then
      echo "boot $((PHASE+1)): action done (${ACTION_EXPECT[$PHASE]}); rebooting"
      PHASE=$((PHASE+1)) STAGE=wait
    elif [ $((SECONDS-LAST_ASK)) -ge 60 ]; then
      finish 1 "FAIL: boot $((PHASE+1)) action did not report ${ACTION_EXPECT[$PHASE]}"
    fi ;;
  esac
  if ! running; then
    echo "QEMU container exited early:"; docker logs --tail 20 "$CNAME" 2>&1 || true
    clean_log
    finish 1 "FAIL: QEMU exited before boot markers appeared"
  fi
  if [ $((SECONDS-START)) -ge "$TIMEOUT" ]; then
    stage_expect="${EXPECT[$PHASE]}"; [ "$STAGE" = action ] && stage_expect="${ACTION_EXPECT[$PHASE]}"
    echo "last check output:"; grep -E '^(VERIFY|SWITCH)' "$CLEAN" | tail -3 || true
    finish 2 "TIMEOUT: boot $((PHASE+1)) ($STAGE) did not reach '$stage_expect' within ${TIMEOUT}s"
  fi
done
