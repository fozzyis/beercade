# x86_64 image assembler

Builds a bootable GPT disk image (ESP, ROOT_A, ROOT_B, DATA) from a payload file.

    bash shared/buildfarm/x86_64/build_image.sh [payload]   # astro (default) or boss
    make image-x86 / make image-boss-x86                     # same, via make

Outputs in `out/`: `<IMAGE_NAME>` (raw GPT disk), `<IMAGE_NAME>.zst` (for
distribution: `zstd -dc x.raw.zst | dd of=/dev/sdX bs=4M`), `<name>.vhdx`
(dynamic, for Hyper-V Generation 2 VMs; Secure Boot must be off, the images are
unsigned) and `<IMAGE_NAME>.sha256`. The initramfs carries virtio, NVMe/AHCI and
Hyper-V (`hv_vmbus`, `hv_storvsc`) storage drivers.

Needs Docker (Docker Desktop on Windows, run from Git Bash). Output goes to
`out/<IMAGE_NAME>`. The Windows `target/` dir is untouched: cargo target and
registry live in the Docker volumes `beercade-x86-cargo-target` and
`beercade-x86-cargo-registry`; the pacman cache in `beercade-x86-pacman-cache`.

Steps:
1. `rust:1-bookworm`: `cargo build --release --locked -p <CARGO_PACKAGES>`, binaries to `out/bin-<PAYLOAD_NAME>/`.
   1b. `node:22-bookworm`: each `FRONTEND_BUILDS` entry is copied (without
   `node_modules`/build output), `npm ci && npm run build`, result to `out/fe-<PAYLOAD_NAME>/<n>/`.
   npm cache in the Docker volume `beercade-x86-npm-cache`.
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
| `FRONTEND_BUILDS` | Optional. `src:build_subdir:dest` entries; built in step 1b, copied to `dest` in the rootfs |
| `IMAGE_COMPRESS` | Optional, default `zstd`; anything else skips the `.zst` |
| `ENABLE_UNITS` | Enabled with `systemctl --root` (the only way units get enabled) |
| `INITCPIO_DIR` | Contains `hooks/` and `install/`, copied to `/etc/initcpio/` |
| `MKINITCPIO_CONF` | mkinitcpio config used for the generic initramfs |
| `INITRAMFS_NAME` | Initramfs file name on the ESP |
| `ESP_DIR` | Copied to the ESP root (`loader/` etc.) |
| `DATA_DIR` | Initial contents of the DATA partition |

Non-artifact files: `/etc/machine-id` is emptied, `systemd-firstboot.service` is
masked, locale C.UTF-8 / timezone UTC, `/etc/resolv.conf` points to the resolved
stub, `/data` exists as mountpoint. `systemd-sysusers` runs after the overlays, so
an overlaid `sysusers.d` file creates its users in the image.

Payloads: `astro.conf` (A.S.T.R.O. node), `boss.conf` (B.O.S.S. server + kiosk,
platform files in `boss/platform/x86_64/`; reuses the astro initcpio hook and
mkinitcpio config, which are image-agnostic).

Verification: `bash shared/tests/verify_qemu_x86_64.sh <payload>` (`make
verify-x86`, `make verify-boss-x86`).
