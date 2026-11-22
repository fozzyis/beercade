# Optimisation backlog — x86_64 and RK3588

Found during M3, `astro-shell` rollout, and Phase 7 GA prep. Each entry says
which platform it applies to, the expected win, the effort, and whether it is
**done**, **superseded**, **proposed** (small, low-risk, ready to land after a yes) or
**later** (needs measurement, hardware or a decision).
Measure latency changes with `astro/emulators/benchmarks/benchmark_frametimes.py`
on real hardware; QEMU numbers don't count.

## Done (x86_64 & GA HEAD)

- **CPU floor x86-64-v3** (AVX2/FMA/BMI2) for compiled native binaries
  (RetroArch, `astro_rt`, `astro-shell` via runtime check). `astro-frontend.service`
  checks the CPU and skips itself (console stays on tty1) below v3. Arch's prebuilt
  cores stay generic x86-64 (see L5).
- **P1** shader caches on `/data/beercade/cache` (Mesa, NVIDIA; XDG cache).
- **P2** DATA grows to fill the disk on first boot (`beercade-grow-data.service`
  + `x-systemd.growfs`), on both x86 and ARM images (`aarch64/payloads/*.conf`).
- **P3** `linux-firmware` split across both `astro.conf` and `boss.conf`:
  `amdgpu`, `intel`, `radeon`, `realtek`, `mediatek`, `atheros`, `broadcom`,
  `marvell`, `other` (plus `nvidia-utils` GSP firmware on `astro.conf`).
- **P4** `usbhid.jspoll=1` on the x86 A.S.T.R.O. kernel command line.
- **P5** Superseded: ES-DE was removed (`bb34ebb`, `3dc2b89`) and replaced by the
  resident Rust + SDL2 `astro-shell` (`astro/shell`).

## Proposed (small, low-risk)

| # | Platform | Change | Win | Effort |
|---|---|---|---|---|
| ~~P1~~ | x86 | Done. **Persistent shader caches.** The x86 root is squashfs + tmpfs, so Mesa/NVIDIA shader caches and RetroArch's Vulkan pipeline cache are lost every boot, meaning shader compiles and stutter on the first frames of every game, every boot. Set `XDG_CACHE_HOME=/data/beercade/cache`, `MESA_SHADER_CACHE_DIR`, `__GL_SHADER_DISK_CACHE_PATH` (+`_SKIP_CLEANUP=1`) in `astro-frontend.service`. | No per-boot shader stutter | ~10 lines |
| ~~P2~~ | both | Done. **Grow DATA to fill the disk** on first boot with `beercade-grow-data.service` (`systemd-repart` / `x-systemd.growfs`) across both `x86_64` and `aarch64` payloads (replacing legacy `first_boot_resize.sh`). | Usable storage; prerequisite for P1 at scale | Done |
| ~~P3~~ | x86 | Done. **Firmware split.** Arch ships `linux-firmware-{amdgpu,intel,nvidia,radeon,realtek,marvell,other,...}`. Both `astro.conf` and `boss.conf` install the split GPU + NIC/Wi-Fi sets instead of the full meta-package. | Smaller squashfs/OTA while covering real PC NICs/GPUs | Done |
| P4 | ARM | Done on x86. **1000 Hz USB gamepad polling**: `usbhid.jspoll=1` (joysticks/pads) on the kernel command line (x86 loader entries; RK3588 / Pi `cmdline.txt`). Encoders that already run at 1 kHz are unaffected. | Up to ~7 ms less input lag on 125 Hz pads | One kernel arg |
| ~~P5~~ | both | Superseded. **ES-DE settings** (`VideoHardwareDecoding`, `ParseGamelistOnly`): ES-DE was replaced by resident `astro-shell` driven by B.O.S.S. sessions. | Cleaner UX, zero local ROM clutter | Removed |
| P6 | ARM | **ARM RetroArch build parity.** Legacy `shared/buildfarm/scripts/build_retroarch.sh` was purged (`3dc2b89`). Add an `aarch64` counterpart to `astro/emulators/retroarch/build_x86_64.sh` carrying `astro/emulators/retroarch/patches/` + `astro_rt`, and add `astro-shell` + RetroArch + cores to `shared/buildfarm/aarch64/payloads/astro.conf`. | ARM gets the `astro_rt` pause bridge & full session stack | Medium; needs ARM build |
| ~~P7~~ | ARM | Superseded/Done. Legacy `boot_esde.sh` / `rk3588_optimizer.sh` path removed; `astro-frontend.service` runs `astro-shell` + `astro-tune` on `aarch64`. | HAL-driven ARM frontend | Done |

## Later (needs measurement, hardware or a decision)

| # | Platform | Idea | Notes |
|---|---|---|---|
| L1 | both | **Preemptive frames / run-ahead** per core in RetroArch for light cores (Snes9x, Genesis Plus GX, Nestopia, FBNeo/MAME 2003+) | Removes 1–2 frames of game-internal lag. x86 has headroom; on RK3588 enable only per core after measuring. Needs per-core override files. |
| L2 | both | **astro-tune: devfreq + GPU clocks.** RK3588: set `/sys/class/devfreq/*/governor` (GPU `fb000000.gpu`, DMC memory controller) to `performance`. x86: amdgpu `power_dpm_force_performance_level=high`, Intel `gt_min_freq_mhz` = max, NVIDIA persistence mode. | GPU/memory clock ramp-up is a common frame-time spike source. Thermals on RK3588 passive cooling must be checked. |
| L3 | both | **IRQ affinity** in astro-tune: pin device IRQs to housekeeping CPUs, except the USB controller carrying the encoders (keep it on an RT core or measure both). | Pairs with the existing `AllowedCPUs` split. |
| L4 | x86 | **Kernel choice**: Arch `linux` is PREEMPT_DYNAMIC; try `preempt=full` (one arg), then `linux-zen` or `linux-rt` (both in the repos). | Measure; `nvidia-open` must match the chosen kernel (`nvidia-open-dkms` or a matching module package). |
| L5 | x86 | **Hot cores built for x86-64-v3** (AVX2) via `NATIVE_BUILDS`, keeping the v2 Arch packages as fallback. Candidates: Beetle PSX HW (software renderer), Mupen64Plus-Next (ParaLLEl-RDP), MAME. | Arch `libretro-*` are generic x86-64. Only worth it where profiles show CPU-bound frames. |
| L6 | both | **Arcade core: FBNeo** alongside MAME. Lighter and lower latency for CPS/Neo Geo/Cave; not packaged in Arch. Plain `make`, easy `NATIVE_BUILDS` script. | Proposal-ready once wanted. |
| L7 | both | **PS1 core**: the ARM manifest builds `duckstation_libretro.so` from upstream DuckStation, which no longer ships a libretro core (SwanStation is the libretro fork). Use Beetle PSX HW or SwanStation on both platforms. | ARM manifest fix. |
| L8 | ARM | **PS2 on RK3588**: verify upstream PCSX2 ARM64 status before any work. x86 uses `libretro-play` for now. | Verify upstream first. |
| ~~L9~~ | both | Done. **One RetroArch config per platform** (`687a83b` consolidated to `astro/rootfs/root/.config/retroarch/retroarch.cfg` with `audio_driver = "alsa"`, plus `astro/platform/x86_64/rootfs/root/.config/retroarch/retroarch.cfg` using Vulkan `khr_display` and `video_frame_delay_auto`). | Done |
| ~~L10~~ | both | Done. **Wire the input hook for real** (`687a83b` + `5978d03` wired `0002-astro-rt-vulkan-udev-input.patch`, `astro_input_start()`, `/dev/shm/astro_rt_stats`, and `/run/astro/menu.sock` pause bridge into Vulkan `khr_display` + udev). Needs encoder hardware measurement. | Done |
| L11 | both | **Smaller OTA payloads**: `mksquashfs -comp zstd -Xcompression-level 19 -b 1M`. zstd decode speed barely depends on level, so boot cost should stay flat. | Measure size and cold-start time. |
| L12 | build | **ccache volume** for `NATIVE_BUILDS` (RetroArch rebuilds from scratch on clean volume). | Dev-loop speed only. |
| ~~L14~~ | both | Superseded. **ES-DE `ParseGamelistOnly=true`**: ES-DE removed in favor of `astro-shell`. | Removed |
| L15 | both | **CPU microcode in the initramfs** (`amd-ucode`/`intel-ucode` + mkinitcpio `microcode` hook) | Not in the image today. Stability/security fixes, can change performance; test on hardware. |
| ~~L16~~ | x86 | Done (Task 7.2). **B.O.S.S. image firmware & GPU drivers**: `boss.conf` now includes split `linux-firmware-*` NIC/GPU packages, `hyperv`, and `mesa` + `vulkan-{radeon,intel,icd-loader}`. | Done |
| L17 | x86 | **`OnFailure=getty@tty1` fires on every frontend crash**, not only after the restart limit; getty starts and is stopped again by the next restart | Harmless but noisy; use `OnFailure` on a small wrapper or `ExecStopPost` checking `$SERVICE_RESULT`. |
| ~~L13~~ | x86 | Superseded. **ES-DE `RunInBackground`**: ES-DE removed in favor of resident `astro-shell`. | Removed |

## B.O.S.S. (found 2026-10-27)

B1–B7 done 2026-10-27 (B5 left the `.woff` fallbacks in place: the latin CSS still references them, Chromium never fetches them). B8–B10 done 2026-10-27: B8 is `BOSS_UI_EFFECTS` (lite by default on ARM), B9 is the root `.cargo/config.toml`, B10 is `shared/buildfarm/aarch64` + `boss/platform/aarch64` (A/B slot switching, `tryboot.txt` / U-Boot `bootcount` + `beercade-boot-check.sh` landed in `687a83b`; Pi 5 16 KB `kernel_2712.img` packaging remains an open hardware parity item since `shared/platform/universal-aarch64/boot/config.txt` still boots `kernel8.img`).

| # | Platform | Change | Win | Effort |
|---|---|---|---|---|
| ~~B1~~ | both | **Edge buffer churns when cloud sync is off.** `buffer_edge_item` always buffers, but `run_edge_forwarder` returns early when `EDGE_INGEST_URL`/token are unset (the shipped default). The `Vec` fills to 4096 and every heartbeat then does `remove(0)`, shifting ~4k `EdgeItem`s, forever. Skip buffering when sync is disabled, and use a `VecDeque` (`pop_front`) for the bounded case. | Removes a constant memmove per heartbeat; matters most on a Pi | Small |
| ~~B2~~ | both | **No `[profile.release]`** in the workspace. Add `lto = "thin"`, `codegen-units = 1`, `strip = true` (keep `panic = "unwind"`: tokio tasks rely on it). | Smaller binaries and OTA payloads, a few % CPU | One block; slower release builds (the emulated arm64 CI job is already ~45 min) |
| ~~B3~~ | both | **`node_online` never re-fires.** `ingest_telemetry` decides "came online" with `nodes.contains_key`, but stale nodes are never removed from the map, so a cabinet that drops and returns raises no `node_online` alert or plugin event (lighting/alerting rules miss it). Track online state from `online_nodes(STALENESS_MS)` instead, or evict in `venue_broadcaster` when it reports `node_offline`. | Correct alerts; bounded map | Small (a bug more than an optimisation) |
| ~~B4~~ | both | **Static assets uncached and uncompressed.** `ServeDir` sends no `Cache-Control` and no compression. Add `Cache-Control: public, max-age=31536000, immutable` for `/_app/immutable/*`, and serve Vite's pre-compressed files (`ServeDir::precompressed_br/gzip` + a compression plugin at build time). | Faster `/tv` and LAN tablet loads; kiosk on loopback barely changes | Small |
| ~~B5~~ | both | **Kiosk fonts ship every subset in two formats.** The `@fontsource/*` imports pull latin, latin-ext, cyrillic, vietnamese… as `.woff2` and `.woff` (~0.9 MB of `.woff` Chromium never uses). Import the `latin` (and `latin-ext`) subset CSS only. | Smaller image/OTA; less CSS to parse | Small |
| ~~B6~~ | both | **`/tv` polls and streams.** It refetches `/api/telemetry` + `/api/v1/plugins` every 5 s on top of the SSE `venue`/`plugins` streams. Poll only while SSE is disconnected. | Less server and TV CPU | Small |
| ~~B7~~ | both | **SSE fan-out copies every message per client.** `broadcast::Sender<String>` clones the full venue report for each subscriber each second. Use `Arc<str>`; and skip building the 1 Hz report when `receiver_count() == 0`. | Scales with screens | Small |
| ~~B8~~ | ARM | **Kiosk/TV GPU cost on a Pi 4.** `backdrop-filter: blur()` (PIN modal), large glows/box-shadows (Vector Night, Pinball lamps) and the full-page starfield/wood backgrounds are the expensive bits for VideoCore VI. Drop the blur on ARM and measure the themes on real hardware; Taproom and Classic are the cheapest. | Smoother kiosk on Pi 4 | Measure first |
| ~~B9~~ | ARM | **Rust target CPU.** B.O.S.S. builds for generic aarch64. `-C target-cpu=cortex-a72` is a safe floor for Pi 4, Pi 5 and RK3588 (all ≥ A72). The server is I/O-bound, so expect little; worth it only alongside B2. | Small | `.cargo/config.toml` |
| ~~B10~~ | ARM | **There is no ARM B.O.S.S. image.** The spec names Raspberry Pi 4/5 as the primary B.O.S.S. appliance, but only `boss/platform/x86_64` exists (no ARM payload, units or tmpfiles). | Prerequisite for any ARM B.O.S.S. tuning | Medium |
