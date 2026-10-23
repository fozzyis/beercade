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
| M1 | CI: build + test on every push | Done (x86_64); arm64 parked | Sonnet |
| M2 | A.S.T.R.O. x86_64 bootable image | Done | Opus + Sonnet |
| M2b | B.O.S.S. x86_64 bootable image | Done | Opus |
| M3 | A.S.T.R.O. PC runtime: GPUs, emulators, latency | After M2 | Opus |
| M4 | Path ownership rules and lint | Any time | Haiku |
| M5 | End-to-end acceptance: A.S.T.R.O. → B.O.S.S. → cloud ingest | Last | Sonnet |

### M0 — Hardware abstraction (done)
`shared/crates/beercade-hal`: platform detection (RK3588, Pi 5, Pi 4, generic
ARM, x86_64), thermals, A/B slot devices, active-slot detection, bootloader
commit, realtime CPU selection. The daemon's OTA, telemetry and thermal
watchdog go through it. ARM behaviour unchanged.

Open item: a failed thermal read is reported as `null` in telemetry. Making
`cpu_temp_c` / `gpu_temp_c` optional in `beercade-shared` (and handling it on
the website dashboard) is a contract change awaiting a decision.

### M1 — CI (done for x86_64)
`.forgejo/workflows/ci.yml`. Runners: `laptop-x86_64` (Docker Desktop,
label `x86_64`) and the RK3588 runner (label `arm64`). Rust check, test and
release build plus website and kiosk type checks (Node 22).

Open item: the arm64 job failed on its first run and is parked; logs not yet
reviewed. Suspect: the timing-sensitive netplay loopback test.

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
- Root's password is locked; tty1 autologins as root (from `astro/rootfs`).
  systemd accepts credentials from SMBIOS, so anything controlling the
  firmware tables can inject units — fine for a kiosk, revisit for hardening.
- Not CI-gated yet: the build needs `--privileged` and ~5 GB, and TCG boot
  takes ~4–5 min. Candidate for a manual/nightly job on the laptop runner.
- `boss-static-net.service` from `astro/rootfs` is copied but not enabled.
- Image is 4.75 GiB raw (sparse partitions); M2b added `.zst` and `.vhdx` outputs.

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
- Hyper-V: the initramfs includes `hv_vmbus hv_storvsc hyperv_keyboard`.
  Hyper-V Generation 2 VMs need Secure Boot off (images are unsigned):
  `New-VM -Generation 2 -VHDPath out\<image>.vhdx`, `Set-VMFirmware
  -EnableSecureBoot Off`. Not exercised by the QEMU gates.

Notes for later milestones:
- M5: B.O.S.S. answers on :8080 at 192.168.100.10; `boss.lan` needs DNS on the
  venue router (avahi publishes `boss.local`). The x86 A.S.T.R.O. image
  hard-codes 192.168.100.2, so two x86 nodes on one LAN clash.
- `beercade-hal` slot switching works with this layout; nothing marks a slot
  good/bad yet (no boot counting).
- The kiosk runs unaccelerated in QEMU; real GPUs need the M3 driver work.

### M3 — A.S.T.R.O. PC runtime
- Build the project's modified RetroArch (it carries the
  `astro_input_drain_pre_vblank` hook) and `astro/rt` (C++ evdev input) for
  x86_64. Stock Arch RetroArch lacks the hook and is not a substitute.
  Mesa and ES-DE can come from Arch packages.
- GPU support and boot-time driver selection: AMD, NVIDIA (`nvidia-open`),
  Intel. Fix the KMS launcher's card probe and `SDL_VIDEODRIVER` (`kmsdrm`).
- Latency tuning as systemd units (CPU governor, PM QoS, `AllowedCPUs=`
  slices), core counts from `beercade-hal`, not hard-coded.
- Emulator core set for x86_64.
- **Gate:** frame-time measurement with
  `astro/emulators/benchmarks/benchmark_frametimes.py` on real hardware for at
  least one AMD, one NVIDIA and one Intel machine. QEMU numbers don't count.

### M4 — Path ownership
CODEOWNERS and a lint that keeps platform-specific files inside
`astro/platform/<arch>/` and HAL code.

### M5 — End-to-end acceptance
Extend `shared/tests/run_acceptance_tests.py`: x86 A.S.T.R.O. node → B.O.S.S.
(x86 image) → `POST /api/v1/edge/ingest` → D1, plus mixed ARM/x86 telemetry.

## Housekeeping
- The graphify graph in `.ai-ignore/archive/graphify-out` is stale (1 Oct)
  and dominated by `node_modules`, type definitions and shaders. Regenerate
  with a proper `.graphifyignore` before relying on it.
- First-boot `DATA` partition resize on x86 (`astro/scripts/first_boot_resize.sh`
  is ARM-oriented).
