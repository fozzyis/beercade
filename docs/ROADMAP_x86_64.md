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
| M2 | A.S.T.R.O. x86_64 bootable image | Next | Opus + Sonnet |
| M2b | B.O.S.S. x86_64 bootable image | After M2 | Sonnet, Opus review |
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

### M2 — A.S.T.R.O. x86_64 image
Shared image assembler (reused by M2b) plus the A.S.T.R.O. image.
- Build runs in Docker on the laptop; partitions built as files, no loop devices.
- Minimal Arch rootfs via pacstrap; `astro-daemon` + systemd unit.
- Generic initramfs (no autodetect) with an overlay hook that mounts the
  squashfs root read-only under a tmpfs overlay.
- systemd-boot, per-slot kernel and initramfs on the ESP.
- **Gate:** `make image-x86` produces the image; an automated QEMU (UEFI)
  boot reaches multi-user.target with `astro-daemon.service` active.

### M2b — B.O.S.S. x86_64 image
Same assembler, different payload.
- `boss-server`, built `boss/kiosk_ui`, `boss-server.service`,
  `boss-kiosk.service` (cage + chromium direct to KMS, seatd), static
  networking.
- No emulators, no latency tuning.
- B.O.S.S. state (database, tokens, bookings, OTA payloads) on `/data`.
- **Gate:** boots in QEMU; `boss-server` answers on :8080; kiosk service
  starts; data written to `/data` survives a reboot; an A/B slot switch with
  `bootctl` boots the other slot.

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
