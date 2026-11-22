# B.E.E.R.C.A.D.E. Workspace Rules (`AGENTS.md`)

## 1. Shared Context (OpenViking MCP)
The `openviking` MCP server (`https://viking.3hx.au/mcp`) is the shared architectural memory across agent surfaces.
- Before architecture, hardware, CI, or ops work, query `viking://resources/beercade/` (ADRs, architecture, contracts, hardware, runbooks).
- Durable cross-session memories live in `viking://user/Claude-Claymore/memories/` (repo remotes, Omnius host facts, user preferences).
- Save new durable learnings (ADRs, host facts, hardware quirks) to `viking://user/Claude-Claymore/memories/` as well as local workspace docs.
- If OpenViking is unreachable, proceed with local repository context and state that it was offline.

## 2. Workspace Architecture & Crate Boundaries
Cargo workspace root: `Cargo.toml` (`resolver = "2"`, release profile: `lto = "thin"`, `codegen-units = 1`, `strip = true`, `panic = "unwind"` for Tokio task containment).

| Component | Path | Responsibility & Key Invariants |
| :--- | :--- | :--- |
| **`beercade-shared`** | `shared/crates/beercade-shared` | Wire & IPC contracts (`NodeTelemetry` with `Option<f32>` thermals, `CabinetCommand`, `CommandRecord`, `CommandAck`, `CreditCounters`), auth primitives, URL helpers. Zero platform-specific I/O. |
| **`beercade-hal`** | `shared/crates/beercade-hal` | Hardware Abstraction Layer: SoC/arch detection (`x86_64`, `aarch64`, `rk3588`, `bcm2712`), `hwmon`/thermal zone readers, dual-slot A/B rootfs state. |
| **`astro-daemon`** | `astro/daemon` | Cabinet node supervisor: KMS game launcher, rollback netplay, mandatory `X-OTA-SHA256` staging verification & cleanup, atomic credit persistence (`credits.json.tmp` -> `sync_all()` -> rename + `.bak` tiered recovery). |
| **`astro-shell` / `astro-tune` / `astro/rt`** | `astro/shell`, `astro/tune`, `astro/rt` | Cabinet shell utilities, low-latency tuning CLI, and real-time C/C++ helpers (`astro/rt/CMakeLists.txt`). |
| **`boss-server`** | `boss` | Central Operations & Supervisory System (Axum REST/WS/SSE): operator PIN auth with 5-failure/60s lockout (`PinAttemptTracker`), command queue & ACK ingestion, heartbeat/thermal alerting, OTA distribution. |
| **Operator & Kiosk UIs** | `boss/kiosk_ui`, `website`, `boss/plugin_sdk` | Frontend TypeScript/SvelteKit/Vite interfaces and plugin SDK. |

## 3. Build, Test & Hygiene Directives
- **Never Traverse or Index Ignored Artifacts:** Skip `.ai-ignore/`, `target/`, `out/`, `astro/roms/`, `astro/rt/build/`, `node_modules/`, `.svelte-kit/`, `*.img`, `*.squashfs`, `*.zst`, and `*.mp4`.
- **Targeted Verification Commands:**
  - Fast compile/type check: `cargo check --workspace --message-format=short`
  - Scoped crate tests: `cargo test -p beercade-shared -p beercade-hal -p astro-daemon -p boss-server --quiet`
- **Contract Changes:** Any schema change in `beercade-shared` or `shared/proto/telemetry.ts` must be verified against both `astro-daemon` and `boss-server` plus their unit/integration test suites before completion.
