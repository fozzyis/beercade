# x86_64 Roadmap — B.O.S.S. and A.S.T.R.O. on PC hardware

## Deliverables

1. **B.O.S.S. x86_64 image** — bootable Arch Linux image for the venue
   terminal and server backend: `boss-server` (Axum, :8080), touchscreen kiosk
   UI, admin portal, netplay relay, OTA distribution.
2. **A.S.T.R.O. x86_64 image** — bootable Arch Linux arcade runtime for PC
   hardware (not O.R.I.O.N.), tuned for PC GPUs: AMD and NVIDIA first, Intel
   integrated graphics supported.

The website (Cloudflare Pages) is out of scope here; CI only type-checks it.

## Decisions

- **One branch** (`main`) for ARM and x86_64. No fork, no sync bot. Platform
  differences live in `beercade-hal` (runtime detection) and
  `astro/platform/<arch>/`. `x86_64_fork_and_patch_sync_plan.md` is superseded;
  its snippets are reference only and several are known to be broken.
- **A/B updates on x86_64**: GPT partitions named `ESP`, `ROOT_A`, `ROOT_B`,
  `DATA`; squashfs root with tmpfs overlay; systemd-boot with entries
  `beercade-slot-a.conf` / `beercade-slot-b.conf`; slot switch via
  `bootctl set-default`. These names are hard-wired in `beercade-hal`.
- **Persistent state lives on `DATA`** (`/data`) so it survives OTA updates.
- **Rejected kernel options**: `mitigations=off` (networked venue box handling
  tokens and payments), `elevator=` (removed in Linux 5.0). `idle=poll` and
  `no_turbo` only with measured evidence.
- **NVIDIA** uses the `nvidia-open` driver with `nvidia_drm.modeset=1`
  (GTX 16xx / RTX and newer). nouveau is not a supported arcade target.
  AMD and Intel use Mesa (RADV/radeonsi, ANV/iris).

## Milestones

| # | Milestone | Status | Model |
|---|---|---|---|
| M0 | Hardware abstraction (`beercade-hal`) | Done | Opus |
| M1 | CI: build + test on every push | Done (`x86_64` & `arm64`) | Sonnet |
| M2 | A.S.T.R.O. x86_64 bootable image | Done | Opus + Sonnet |
| M2b | B.O.S.S. x86_64 bootable image | Done | Opus |
| M3 | A.S.T.R.O. PC runtime: GPUs, emulators, latency | Built (`astro-shell` + `astro_rt` wired); real-hardware frame-time gate open | Opus + Sonnet |
| M4 | Path ownership rules and lint | Done (`CODEOWNERS` + CI `governance` job) | Haiku |
| M5 | End-to-end acceptance: A.S.T.R.O. → B.O.S.S. → cloud ingest | Harness & runbook ready; 24h mixed-arch venue hardware soak pending | Sonnet |

### M0 — Hardware abstraction (done)
`shared/crates/beercade-hal`: platform detection (RK3588, Pi 5, Pi 4, generic
ARM, x86_64), thermals, A/B slot devices, active-slot detection, bootloader
commit, realtime CPU selection. The daemon's OTA, telemetry and thermal
watchdog go through it. ARM behaviour unchanged.

Resolved: failed thermal reads are modeled as `Option<f32>` (`cpu_temp_c` /
`gpu_temp_c` in `shared/crates/beercade-shared/src/models.rs`, propagated via
`typeshare` to `shared/proto/telemetry.ts`) with NaN guards in `astro-daemon`.

### M1 — CI (done: `x86_64` & `arm64`)
`.github/workflows/ci.yml`. Runners: GitHub-hosted `ubuntu-24.04` (`x86_64`)
and `ubuntu-24.04-arm` (`arm64`) on the public GitHub repo (migrated from Forgejo). Runs `RUSTFLAGS=-D warnings`
`cargo check --workspace --locked`, `cargo test --workspace --locked`,
`gen_ts_proto --check`, `astro/rt` CMake + Ninja + CTest, release builds,
Node 22 frontend type checks (`website`, `boss/kiosk_ui`), and the M4
`governance` job (`scripts/lint_governance.py` + payload/VHDX static checks).

The earlier `arm64` netplay rollback panic (`netplay.rs`, "slot within ring")
was fixed in `1f14198` / `f7617b3` (inputs below the resync frame ignored, gaps
escalate to hard resync, deterministic regression tests added) and the `arm64`
CI matrix job was re-enabled in `c4958e7`.

### M2 — A.S.T.R.O. x86_64 image (done)
`make image-x86` → `out/beercade-astro-x86_64.raw` (4.75 GiB raw; squashfs
root ~283 MiB). `make verify-x86` boots it under QEMU/OVMF (TCG) and passes.
- Assembler: `shared/buildfarm/x86_64/` (`build_image.sh` on the host,
  `container_build.sh` in `archlinux:latest`). Payload-driven:
  `payloads/astro.conf` lists cargo packages, pacman packages, overlays, units
  to enable, initcpio/ESP/DATA sources. M2b adds `payloads/boss.conf`. Docker
  volumes cache cargo and pacman; partitions are built as files and `dd`'d in.
- Boot platform: `astro/platform/x86_64/` — `astro_overlay` initcpio hook
  (squashfs at `/run/astro/lower`, tmpfs upper at `/run/astro/rw`),
  `mkinitcpio.conf` (no autodetect), systemd-boot loader + slot entries,
  `fstab`, `astro-daemon.service`, initial `DATA` contents.
- Overlays skip `*.local` (real credentials) and `*.wants/` (committed from
  Windows as plain files, not symlinks); units are enabled from the payload.
- Gate: `shared/tests/verify_qemu_x86_64.sh`. systemd stops printing status
  once a getty owns the serial console, so the script passes a test-only root
  autologin drop-in for `serial-getty@ttyS0` as an SMBIOS systemd credential
  and checks `systemctl is-active multi-user.target astro-daemon.service`.

Notes for later milestones:
- Root's password is locked; `astro-frontend.service` claims `tty1` (conflicting
  with `getty@tty1.service`) to run `astro-shell` on KMS. systemd accepts
  credentials from SMBIOS, so anything controlling firmware tables can inject
  units — fine for a kiosk test VM, revisit for production hardening.
- Not CI-gated yet: the build needs `--privileged` and ~5 GB, and TCG boot
  takes ~4–5 min. Candidate for a manual/nightly job on the laptop runner.
- Image outputs: `.raw`, `.raw.zst`, `.vhdx`, and `.raw.sha256`.

### M2b — B.O.S.S. x86_64 image (done)
`make image-boss-x86` → `out/beercade-boss-x86_64.raw` (4.75 GiB raw, squashfs
root ~590 MiB, mostly Chromium), `.raw.zst` (~1.24 GiB), `.vhdx` (~1.4 GiB),
`.raw.sha256`. `make verify-boss-x86` passes (two TCG boots, ~7.5 min).
- Payload `payloads/boss.conf`; platform files in `boss/platform/x86_64/`
  (units, sysusers/tmpfiles, static network, fstab, loader entries, DATA seed).
  Reuses the astro initcpio hook and mkinitcpio config (image-agnostic).
- Assembler additions (astro unaffected): `FRONTEND_BUILDS` (node:22 build of
  `boss/kiosk_ui` → `/usr/share/boss/kiosk_ui`), `systemd-sysusers` after the
  overlays, `.zst` + `.vhdx` + sha256 for every payload.
- `boss-server.service`: user `boss`, `BOSS_PORT=8080` (the code default stays
  8989 for ARM/Docker; nodes and the website expect `boss.lan:8080`), tokens
  and overrides from `EnvironmentFile=-/data/beercade/boss.env` (example in
  DATA), state in `/data/beercade/sync`, `ProtectSystem=strict`.
- `boss-kiosk.service`: user `kiosk` (groups seat/video/render/input), seatd,
  `cage -s -- chromium --kiosk --ozone-platform=wayland`, waits for
  `/api/health`. `WLR_RENDERER_ALLOW_SOFTWARE=1` for GPU-less VMs. Takes tty1
  (conflicts with `getty@tty1`).
- Network: static `192.168.100.10/24`, gateway/DNS `192.168.100.1`, mDNS on.
- Gate: `verify_qemu_x86_64.sh boss`. Boot 1 (slot A): multi-user, seatd,
  boss-server, boss-kiosk active, `/api/health` = ok, `/` = 200, one cage
  process, zero kiosk restarts; writes a nonce to `/data`, `bootctl
  set-default beercade-slot-b.conf` (checked via the `LoaderEntryDefault` EFI
  var), reboots. Boot 2: same checks, `root=PARTLABEL=ROOT_B`, nonce present.
  OVMF vars and the snapshot disk live for the whole QEMU process.
- Hyper-V: the initramfs includes `hv_vmbus hv_storvsc hv_netvsc hyperv_keyboard`,
  and both x86 payloads include `hyperv` (`hv_kvp_daemon.service`).
  Hyper-V Generation 2 VMs require Secure Boot off (the images are unsigned):
  `New-VM -Generation 2 -VHDPath out\<image>.vhdx`, `Set-VMFirmware
  -EnableSecureBoot Off`. Automated verification lives in
  `shared/tests/verify_hyperv_x86_64.ps1` (`make verify-hyperv-x86` /
  `make verify-hyperv-boss-x86`), and `shared/tests/verify_qemu_x86_64.sh`
  also accepts `.vhdx` images directly.

Notes for later milestones:
- M5: B.O.S.S. answers on :8080 at `192.168.100.10`; `boss.lan` needs DNS on the
  venue router (avahi publishes `boss.local`). The x86 A.S.T.R.O. image uses
  `DHCP=yes` (`astro/rootfs/etc/systemd/network/10-wired.network`), and
  `astro-daemon` defaults `BOSS_ENDPOINT` to `http://boss:8080` so multiple
  cabinets do not clash on one LAN.
- `beercade-hal` A/B slot switching, boot counting, and health-gated boot
  blessing are implemented (`systemd-bless-boot` on x86_64, `tryboot.txt` on
  RPi, U-Boot `bootcount`/`altbootcmd` on RK3588, wired via
  `beercade-boot-check.sh`).
- `boss.conf` includes split NIC/GPU `linux-firmware-*` packages plus `mesa` /
  `vulkan-radeon` / `vulkan-intel` / `vulkan-icd-loader` for hardware-accelerated
  kiosk rendering on bare-metal PCs alongside the Hyper-V `hyperv_drm` guard.

### M3 — A.S.T.R.O. PC runtime (built; hardware gate open)
`make image-x86` → `out/beercade-astro-x86_64.raw` (+ `.raw.zst` and `.vhdx`;
root slots are 4 GiB each). `shared/tests/verify_qemu_x86_64.sh astro` verifies
`astro-tune`, `astro-daemon`, `astro-frontend` (resident `astro-shell`), DATA
growth on a 16 GB disk, `hv_vmbus`, RetroArch, and ≥10 libretro cores.
- **RetroArch**: upstream v1.22.2 built via `astro/emulators/retroarch/build_x86_64.sh`
  (`-march=x86-64-v3`, KMS/plain DRM/EGL/Vulkan, udev, ALSA, SDL2; no X11/Wayland/Pulse)
  with `0001-*.patch` (`drm_gfx.c` hook) and `0002-astro-rt-vulkan-udev-input.patch`
  (calls `astro_input_start()` and wires the `/dev/shm/astro_rt_stats` +
  `/run/astro/menu.sock` pause bridge for Vulkan `khr_display` + udev).
- **Config**: `astro/platform/x86_64/rootfs/root/.config/retroarch/retroarch.cfg`
  uses Vulkan `khr_display`, ALSA default device, udev input, and locks out
  menu toggles / `Escape` so exits go exclusively through the daemon pause menu.
- **GPUs**: `mesa vulkan-radeon vulkan-intel nvidia-open nvidia-utils` plus
  split `linux-firmware-{amdgpu,intel,radeon,realtek,mediatek,atheros,broadcom,marvell,other}`;
  nouveau blacklisted, `nvidia_drm modeset=1 fbdev=1`
  (`astro/platform/x86_64/rootfs/etc/modprobe.d/beercade-gpu.conf`, notes in
  `astro/platform/x86_64/gpu.md`). The KMS launcher probes for the card with
  a connected output (skipping simpledrm/efifb), sets `SDL_VIDEODRIVER=kmsdrm`,
  `SDL_KMSDRM_DEVICE_INDEX`, and NVIDIA's GBM backend; `ASTRO_DEV` overrides.
- **Latency**: `astro-tune` (`astro/tune`, started before `astro-daemon`):
  `performance` governor/EPP, `AllowedCPUs` via direct cgroupfs writes (with
  `systemctl set-property --runtime` fallback) — `astro-rt.slice` gets
  `beercade-hal`'s realtime set, `system` / `user` slices and `init.scope` the
  rest — and holds `/dev/cpu_dma_latency` = 0.
- **Cores**: Arch `libretro-*` packages, see
  `astro/emulators/cores/cores_x86_64.md`.
- **Customer Shell (`astro-shell`) & Session Launch**: ES-DE has been removed
  and replaced by the resident Rust + SDL2 KMS shell (`astro/shell`), launched by
  `astro-frontend.service` (`astro_frontend_start.sh`). `astro-daemon` drives
  attract mode and `BRC-XXXX` patron sessions over `/run/astro/shell.sock`
  (`SIGSTOP`/`SIGCONT` DRM handoff in `shell_ipc.rs`), streams/caches ROMs via
  LRU staging with SHA-256 verification (`staging.rs`), and spawns
  `/usr/bin/retroarch` directly.
- **CPU floor x86-64-v3** (AVX2/FMA/BMI2, Haswell / Zen and newer) for
  compiled native/frontend binaries; `astro-frontend.service` checks CPU flags
  and skips itself below v3. QEMU gates run with `-cpu max` (TCG default is v1).
- **Optimisations applied** (see `docs/OPTIMIZATION_BACKLOG.md`): shader
  caches on `/data`, DATA grows to fill the disk on first boot (both x86
  images + ARM), split firmware packages, `usbhid.jspoll=1`, `astro_rt`
  1000 Hz Vulkan/udev pause bridge, and `sd_notify` readiness.

Open (needs decisions or hardware):
- **Gate not run**: frame-time measurement on real AMD, NVIDIA and Intel
  machines with `benchmark_frametimes.py`. Only QEMU boots are verified.
- **Missing cores**: FBNeo, DuckStation, PCSX2 are not packaged in Arch.
- `astro/rt/deploy` udev/limits rules are RK3588-specific (rockchipdp0 audio)
  and not installed on x86.

### M4 — Path ownership (done)
Root `CODEOWNERS` and `scripts/lint_governance.py` (CI `governance` job in
`.github/workflows/ci.yml`) enforce path ownership coverage, Cargo workspace
crate-boundary rules, `beercade-shared` platform-I/O purity, and confinement of
arch-specific boot files to `{shared,astro,boss}/platform/<arch>/`,
`shared/buildfarm/<arch>/`, or `shared/tests/`.

### M5 — End-to-end acceptance (harness ready; 24h hardware soak pending)
`shared/tests/m5_venue_soak.py` exercises `x86_64` + `aarch64` A.S.T.R.O.
nodes → B.O.S.S. (`:8080` + UDP `:9750` netplay relay + OTA `/api/v1/sync`) →
`POST /api/v1/edge/ingest` → D1/KV (`--mode smoke` self-contained local check;
`--mode venue --duration-secs 86400` against live fleet). See
`docs/M5_VENUE_SOAK_RUNBOOK.md` for full hardware soak procedures.

## Housekeeping
- Root `graphify-out/` holds the Graphify structural index (configured via
  `.graphifyignore`); refresh with `graphify update .` after structural changes.
- First-boot `DATA` partition resize uses `beercade-grow-data.service` across
  both x86_64 and aarch64 images (`astro/scripts/first_boot_resize.sh` was
  removed).
