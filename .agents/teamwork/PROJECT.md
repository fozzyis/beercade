# Project: B.E.E.R.C.A.D.E. x86_64 RC1 Blockers & Core Robustness

## Architecture
B.E.E.R.C.A.D.E. is a distributed arcade platform consisting of:
- **`beercade-shared`**: Shared IPC data structures (`NodeTelemetry`, `CabinetCommand`, `CommandRecord`, `CommandAck`, `CreditCounters`), authentication primitives, and URL helpers.
- **`beercade-hal`**: Hardware abstraction layer for SoC detection, hardware temperature sensors (`hwmon` / thermal zones), and dual-slot A/B rootfs management.
- **`astro-daemon`**: Station node supervisor running on each cabinet. Manages input, game launching, credit accounting, rollback netplay, and OTA updates.
- **`boss-server`**: Central Operations & Supervisory System daemon. Exposes Axum REST, WebSocket, SSE APIs, operator PIN authentication, and command dispatching.

```
+-----------------------------------------------------------+
|                      B.O.S.S. Server                      |
|  - Operator PIN Auth & Rate Limiting (R5)                 |
|  - Command Queue, Dispatch & ACK Ingestion (R6)           |
|  - Heartbeat & Telemetry Ingestion (R4)                   |
+-----------------------------+-----------------------------+
                              | REST / WS / SSE
+-----------------------------v-----------------------------+
|                     beercade-shared                       |
|  - NodeTelemetry with Option<f32> thermals (R4)           |
|  - CabinetCommand, CommandRecord, CommandAck, Credits     |
+-----------------------------+-----------------------------+
                              |
+-----------------------------v-----------------------------+
|                    A.S.T.R.O. Daemon                      |
|  - Mandatory SHA-256 OTA Verification & Cleanup (R1)      |
|  - Crash-Safe & Atomic Credit Persistence (R2)            |
|  - Dynamic DHCP & Link-Local Network Config (R3)          |
|  - Clean Thermal Sampling (R4)                            |
|  - Command Dispatcher Execution (R6)                      |
+-----------------------------------------------------------+
```

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | R4: Option<f32> Thermal Contract | Refactor `NodeTelemetry` to use `Option<f32>` with `#[serde(default)]`; allow missing thermals without validation error | M1 | ORIGINAL_REQUEST §R4 |
| 2 | R4: Daemon Thermal Sampling Clean | Remove 42.0°C clamp in `main.rs`; return `Option<f32>` from `sample_temps()` in `netplay.rs` | M1 | ORIGINAL_REQUEST §R4 |
| 3 | R4: BOSS Thermal Telemetry Handling | Safely compute thermal alerts over `Option<f32>` in `plugins.rs` and accept `None` in heartbeats | M1 | ORIGINAL_REQUEST §R4 |
| 4 | R5: Operator PIN Rate Limiting | Add `PinAttemptTracker` in `boss/src/state.rs`; enforce 5-failure lockout (60s) in `POST /api/v1/auth/pin` | M2 | ORIGINAL_REQUEST §R5 |
| 5 | R6: Operator PIN Rate Limit Tests | Unit tests for PIN unlock success, failure counting, 429 lockout, and reset | M2 | ORIGINAL_REQUEST §R6 |
| 6 | R6: Command Queue & ACK Tests | Unit & integration tests for command queueing, polling, and ACK behavior in BOSS | M2 | ORIGINAL_REQUEST §R6 |
| 7 | R1: Mandatory OTA Hash Verification | Reject OTA staging if `X-OTA-SHA256` missing, empty, or mismatched; delete staged file on error | M3 | ORIGINAL_REQUEST §R1 |
| 8 | R2: Atomic Credit Persistence | Write `credits.json.tmp`, `sync_all()`, atomic rename to `credits.json` | M3 | ORIGINAL_REQUEST §R2 |
| 9 | R2: Credit Backup & Crash Recovery | Maintain `credits.json.bak`, tiered recovery in `load_credits` on boot | M3 | ORIGINAL_REQUEST §R2 |
| 10 | R3: Remove Static IP Conflicts | Remove `192.168.100.2` from `10-wired.network` & `boss_static_net.sh`; enable DHCP + IPv4LL fallback | M3 | ORIGINAL_REQUEST §R3 |
| 11 | R6: Credit Persistence & Recovery Tests | Unit tests for atomic write, corrupted file recovery, and rapid counter updates | M3 | ORIGINAL_REQUEST §R6 |
| 12 | R6: Command Dispatcher Execution Tests | Unit tests for command execution in ASTRO daemon (`execute_cabinet_command`) | M3 | ORIGINAL_REQUEST §R6 |
| 13 | R6: OTA Staging Tests | Unit tests for missing, empty, and bad SHA256 header rejection and cleanup | M3 | ORIGINAL_REQUEST §R6 |
| 14 | R6: Full Workspace Test Verification | Execute `cargo test` across all touched crates; verify 100% pass | M4 | ORIGINAL_REQUEST Acceptance |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Clean Telemetry Contract for Thermals | `beercade-shared`, `astro-daemon`, `boss-server`: `NodeTelemetry` `Option<f32>`, clamp removal, `validate()` & serde tests | none | DONE |
| M2 | B.O.S.S. Server Security & Command Plane | `boss-server`: PIN rate limiter, lockout (R5), command ACK handling & tests | M1 | DONE |
| M3 | A.S.T.R.O. Daemon Robustness & Network | `astro-daemon`: OTA verification (R1), credit persistence & recovery (R2), network config (R3), command exec & tests | M1 | DONE |
| M4 | Final Milestone: Integration Verification & Audit | Opaque-box E2E test verification, adversarial coverage hardening, forensic audit | M1, M2, M3 | DONE |

## Interface Contracts
### `beercade-shared` <-> `astro-daemon` & `boss-server`
- `NodeTelemetry`:
  - `cpu_temp_c: Option<f32>` (`#[serde(default)]`)
  - `gpu_temp_c: Option<f32>` (`#[serde(default)]`)
  - `validate()`: accepts `None` as valid (returns `Ok(())`). If `Some(val)`, requires `(-20.0..=120.0).contains(&val)`.
- `CabinetCommand`:
  - `Ping { seq: u64 }`
  - `AddCredits { credits: u32 }`
  - `StartSession { session_id: String, duration_secs: u64 }`
  - `EndSession`
  - `Reboot { delay_secs: u64 }`
  - `LaunchGame { core: String, rom: String, netplay: Option<NetplaySessionConfig> }`
- `CommandAck`:
  - `command_id: String`
  - `success: bool`
  - `detail: Option<serde_json::Value>`

### `boss-server` Operator PIN Auth
- `POST /api/v1/auth/pin`:
  - Request: `{"pin": "string"}`
  - Success (200 OK): `{"ok": true, "token": "string", "expires_at_epoch_ms": u64, "venue": "string"}`
  - Bad PIN (<5 failures) (401 Unauthorized): `{"ok": false, "error": "invalid_pin", "detail": "...", "remaining_attempts": u32}`
  - Locked out (>=5 failures) (429 Too Many Requests): `{"ok": false, "error": "rate_limited", "detail": "...", "retry_after_secs": u64}`
  - Header on 429: `Retry-After: <seconds>`

## Code Layout
- `shared/crates/beercade-shared/src/models.rs`: Shared telemetry and command structs
- `boss/src/state.rs`: `AppState`, `PinAttemptTracker`
- `boss/src/routes.rs`: Axum route handlers (`verify_operator_pin`, `poll_commands`, `ack_command`, etc.)
- `boss/src/plugins.rs`: Plugin event bus & thermal alert plugin
- `astro/daemon/src/ota.rs`: OTA download, staging, and SHA-256 verification
- `astro/daemon/src/main.rs`: `DaemonSharedState`, `load_credits`, `persist_credits`, `sample_telemetry`, `execute_cabinet_command`
- `astro/daemon/src/netplay.rs`: `sample_temps()`
- `astro/rootfs/etc/systemd/network/10-wired.network`: Networkd configuration (dynamic DHCP + link-local)
- `astro/rootfs/usr/local/bin/boss_static_net.sh`: Interface link setup script
