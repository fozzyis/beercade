# Raspberry Pi (aarch64) image assembler

Builds a bootable GPT image for the B.O.S.S. appliance on a Raspberry Pi 4 or 5,
with the same layout and boot path as the x86 images (squashfs A/B root,
`astro_overlay` initramfs hook, DATA grown on first boot).

    bash shared/buildfarm/aarch64/build_image.sh boss   # or: make image-boss-rpi

Needs Docker with arm64 emulation (Docker Desktop has it; on Linux install
`qemu-user-static`/binfmt). Everything except the frontend build runs as
`linux/arm64`, so the first cargo build is slow (tens of minutes); the cargo
target, registry and the downloaded rootfs live in the Docker volumes
`beercade-arm-cargo-target`, `beercade-arm-cargo-registry` and
`beercade-arm-alarm-cache`.

Outputs in `out/`: `beercade-boss-rpi-aarch64.raw`, `.raw.zst` (flash with
`zstd -dc x.raw.zst | dd of=/dev/sdX bs=4M`, or Raspberry Pi Imager) and
`.sha256`.

Steps:
1. `rust:1-bookworm` (arm64): `cargo build --release --locked -p <CARGO_PACKAGES>`.
   The workspace `.cargo/config.toml` sets the aarch64 CPU floor (Cortex-A72,
   without the crypto extensions the Pi 4 lacks).
   1b. `node:22-bookworm`: frontend builds, as for x86.
2. `debian:bookworm-slim` (arm64, privileged) runs `container_build.sh`:
   - downloads the official Arch Linux ARM `rpi-aarch64` rootfs (`ALARM_URL`
     overrides) and checks its md5 and its signature against the Arch Linux ARM
     build key `68B3537F39A313B3E574D06777193F152BDBE6A6`;
   - installs `PACMAN_PACKAGES` in a chroot, deletes the stock `alarm` user and
     locks root;
   - overlays, binaries, frontends, sysusers, unit enablement;
   - builds the initramfs from `MKINITCPIO_CONF`, stages `/boot` (firmware,
     `kernel8.img`, device trees) plus `BOOT_DIR` (`config.txt`, `cmdline.txt`)
     into the BOOT partition, then squashfs, DATA and the GPT disk.

Layout (1 MiB aligned): BOOT 512 MiB FAT32, ROOT_A / ROOT_B 2 GiB squashfs (same
image), DATA 256 MiB ext4. GPT boots on Pi 5, and on Pi 4 with a bootloader
EEPROM from 2020-09 or later (`rpi-eeprom-update`).

Payload variables are the x86 ones (see `../x86_64/README.md`) with `BOOT_DIR`
in place of `ESP_DIR`. `boss/platform/common/` is shared with the x86 image;
`boss/platform/aarch64/` adds the Pi boot files, initramfs config and a kiosk
drop-in that enables Chromium GPU raster on the VideoCore GPU.

Not done yet:
- **Slot B is never booted.** The Pi firmware has no boot menu; `cmdline.txt`
  always names `ROOT_A`. Switching slots means rewriting `cmdline.txt` (or
  moving to `autoboot.txt` + `tryboot`), which the OTA flow does not do yet.
- **Not yet run end to end**: no build or boot on real Pi 4/5 hardware so far.
