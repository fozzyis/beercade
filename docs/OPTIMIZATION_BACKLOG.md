# Optimisation backlog — x86_64 and RK3588

Found while doing M3 and the ES-DE frontend (2026-10-24). Each entry says
which platform it applies to, the expected win, the effort, and whether it is
**done**, **proposed** (small, low-risk, ready to land after a yes) or
**later** (needs measurement, hardware or a decision).
Measure latency changes with `astro/emulators/benchmarks/benchmark_frametimes.py`
on real hardware; QEMU numbers don't count.

## Done (x86_64, 2026-10-24)

- **CPU floor x86-64-v3** (AVX2/FMA/BMI2) for everything we compile
  (RetroArch, astro_rt, ES-DE, FreeImage). `astro-frontend.service` checks the
  CPU and skips itself (console stays on tty1) below v3. Arch's prebuilt
  cores stay generic x86-64 (see L5).
- **P1** shader caches on `/data/beercade/cache` (Mesa, NVIDIA; XDG cache).
- **P2** DATA grows to fill the disk on first boot (`beercade-grow-data.service`
  + `x-systemd.growfs`), on both x86 images. ARM still uses its own script.
- **P3** `linux-firmware` split: amdgpu, intel, realtek, mediatek, atheros,
  broadcom only.
- **P4** `usbhid.jspoll=1` on the x86 A.S.T.R.O. kernel command line.
- **P5** (partly) ES-DE `VideoHardwareDecoding=true`. `ParseGamelistOnly`
  was *not* enabled: it hides every game that isn't already in a
  `gamelist.xml`, so a fresh cabinet with copied-in ROMs shows nothing. Moved
  to L14.

## Proposed (small, low-risk)

| # | Platform | Change | Win | Effort |
|---|---|---|---|---|
| ~~P1~~ | x86 | Done. **Persistent shader caches.** The x86 root is squashfs + tmpfs, so Mesa/NVIDIA shader caches and RetroArch's Vulkan pipeline cache are lost every boot, meaning shader compiles and stutter on the first frames of every game, every boot. Set `XDG_CACHE_HOME=/data/beercade/cache`, `MESA_SHADER_CACHE_DIR`, `__GL_SHADER_DISK_CACHE_PATH` (+`_SKIP_CLEANUP=1`) in `astro-frontend.service`; RetroArch inherits them from ES-DE. ARM already keeps its overlay upper on `/data`, so it's unaffected. | No per-boot shader stutter | ~10 lines |
| P2 | ARM | Done on x86. **Grow DATA to fill the disk** on first boot with `systemd-repart` (`GrowFileSystem=yes`) instead of the ARM-only `first_boot_resize.sh`. DATA is 256 MiB in the images; ROMs, saves and caches won't fit. | Usable storage; prerequisite for P1 at scale | Small (repart.d file + unit); test on a larger disk in QEMU |
| ~~P3~~ | x86 | Done. **Firmware split.** Arch now ships `linux-firmware-{amdgpu,intel,nvidia,radeon,realtek,other,...}`. Install only GPU + common NIC/Wi-Fi sets instead of the full meta-package. | Smaller squashfs/OTA (≈ hundreds of MiB, to measure) | Payload list edit |
| P4 | ARM | Done on x86. **1000 Hz USB gamepad polling**: `usbhid.jspoll=1` (joysticks/pads) on the kernel command line (x86 loader entries; RK3588 `cmdline.txt`). Encoders that already run at 1 kHz are unaffected. | Up to ~7 ms less input lag on 125 Hz pads | One kernel arg |
| P5 | ARM | Done on x86 (hardware decoding only). **ES-DE settings**: `VideoHardwareDecoding=true` (VAAPI/NVDEC on x86; check rkmpp support on RK3588), seeded in `/data/beercade/ES-DE/settings/es_settings.xml`. | Faster frontend start, less CPU in menus | Settings file |
| P6 | ARM | **ARM RetroArch build drops the project patch.** `shared/buildfarm/scripts/build_retroarch.sh` clones RetroArch *master*, unpinned, with no patches and no `HAVE_ASTRO_RT_INPUT`. Make it use the same tag and `astro/emulators/retroarch/patches/` as x86. | ARM gets the hook it is documented to have; reproducible builds | Small; needs an ARM build to verify |
| P7 | ARM | **Broken RK3588 ES-DE launcher**: `boot_esde.sh` runs a dev path (`/data/dev/beercade/emu_opti/rk3588_optimizer.sh`), calls `emulationstation` (ES-DE 3.x binary is `es-de`) and hard-codes `taskset -c 4-7`. Run ES-DE via `astro-frontend.service` + `astro-tune` (HAL already knows the A76 cluster), build ES-DE with `-DGLES=ON` using the same `NATIVE_BUILDS` script. | Working, HAL-driven ARM frontend | Medium |

## Later (needs measurement, hardware or a decision)

| # | Platform | Idea | Notes |
|---|---|---|---|
| L1 | both | **Preemptive frames / run-ahead** per core in RetroArch for light cores (Snes9x, Genesis Plus GX, Nestopia, FBNeo/MAME 2003+) | Removes 1–2 frames of game-internal lag. x86 has headroom; on RK3588 enable only per core after measuring. Needs per-core override files. |
| L2 | both | **astro-tune: devfreq + GPU clocks.** RK3588: set `/sys/class/devfreq/*/governor` (GPU `fb000000.gpu`, DMC memory controller) to `performance`. x86: amdgpu `power_dpm_force_performance_level=high`, Intel `gt_min_freq_mhz` = max, NVIDIA persistence mode. | GPU/memory clock ramp-up is a common frame-time spike source. Thermals on RK3588 passive cooling must be checked. |
| L3 | both | **IRQ affinity** in astro-tune: pin device IRQs to housekeeping CPUs, except the USB controller carrying the encoders (keep it on an RT core or measure both). | Pairs with the existing `AllowedCPUs` split. |
| L4 | x86 | **Kernel choice**: Arch `linux` is PREEMPT_DYNAMIC; try `preempt=full` (one arg), then `linux-zen` or `linux-rt` (both in the repos). | Measure; `nvidia-open` must match the chosen kernel (`nvidia-open-dkms` or a matching module package). |
| L5 | x86 | **Hot cores built for x86-64-v3** (AVX2) via `NATIVE_BUILDS`, keeping the v2 Arch packages as fallback. Candidates: Beetle PSX HW (software renderer), Mupen64Plus-Next (ParaLLEl-RDP), MAME. | Arch `libretro-*` are generic x86-64. Only worth it where profiles show CPU-bound frames. |
| L6 | both | **Arcade core: FBNeo** alongside MAME. Lighter and lower latency for CPS/Neo Geo/Cave; on the ARM manifest but not packaged in Arch. Plain `make`, easy `NATIVE_BUILDS` script. | Proposal-ready once wanted. |
| L7 | both | **PS1 core**: the ARM manifest builds `duckstation_libretro.so` from upstream DuckStation, which no longer ships a libretro core (SwanStation is the libretro fork). Use Beetle PSX HW or SwanStation on both platforms. | ARM manifest fix. |
| L8 | ARM | **PS2 on RK3588**: manifest lists PCSX2 with `-DENABLE_DIRECT_KMS=ON`, which isn't a PCSX2 build option as far as I can find; PCSX2's ARM64 support status needs checking before any work. x86 uses `libretro-play` for now. | Verify upstream first. |
| L9 | both | **One RetroArch config per platform.** RK3588 has three conflicting ones (`astro/emulators/configs/retroarch_rk3588.cfg`: Vulkan, frame delay 4; `astro/rt/deploy/retroarch_rk3588.cfg`: GL, threaded, vsync off; `astro/rootfs/root/.config/retroarch/retroarch.cfg`: GL, `audio_driver = "null"`). The last one wins and has no audio. | Decide the real RK3588 settings, then delete the others. x86 uses `video_frame_delay_auto`; consider it on RK3588 too. |
| L10 | both | **Wire the input hook for real** (see roadmap M3): start the `astro_rt` producer and feed its events to RetroArch's input, in the video driver actually used (Vulkan `khr_display`), not only plain `drm`. | The largest latency item; needs encoder hardware to verify. |
| L11 | both | **Smaller OTA payloads**: `mksquashfs -comp zstd -Xcompression-level 19 -b 1M`. zstd decode speed barely depends on level, so boot cost should stay flat. | Measure size and cold-start time. |
| L12 | build | **ccache volume** for `NATIVE_BUILDS` (RetroArch + ES-DE + FreeImage rebuild from scratch, ~20 min per image). | Dev-loop speed only. |
| L14 | both | **ES-DE `ParseGamelistOnly=true`** once every system has a curated/scraped `gamelist.xml` | Faster start on big sets, but games missing from gamelists disappear. |
| L15 | both | **CPU microcode in the initramfs** (`amd-ucode`/`intel-ucode` + mkinitcpio `microcode` hook) | Not in the image today. Stability/security fixes, can change performance; test on hardware. |
| L16 | x86 | **B.O.S.S. image has no firmware packages** | Fine in QEMU; real boards may need `linux-firmware-realtek` (NIC) and GPU firmware for the kiosk. |
| L17 | x86 | **`OnFailure=getty@tty1` fires on every frontend crash**, not only after the restart limit; getty starts and is stopped again by the next restart | Harmless but noisy; use `OnFailure` on a small wrapper or `ExecStopPost` checking `$SERVICE_RESULT`. |
| L13 | x86 | **ES-DE `RunInBackground`** stays `false` (ES-DE stops rendering while a game runs). Keep it; listed so nobody flips it for convenience. | No change. |
