# B.E.E.R.C.A.D.E. Architecture Decision Records & Host Facts (GA Baseline)

> **OpenViking MCP Sync Note:** The `openviking` MCP endpoint (`https://viking.3hx.au/mcp`) was offline/unmounted during the Phase 7 session. This file stores the authoritative GA ADRs and host facts locally so they can be mirrored into `viking://resources/beercade/` and `viking://user/Claude-Claymore/memories/` once OpenViking is reachable.

---

## Architecture Decision Records (ADRs)

### ADR-01 — Single-Branch Multi-Arch Monorepo & M4 Governance
- **Context:** Support both `x86_64` PC arcade hardware and `aarch64` SBCs (RK3588, RPi 4/5) from a single `main` branch without fork drift.
- **Decision:** Enforce strict path ownership via root `CODEOWNERS` (`@fozzyis`) and automated CI checks (`scripts/lint_governance.py`, `.github/workflows/ci.yml` `governance` job):
  - `beercade-shared`: zero workspace-crate deps, zero platform I/O/arch tokens (`target_arch`, `/sys/class/`, `/proc/device-tree`, `efivars`).
  - `beercade-hal`: zero workspace-crate deps; encapsulates all SoC/arch, thermal (`hwmon`), A/B slot, and realtime CPU probing.
  - `astro-daemon` / `boss-server`: depend only on `beercade-shared` and `beercade-hal` (never each other).
  - `astro-shell`: depends only on `beercade-shared`.
  - `astro-tune`: depends only on `beercade-hal`.
  - Platform boot configs (`config.txt`, `boot.cmd`, `mkinitcpio.conf`, `loader.conf`, `beercade-slot-*.conf`, `*.dtb`) must remain under `{shared,astro,boss}/platform/<arch>/`, `shared/buildfarm/<arch>/`, or `shared/tests/`.

### ADR-02 — Typed Wire Contracts & `Option<f32>` Thermals
- **Context:** Rust (`boss-server`, `astro-daemon`, `astro-shell`) and TypeScript (`boss/kiosk_ui`, `website` Cloudflare Workers) share telemetry and session contracts.
- **Decision:** `shared/crates/beercade-shared/src/models.rs` generates `shared/proto/telemetry.ts` via `cargo run -p beercade-shared --locked --bin gen_ts_proto -- --write` and is CI-checked with `--check`. Failed/absent thermal probes report `Option<f32>` (`None` / JSON `null`) with NaN validation rather than synthetic placeholder temperatures.

### ADR-03 — Resident `astro-shell` & `astro_rt` Vulkan/udev Pause Bridge
- **Context:** Stock ES-DE caused multi-second launch latency, held local ROM menus, and left `astro_rt` unused.
- **Decision:** ES-DE was replaced by the resident Rust + SDL2 KMS shell (`astro/shell`), driven over `/run/astro/shell.sock` (`SIGSTOP`/`SIGCONT` DRM handoff in `astro/daemon/src/shell_ipc.rs`). `astro-daemon` stages B.O.S.S. session/attract ROMs via streaming SHA-256 verification + LRU cache (`staging.rs`) and spawns `/usr/bin/retroarch` directly with `0002-astro-rt-vulkan-udev-input.patch` (1000 Hz `astro_rt` polling + `/dev/shm/astro_rt_stats` + `/run/astro/menu.sock`).

### ADR-04 — Verified A/B Rootfs OTA & Health-Gated Boot Blessing
- **Context:** Unattended cabinets require crash-safe OTA rootfs updates with automatic rollback if a new slot fails health checks.
- **Decision:** `boss/src/ota_sync.rs` streams payloads with `Content-Length` bounds and `X-OTA-SHA256`; `astro/daemon/src/ota.rs` verifies free space (`statvfs` +64 MiB), SHA-256 integrity, writes the inactive `ROOT_A`/`ROOT_B` slot, and invokes `beercade-hal` slot switching. `beercade-boot-check.sh` validates service health (`/api/health` on B.O.S.S., `astro-daemon` + `astro-frontend` on A.S.T.R.O.) before blessing the slot (`systemd-bless-boot` on x86_64, `tryboot.txt` promotion on RPi, U-Boot `bootcount`/`altbootcmd` on RK3588).

### ADR-05 — Split `linux-firmware-*` & Hyper-V Gen2 VHDX Support
- **Context:** Arch Linux split `linux-firmware` into vendor packages; real PC NICs/GPUs and Hyper-V Gen2 `.vhdx` verification require explicit coverage without bloating images with full `linux-firmware`.
- **Decision:** Both `shared/buildfarm/x86_64/payloads/astro.conf` and `boss.conf` install split `linux-firmware-{amdgpu,intel,radeon,realtek,mediatek,atheros,broadcom,marvell,other}`, `mesa`, `vulkan-{radeon,intel,icd-loader}`, `curl`, and `hyperv` (`hv_kvp_daemon.service` with `ConditionVirtualization=microsoft`). Automated Gen2 `.vhdx` verification uses `shared/tests/verify_hyperv_x86_64.ps1` (Secure Boot off) alongside `shared/tests/verify_qemu_x86_64.sh`.

---

## Host, CI, & Venue Topology Facts

| Item | Value / Contract |
| :--- | :--- |
| **Git Remote & Owner** | `https://github.com/fozzyis/beercade.git` (`@fozzyis`) |
| **GitHub Actions CI Jobs** | `rust` (`matrix`: `x86_64` on `ubuntu-24.04`, `arm64` on `ubuntu-24.04-arm`, Rust + `gen_ts_proto` + `astro_rt` CMake/CTest), `frontends` (`ubuntu-24.04`, Node 22, SvelteKit checks), `governance` (`ubuntu-24.04`, M4 `CODEOWNERS`/crate lint + x86 payload/VHDX static tests) |
| **B.O.S.S. Default Ports** | `:8080` in `boss-server.service` (binary default `8989` if `BOSS_PORT` unset); `:9750` UDP netplay discovery/fallback relay (`BOSS_NETPLAY_UDP_PORT`); `:8990` UDP LAN beacon (`BOSS_DISCOVERY_KEY` HMAC) |
| **A.S.T.R.O. Network & Endpoint** | DHCP (`astro/rootfs/etc/systemd/network/10-wired.network`); `astro-daemon` defaults `BOSS_ENDPOINT` to `http://boss:8080` |
| **Cloudflare Edge Stack** | `website/src/routes/api/v1/edge/{ingest,latest,stream}/+server.ts` -> D1 (`0001`–`0003` migrations) + KV (`batch:seen:*`, `latest:node:*`, `latest:venue:*`); requires real D1/KV IDs in `website/wrangler.jsonc` and `EDGE_INGEST_TOKEN` before live cloud soak |
| **Local Workstation Notes** | Windows 11 host (`i7-14650HX` / `RTX 4060 Laptop`, PowerShell shell, Python `3.9.13`, Hyper-V cmdlets available); `astro-shell` links system `SDL2`/`SDL2_ttf` so full `--workspace` native cargo builds run in Linux/WSL2/Docker CI (`scripts/ci_local.sh`), while pure-Rust crates (`beercade-shared`, `beercade-hal`, `astro-daemon`, `astro-tune`, `boss-server`) build & test natively on Windows |
| **M5 Verification Harnesses** | `shared/tests/m5_venue_soak.py --mode smoke` (self-contained local `boss-server` + SQLite D1/KV + UDP `:9750` + `BRC-XXXX` + OTA round-trip) and `--mode venue --duration-secs 86400` (see `docs/M5_VENUE_SOAK_RUNBOOK.md`) |
