#!/usr/bin/env bash
# Runs inside debian:bookworm-slim (linux/arm64, privileged). Repo at /src (ro),
# output at /out, download cache at /cache.
#
# Base: the official Arch Linux ARM "rpi-aarch64" rootfs (Pi 4 and Pi 5:
# linux-rpi, raspberrypi-bootloader, firmware), verified against its md5 and
# the Arch Linux ARM build key. Packages are added in a chroot (the container is
# already arm64, so no extra emulation). Layout matches the x86 images:
# BOOT (FAT, firmware + kernel + initramfs), ROOT_A / ROOT_B (squashfs), DATA.
set -euo pipefail
: "${PAYLOAD:=boss}"
: "${SRC:=/src}"
: "${OUT:=/out}"
: "${CACHE:=/cache}"
# shellcheck source=/dev/null
source "$SRC/shared/buildfarm/aarch64/payloads/$PAYLOAD.conf"
: "${HOSTNAME:=$PAYLOAD_NAME}"
[[ -v FRONTEND_BUILDS ]] || FRONTEND_BUILDS=()
: "${IMAGE_COMPRESS:=zstd}"
: "${ALARM_URL:=http://os.archlinuxarm.org/os/ArchLinuxARM-rpi-aarch64-latest.tar.gz}"
# "Arch Linux ARM Build System <builder@archlinuxarm.org>"
ALARM_KEY=68B3537F39A313B3E574D06777193F152BDBE6A6

WORK=/work
ROOT=$WORK/rootfs
BIN_DIR=$OUT/$BIN_SUB
FE_DIR=$OUT/$FE_SUB
BOOT_MIB=512 ROOT_MIB=${ROOT_MIB:-2048} DATA_MIB=256

banner() { printf '\n==== %s ====\n' "$*"; }
die() { echo "ERROR: $*" >&2; exit 1; }
in_root() { chroot "$ROOT" /usr/bin/env -i HOME=/root PATH=/usr/bin:/usr/sbin LANG=C.UTF-8 "$@"; }

for f in "$MKINITCPIO_CONF" "$INITCPIO_DIR" "$BOOT_DIR" "$DATA_DIR"; do
  [[ -e "$SRC/$f" ]] || die "payload path missing: $f"
done

banner "tools"
if ! (command -v mksquashfs && command -v zstd && command -v bsdtar && command -v mcopy) >/dev/null 2>&1; then
  apt-get update -qq
  apt-get install -y -qq --no-install-recommends ca-certificates curl gnupg dirmngr libarchive-tools \
    squashfs-tools dosfstools mtools e2fsprogs fdisk zstd >/dev/null
fi

banner "Arch Linux ARM rootfs"
mkdir -p "$CACHE"
TARBALL="$CACHE/$(basename "$ALARM_URL")"
# Re-download only when upstream has a newer "latest" (curl -z compares mtimes).
if [[ -f "$TARBALL" ]]; then
  curl -fsSL -R -z "$TARBALL" -o "$TARBALL" "$ALARM_URL"
else
  curl -fsSL -R -o "$TARBALL" "$ALARM_URL"
fi
curl -fsSL -o "$TARBALL.md5" "$ALARM_URL.md5"
curl -fsSL -o "$TARBALL.sig" "$ALARM_URL.sig"
(cd "$CACHE" && md5sum -c "$(basename "$TARBALL").md5") || die "rootfs md5 mismatch"
export GNUPGHOME="$WORK/gnupg"
rm -rf "$WORK"; mkdir -p "$GNUPGHOME" "$ROOT"; chmod 700 "$GNUPGHOME"
gpg --batch --quiet --keyserver hkps://keyserver.ubuntu.com --recv-keys "$ALARM_KEY" \
  || die "could not fetch the Arch Linux ARM build key"
SIG_STATUS="$(gpg --batch --status-fd 1 --verify "$TARBALL.sig" "$TARBALL" 2>/dev/null || true)"
[[ "$SIG_STATUS" == *"VALIDSIG $ALARM_KEY"* ]] || die "rootfs signature does not match $ALARM_KEY"
echo "  verified $(basename "$TARBALL")"
bsdtar -xpf "$TARBALL" -C "$ROOT"

banner "chroot setup"
mount --bind /proc "$ROOT/proc"
mount --bind /sys "$ROOT/sys"
mount --bind /dev "$ROOT/dev"
mount --bind /dev/pts "$ROOT/dev/pts"
mkdir -p "$CACHE/pacman-pkg" "$ROOT/var/cache/pacman/pkg"
mount --bind "$CACHE/pacman-pkg" "$ROOT/var/cache/pacman/pkg"
trap 'umount -l "$ROOT/var/cache/pacman/pkg" "$ROOT/dev/pts" "$ROOT/dev" "$ROOT/sys" "$ROOT/proc" 2>/dev/null || true' EXIT
rm -f "$ROOT/etc/resolv.conf"; cp /etc/resolv.conf "$ROOT/etc/resolv.conf"

banner "pacman: ${PACMAN_PACKAGES[*]}"
in_root pacman-key --init >/dev/null
in_root pacman-key --populate archlinuxarm >/dev/null
PACMAN_OPTS=(--noconfirm --needed)
# pacman 7's download sandbox needs Landlock, which emulated containers lack.
grep -q '^DisableSandbox' "$ROOT/etc/pacman.conf" 2>/dev/null || sed -i '/^\[options\]/a DisableSandbox' "$ROOT/etc/pacman.conf"
sed -i 's/^[[:space:]]*CheckSpace/#CheckSpace/' "$ROOT/etc/pacman.conf"
in_root pacman -Syu "${PACMAN_OPTS[@]}" "${PACMAN_PACKAGES[@]}"

banner "accounts"
# The stock image ships alarm/alarm and root/root; an appliance gets neither.
in_root userdel -r alarm 2>/dev/null || true
in_root passwd -l root >/dev/null

banner "overlays"
for ov in "${OVERLAYS[@]}"; do
  s="${ov%%:*}" d="${ov#*:}"
  [[ -d "$SRC/$s" ]] || die "overlay source missing: $s"
  echo "  $s -> $d"
  mkdir -p "$ROOT$d"
  # *.local holds real credentials; *.wants/ are plain files on Windows checkouts.
  tar -C "$SRC/$s" --exclude='*.local' --exclude='*.wants' -cf - . \
    | tar -C "$ROOT$d" --no-same-owner --no-overwrite-dir -xf -
done
chmod 0700 "$ROOT/root"

banner "binaries"
for p in "${CARGO_PACKAGES[@]}"; do
  [[ -f "$BIN_DIR/$p" ]] || die "missing binary $BIN_DIR/$p"
  install -Dm0755 "$BIN_DIR/$p" "$ROOT/usr/bin/$p"
done

if ((${#FRONTEND_BUILDS[@]})); then
  banner "frontends"
  for i in "${!FRONTEND_BUILDS[@]}"; do
    IFS=: read -r _ _ fe_dest <<<"${FRONTEND_BUILDS[$i]}"
    [[ -f "$FE_DIR/$i/index.html" ]] || die "missing frontend build $FE_DIR/$i"
    echo "  $FE_DIR/$i -> $fe_dest"
    mkdir -p "$ROOT$fe_dest"
    cp -r "$FE_DIR/$i/." "$ROOT$fe_dest/"
    chown -R 0:0 "$ROOT$fe_dest"
  done
fi

in_root systemd-sysusers

banner "units"
in_root systemctl enable "${ENABLE_UNITS[@]}"
in_root systemctl mask systemd-firstboot.service

banner "first boot settings"
ln -sfn /usr/share/zoneinfo/UTC "$ROOT/etc/localtime"
echo "LANG=C.UTF-8" > "$ROOT/etc/locale.conf"
[[ -s "$ROOT/etc/hostname" ]] || echo "$HOSTNAME" > "$ROOT/etc/hostname"
: > "$ROOT/etc/machine-id"
install -d -m0755 "$ROOT/data"

banner "initramfs"
KVER=
for d in "$ROOT"/usr/lib/modules/*/; do
  [[ -f "$d/pkgbase" || -f "$d/modules.dep" ]] && { KVER="$(basename "$d")"; break; }
done
[[ -n "$KVER" ]] || die "kernel modules not found"
echo "  kernel $KVER"
[[ -f "$ROOT/boot/kernel8.img" ]] || die "kernel8.img not found in /boot (is linux-rpi installed?)"
install -Dm0644 "$SRC/$INITCPIO_DIR/hooks/"* -t "$ROOT/etc/initcpio/hooks/"
install -Dm0644 "$SRC/$INITCPIO_DIR/install/"* -t "$ROOT/etc/initcpio/install/"
install -Dm0644 "$SRC/$MKINITCPIO_CONF" "$ROOT/etc/mkinitcpio-$PAYLOAD_NAME.conf"
in_root mkinitcpio -c "/etc/mkinitcpio-$PAYLOAD_NAME.conf" -k "$KVER" -g "/root/$INITRAMFS_NAME"
mv "$ROOT/root/$INITRAMFS_NAME" "$WORK/$INITRAMFS_NAME"

# Stage the BOOT partition from the firmware package's /boot, then empty it in
# the rootfs (nothing mounts /boot on the appliance).
cp -a "$ROOT/boot" "$WORK/boot-stage"
rm -f "$WORK/boot-stage"/initramfs-*.img
cp "$WORK/$INITRAMFS_NAME" "$WORK/boot-stage/"
cp "$WORK/$INITRAMFS_NAME" "$WORK/boot-stage/initramfs.img"
cp -a "$SRC/$BOOT_DIR/." "$WORK/boot-stage/"
find "$ROOT/boot" -mindepth 1 -delete

if [[ -d "$SRC/astro/rootfs/lib/modules" ]]; then
  echo "  staging Rockchip kernel modules -> /usr/lib/modules"
  cp -a "$SRC/astro/rootfs/lib/modules/." "$ROOT/usr/lib/modules/"
fi

umount -l "$ROOT/var/cache/pacman/pkg" "$ROOT/dev/pts" "$ROOT/dev" "$ROOT/sys" "$ROOT/proc"
trap - EXIT
ln -sfn /run/systemd/resolve/stub-resolv.conf "$ROOT/etc/resolv.conf"
rm -rf "$ROOT"/var/cache/pacman/pkg/* "$ROOT"/var/lib/pacman/sync/*

banner "squashfs"
mksquashfs "$ROOT" "$WORK/root.sqfs" -comp zstd -noappend -quiet
SQ=$(stat -c %s "$WORK/root.sqfs")
echo "  root.sqfs $((SQ / 1048576)) MiB"
((SQ <= ROOT_MIB * 1048576)) || die "squashfs larger than $ROOT_MIB MiB"

banner "BOOT"
export MTOOLS_SKIP_CHECK=1
BOOT=$WORK/boot.img
mkfs.vfat -F 32 -n BOOT -C "$BOOT" $((BOOT_MIB * 1024)) >/dev/null
mcopy -i "$BOOT" -s "$WORK/boot-stage/"* ::/
mdir -i "$BOOT" ::/ | grep -E 'config|cmdline|kernel8|initramfs|start4|Image|extlinux' || true

banner "DATA"
DSTAGE=$WORK/data-stage
cp -a "$SRC/$DATA_DIR" "$DSTAGE"
chown -R 0:0 "$DSTAGE"
mkfs.ext4 -q -L DATA -d "$DSTAGE" "$WORK/data.img" "${DATA_MIB}M"

banner "disk"
# GPT boots on Pi 5 and on Pi 4 with a 2020-09 or newer bootloader EEPROM.
# Radxa Rock 5C boots via Rockchip U-Boot at Sector 64 (32 KB offset in the 16 MiB gap).
IMG=$WORK/disk.raw
S=2048  # 512-byte sectors per MiB
P1=16; P2=$((P1 + BOOT_MIB)); P3=$((P2 + ROOT_MIB)); P4=$((P3 + ROOT_MIB)); END=$((P4 + DATA_MIB))
rm -f "$IMG"; truncate -s $(((END + 1) * 1048576)) "$IMG"
sfdisk -q "$IMG" <<SF
label: gpt
start=$((P1*S)), size=$((BOOT_MIB*S)), type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="BOOT"
start=$((P2*S)), size=$((ROOT_MIB*S)), type=B921B045-1DF0-41C3-AF44-4C6F280D3FAE, name="ROOT_A"
start=$((P3*S)), size=$((ROOT_MIB*S)), type=B921B045-1DF0-41C3-AF44-4C6F280D3FAE, name="ROOT_B"
start=$((P4*S)), size=$((DATA_MIB*S)), type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="DATA"
SF

UBOOT_BIN="$SRC/shared/buildfarm/aarch64/bootloader/u-boot-rockchip.bin"
if [[ -f "$UBOOT_BIN" ]]; then
  echo "  flashing Rockchip U-Boot to Sector 64..."
  dd if="$UBOOT_BIN" of="$IMG" seek=64 conv=notrunc status=none
fi

dd if="$BOOT" of="$IMG" bs=1M seek=$P1 conv=notrunc status=none
dd if="$WORK/root.sqfs" of="$IMG" bs=1M seek=$P2 conv=notrunc status=none
dd if="$WORK/root.sqfs" of="$IMG" bs=1M seek=$P3 conv=notrunc status=none
dd if="$WORK/data.img" of="$IMG" bs=1M seek=$P4 conv=notrunc status=none
sfdisk -l "$IMG"

banner "output"
cp "$IMG" "$OUT/$IMAGE_NAME"
rm -f "$OUT/$IMAGE_NAME.zst"
if [[ "$IMAGE_COMPRESS" == zstd ]]; then
  # Flash with `zstd -dc <img>.zst | dd of=/dev/sdX bs=4M`, or Raspberry Pi Imager.
  zstd -q -T0 -6  "$IMG" -o "$OUT/$IMAGE_NAME.zst"
fi
(cd "$OUT" && sha256sum "$IMAGE_NAME" "$IMAGE_NAME".zst 2>/dev/null > "$IMAGE_NAME.sha256" || true)
ls -l "$OUT/$IMAGE_NAME"*
