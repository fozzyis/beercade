# x86_64 image assembler

Builds a bootable GPT disk image (ESP, ROOT_A, ROOT_B, DATA) from a payload file.

    bash shared/buildfarm/x86_64/build_image.sh [payload]   # default: astro

Needs Docker (Docker Desktop on Windows, run from Git Bash). Output goes to
`out/<IMAGE_NAME>`. The Windows `target/` dir is untouched: cargo target and
registry live in the Docker volumes `beercade-x86-cargo-target` and
`beercade-x86-cargo-registry`; the pacman cache in `beercade-x86-pacman-cache`.

Steps:
1. `rust:1-bookworm`: `cargo build --release --locked -p <CARGO_PACKAGES>`, binaries to `out/bin-<PAYLOAD_NAME>/`.
2. `archlinux:latest --privileged` runs `container_build.sh`: pacstrap, overlays,
   binaries, unit enablement, mkinitcpio, squashfs, ESP (mtools), DATA (mkfs.ext4 -d),
   then the GPT disk is stitched with sfdisk + dd (no loop devices).

Layout (1 MiB aligned): ESP 512 MiB FAT32, ROOT_A / ROOT_B 2 GiB squashfs (same
image), DATA 256 MiB ext4. The ESP gets systemd-boot as `EFI/BOOT/BOOTX64.EFI` and
`EFI/systemd/systemd-bootx64.efi`, `ESP_DIR` contents at its root, and
`beercade/slot-{a,b}/{vmlinuz-linux,<INITRAMFS_NAME>}`.

## Payload variables (`payloads/<name>.conf`, bash-sourced, paths relative to repo root)

| Variable | Meaning |
|---|---|
| `PAYLOAD_NAME` | Short name; used for `bin-<name>` and `/etc/mkinitcpio-<name>.conf` |
| `IMAGE_NAME` | Output file in `out/` |
| `HOSTNAME` | Passed to systemd-firstboot (an overlaid `/etc/hostname` wins); default `PAYLOAD_NAME` |
| `CARGO_PACKAGES` | Workspace packages; binary of the same name goes to `/usr/bin` |
| `PACMAN_PACKAGES` | pacstrap package list |
| `OVERLAYS` | `src:dest` dirs copied in order. `*.local` files and `*.wants/` dirs are skipped |
| `ENABLE_UNITS` | Enabled with `systemctl --root` (the only way units get enabled) |
| `INITCPIO_DIR` | Contains `hooks/` and `install/`, copied to `/etc/initcpio/` |
| `MKINITCPIO_CONF` | mkinitcpio config used for the generic initramfs |
| `INITRAMFS_NAME` | Initramfs file name on the ESP |
| `ESP_DIR` | Copied to the ESP root (`loader/` etc.) |
| `DATA_DIR` | Initial contents of the DATA partition |

Non-artifact files: `/etc/machine-id` is emptied, `systemd-firstboot.service` is
masked, locale C.UTF-8 / timezone UTC, `/etc/resolv.conf` points to the resolved
stub, `/data` exists as mountpoint. Building a new image (e.g. B.O.S.S., M2b) means
adding `payloads/boss.conf`; a non-cargo build step (kiosk UI) would be a new
optional hook in `build_image.sh`.
