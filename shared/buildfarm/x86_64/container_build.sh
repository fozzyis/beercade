#!/usr/bin/env bash
# Runs inside archlinux:latest (privileged). Repo at /src (ro), output at /out.
# Partitions are built as files and stitched into a GPT disk; no loop devices.
set -euo pipefail
: "${PAYLOAD:=astro}"
SRC=/src
OUT=/out
# shellcheck source=/dev/null
source "$SRC/shared/buildfarm/x86_64/payloads/$PAYLOAD.conf"
: "${HOSTNAME:=$PAYLOAD_NAME}"
[[ -v FRONTEND_BUILDS ]] || FRONTEND_BUILDS=()
[[ -v NATIVE_BUILDS ]] || NATIVE_BUILDS=()
: "${IMAGE_COMPRESS:=zstd}"

WORK=/work
ROOT=$WORK/rootfs
BIN_DIR=$OUT/bin-$PAYLOAD_NAME
FE_DIR=$OUT/fe-$PAYLOAD_NAME
ESP_MIB=512 ROOT_MIB=${ROOT_MIB:-2048} DATA_MIB=256

banner() { printf '\n==== %s ====\n' "$*"; }
die() { echo "ERROR: $*" >&2; exit 1; }

for f in "$MKINITCPIO_CONF" "$INITCPIO_DIR" "$ESP_DIR" "$DATA_DIR"; do
  [[ -e "$SRC/$f" ]] || die "payload path missing: $f"
done

banner "tools"
pacman -Syu --noconfirm --needed arch-install-scripts squashfs-tools dosfstools mtools e2fsprogs util-linux zstd qemu-img >/dev/null

banner "pacstrap: ${PACMAN_PACKAGES[*]}"
rm -rf "$WORK"; mkdir -p "$ROOT"
pacstrap -c -K "$ROOT" "${PACMAN_PACKAGES[@]}"

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
    chown -R root:root "$ROOT$fe_dest"
  done
fi

for i in "${!NATIVE_BUILDS[@]}"; do
  d="$OUT/native-$PAYLOAD_NAME/$i"
  [[ -d "$d/usr" ]] || die "missing native build output $d"
  banner "native build ${NATIVE_BUILDS[$i]}"
  tar -C "$d" -cf - . | tar -C "$ROOT" --no-same-owner --no-overwrite-dir -xf -
done

# Users/groups from overlaid sysusers.d (pacstrap ran sysusers before the overlays).
systemd-sysusers --root="$ROOT"

banner "units"
systemctl --root="$ROOT" enable "${ENABLE_UNITS[@]}"
systemctl --root="$ROOT" mask systemd-firstboot.service

banner "first boot settings"
systemd-firstboot --root="$ROOT" --locale=C.UTF-8 --timezone=UTC --hostname="$HOSTNAME"
: > "$ROOT/etc/machine-id"

if [[ -n "${DEV_SSH_KEY:-}" ]]; then
  banner "dev SSH key for root"
  install -d -m0700 "$ROOT/root/.ssh"
  printf '%s\n' "$DEV_SSH_KEY" > "$ROOT/root/.ssh/authorized_keys"
  chmod 0600 "$ROOT/root/.ssh/authorized_keys"
  install -d -m0755 "$ROOT/etc/ssh/sshd_config.d"
  printf '%s\n' "PermitRootLogin prohibit-password" "PasswordAuthentication no" \
    > "$ROOT/etc/ssh/sshd_config.d/10-beercade-dev.conf"
fi
install -d -m0755 "$ROOT/data"
install -d -m0700 "$ROOT/efi"

banner "initramfs"
KVER=
for d in "$ROOT"/usr/lib/modules/*/; do
  [[ -f "$d/vmlinuz" ]] && { KVER="$(basename "$d")"; break; }
done
[[ -n "$KVER" ]] || die "kernel not found"
echo "  kernel $KVER"
install -Dm0644 "$SRC/$INITCPIO_DIR/hooks/"* -t "$ROOT/etc/initcpio/hooks/"
install -Dm0644 "$SRC/$INITCPIO_DIR/install/"* -t "$ROOT/etc/initcpio/install/"
install -Dm0644 "$SRC/$MKINITCPIO_CONF" "$ROOT/etc/mkinitcpio-$PAYLOAD_NAME.conf"
arch-chroot "$ROOT" mkinitcpio -c "/etc/mkinitcpio-$PAYLOAD_NAME.conf" -k "$KVER" -g "/root/$INITRAMFS_NAME"
mv "$ROOT/root/$INITRAMFS_NAME" "$WORK/$INITRAMFS_NAME"
cp "$ROOT/usr/lib/modules/$KVER/vmlinuz" "$WORK/vmlinuz-linux"
# The rootfs never boots its own /boot; drop the autodetect images from the linux package.
rm -f "$ROOT"/boot/initramfs-*.img

# After arch-chroot (it bind-mounts its own resolv.conf).
ln -sfn /run/systemd/resolve/stub-resolv.conf "$ROOT/etc/resolv.conf"
rm -rf "$ROOT"/var/cache/pacman/pkg/* "$ROOT"/var/lib/pacman/sync/*

banner "squashfs"
mksquashfs "$ROOT" "$WORK/root.sqfs" -comp zstd -noappend -quiet
SQ=$(stat -c %s "$WORK/root.sqfs")
echo "  root.sqfs $((SQ / 1048576)) MiB"
((SQ <= ROOT_MIB * 1048576)) || die "squashfs larger than $ROOT_MIB MiB"

banner "ESP"
export MTOOLS_SKIP_CHECK=1
ESP=$WORK/esp.img
mkfs.vfat -F 32 -n ESP -C "$ESP" $((ESP_MIB * 1024)) >/dev/null
EFI="$ROOT/usr/lib/systemd/boot/efi/systemd-bootx64.efi"
[[ -f "$EFI" ]] || die "systemd-boot not found in rootfs"
mmd -i "$ESP" ::/EFI ::/EFI/BOOT ::/EFI/systemd ::/beercade ::/beercade/slot-a ::/beercade/slot-b
mcopy -i "$ESP" "$EFI" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$ESP" "$EFI" ::/EFI/systemd/systemd-bootx64.efi
mcopy -i "$ESP" -s "$SRC/$ESP_DIR/"* ::/
for s in slot-a slot-b; do
  mcopy -i "$ESP" "$WORK/vmlinuz-linux" "$WORK/$INITRAMFS_NAME" "::/beercade/$s/"
done
mdir -i "$ESP" -/ ::/ | grep -E '^ |Directory' || true

banner "DATA"
DSTAGE=$WORK/data-stage
cp -a "$SRC/$DATA_DIR" "$DSTAGE"
chown -R root:root "$DSTAGE"
mkfs.ext4 -q -L DATA -d "$DSTAGE" "$WORK/data.img" "${DATA_MIB}M"

banner "disk"
IMG=$WORK/disk.raw
S=2048  # 512-byte sectors per MiB
P1=1; P2=$((P1 + ESP_MIB)); P3=$((P2 + ROOT_MIB)); P4=$((P3 + ROOT_MIB)); END=$((P4 + DATA_MIB))
rm -f "$IMG"; truncate -s $(((END + 1) * 1048576)) "$IMG"
sfdisk -q "$IMG" <<SF
label: gpt
start=$((P1*S)), size=$((ESP_MIB*S)), type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="ESP"
start=$((P2*S)), size=$((ROOT_MIB*S)), type=4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709, name="ROOT_A"
start=$((P3*S)), size=$((ROOT_MIB*S)), type=4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709, name="ROOT_B"
start=$((P4*S)), size=$((DATA_MIB*S)), type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="DATA"
SF
dd if="$ESP" of="$IMG" bs=1M seek=$P1 conv=notrunc status=none
dd if="$WORK/root.sqfs" of="$IMG" bs=1M seek=$P2 conv=notrunc status=none
dd if="$WORK/root.sqfs" of="$IMG" bs=1M seek=$P3 conv=notrunc status=none
dd if="$WORK/data.img" of="$IMG" bs=1M seek=$P4 conv=notrunc status=none
sfdisk -d "$IMG"
sfdisk -l "$IMG"

banner "output"
cp "$IMG" "$OUT/$IMAGE_NAME"
rm -f "$OUT/$IMAGE_NAME.zst" "$OUT/${IMAGE_NAME%.raw}.vhdx"
if [[ "$IMAGE_COMPRESS" == zstd ]]; then
  # Mostly-empty partitions compress to almost nothing; flash with
  # `zstd -dc <img>.zst | dd of=/dev/sdX bs=4M`.
  zstd -q -T2 -9 "$IMG" -o "$OUT/$IMAGE_NAME.zst"
fi
# Hyper-V Gen2 (Secure Boot off; the images are unsigned).
qemu-img convert -f raw -O vhdx -o subformat=dynamic "$IMG" "$OUT/${IMAGE_NAME%.raw}.vhdx"
(cd "$OUT" && sha256sum "$IMAGE_NAME" "$IMAGE_NAME".zst "${IMAGE_NAME%.raw}.vhdx" 2>/dev/null > "$IMAGE_NAME.sha256" || true)
ls -l "$OUT/$IMAGE_NAME"* "$OUT/${IMAGE_NAME%.raw}.vhdx"
