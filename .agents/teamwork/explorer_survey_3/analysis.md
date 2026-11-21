# Analysis: Testing Infrastructure & Command Dispatcher (Requirement R6)

## Executive Summary
This report provides an in-depth survey of the B.E.E.R.C.A.D.E. workspace test infrastructure, command dispatching plane, and testing strategies for requirements R1 through R5.

Key findings:
1. **Workspace Crates**: 5 workspace members defined in root `Cargo.toml`: `boss` (`boss-server`), `astro/daemon` (`astro-daemon`), `astro/tune` (`astro-tune`), `shared/crates/beercade-shared` (`beercade-shared`), and `shared/crates/beercade-hal` (`beercade-hal`).
2. **Current Test Health**: `cargo test` executes cleanly with **35 passed tests** across all 5 crates (0 failures, 1 compiler warning in `boss-server`). However, there is a total test void regarding the core business logic of R1-R5 and command execution:
   - `astro-daemon`: 10 netplay tests; **zero** tests for `main.rs`, credit persistence, or OTA staging.
   - `boss-server`: 4 tests (netplay relay, auth delegation, filename sanitize); **zero** tests for routes, state, command dispatch/ACK, or operator PIN authentication.
   - `beercade-shared`: 7 tests (URL format, bearer tokens, SHA-256); **zero** tests for telemetry validation or command models.
3. **Command Dispatch Plane**: A complete bidirectional command loop connects B.O.S.S. and A.S.T.R.O.:
   - Data contracts: `CabinetCommand`, `CommandRecord`, `CommandStatus`, `CommandAck` in `beercade-shared::models`.
   - B.O.S.S. Server: `AppState.enqueue_command` queues commands; `GET /api/v1/cabinet/:node_id/commands` drains and dispatches; `POST /api/v1/cabinet/:node_id/commands/ack` records ACK/failure outcomes and triggers SSE + plugin events.
   - A.S.T.R.O. Daemon: Background task polls commands every 2s, routes to `execute_cabinet_command` (Ping, AddCredits, StartSession, EndSession, Reboot, LaunchGame), and sends back `ack_command`.
   - **Zero tests currently exist** for command execution or ACK state transitions.
4. **Zero-Dependency Test Harnessing**:
   - `tempfile` is not in `Cargo.lock` or `Cargo.toml`. Instead, `beercade-hal` already implements a robust, zero-dependency RAII `FakeRoot` pattern using `std::env::temp_dir()`, which creates isolated directories and cleans them up on `Drop`.
   - Both `boss-server` and `astro-daemon` already include `tokio` (with full async runtime and `tokio::test`) and `reqwest`.
   - `boss-server` includes `axum` and can bind in-memory listeners (`tokio::net::TcpListener::bind("127.0.0.1:0")`) for full HTTP roundtrip integration tests without external test runners or OS image builds.

---

## 1. Workspace & Test Layout

### 1.1 Workspace Members
The workspace is configured in `Cargo.toml` with `resolver = "2"`:
```toml
[workspace]
members = [
    "boss",
    "astro/daemon",
    "astro/tune",
    "shared/crates/beercade-shared",
    "shared/crates/beercade-hal",
]
```

Crate breakdown:
| Member Directory | Package Name | Crate Type | Primary Function |
|---|---|---|---|
| `boss` | `boss-server` | Binary (`src/main.rs`) | Central Operations & Supervisory System daemon (Axum REST, WS, SSE, plugin bus, edge buffer) |
| `astro/daemon` | `astro-daemon` | Binary (`src/main.rs`) | Station node supervisor daemon (telemetry loop, command execution, credits, rollback netplay, OTA) |
| `astro/tune` | `astro-tune` | Binary (`src/main.rs`) | Boot-time CPU governor, PM QoS, and CPU partitioning utility |
| `shared/crates/beercade-shared` | `beercade-shared` | Library (`src/lib.rs`) | Shared IPC data models (`CabinetCommand`, `NodeTelemetry`, `CreditCounters`), auth tokens, URL helpers |
| `shared/crates/beercade-hal` | `beercade-hal` | Library (`src/lib.rs`) | Hardware abstraction layer (SoC detection, thermals, A/B rootfs slot detection and bootloader commit) |

### 1.2 Existing Automated Tests Inventory
A baseline run of `cargo test` exercises 35 unit tests:

1. **`astro-daemon`** (10 tests, finished in ~1.25s):
   - `netplay::tests::drift_controller_converges_and_is_jury_stable`
   - `netplay::tests::crc32_incremental_matches_oneshot`
   - `netplay::tests::crc32_known_vectors`
   - `netplay::tests::ring_buffer_wraparound`
   - `netplay::tests::packet_roundtrip_all_variants`
   - `netplay::tests::synthetic_core_is_deterministic_and_order_sensitive`
   - `netplay::tests::rollback_with_history_gap_escalates`
   - `netplay::tests::late_input_below_resync_frame_is_ignored`
   - `netplay::tests::hard_resync_via_state_transfer`
   - `netplay::tests::loopback_rollback_convergence`
   *Deficiency*: Covers exclusively the rollback networking math in `netplay.rs`. No tests for `main.rs`, credit persistence, command dispatch, or `ota.rs`.

2. **`astro-tune`** (5 tests, finished in ~0.03s):
   - `tests::no_plan_without_rt`
   - `tests::format_lists`
   - `tests::parse_lists`
   - `tests::plan_for_eight_cpus`
   - `tests::online_from_root`

3. **`beercade-hal`** (9 tests, finished in ~0.07s):
   - `tests::compatible_strings_map_to_platforms`
   - `tests::extlinux_rewrite_only_touches_root_argument`
   - `tests::realtime_cpus_never_name_cores_that_do_not_exist`
   - `tests::missing_sensor_is_an_error_not_a_fake_reading`
   - `tests::unknown_environment_defaults_to_slot_a`
   - `tests::substrings_do_not_fake_a_slot_match`
   - `tests::x86_thermals_pick_matching_hwmon_drivers`
   - `tests::arm_thermals_read_zones_and_tolerate_missing_gpu`
   - `tests::active_slot_from_cmdline_then_mounts`

4. **`beercade-shared`** (7 tests, finished in ~0.00s):
   - `auth::tests::test_bearer_helpers`
   - `auth::tests::test_constant_time_eq`
   - `auth::tests::test_validate_token`
   - `ipc::tests::test_http_url_formatting`
   - `ipc::tests::test_ota_download_url`
   - `crypto::tests::test_sha256`
   - `ipc::tests::test_ws_url_formatting`
   *Deficiency*: Zero tests in `models.rs` for `NodeTelemetry::validate()`, `CabinetCommand`, `CommandAck`, or `CreditCounters`.

5. **`boss-server`** (4 tests, finished in ~0.02s):
   - `auth::tests::test_constant_time_eq_delegation`
   - `ota_sync::tests::test_sanitize_filename`
   - `netplay_relay::tests::relay_discovery_and_forwarding`
   - `netplay_relay::tests::relay_health_report_ingest`
   *Deficiency*: Zero tests for operator PIN authentication (`verify_operator_pin`), rate limiting, command queueing/draining/ACKing, or telemetry ingestion routes.

### 1.3 Compiler Warnings & Build Health
During compilation, one warning occurs:
```
warning: method `id` is never used
   --> boss\src\plugins.rs:103:8
102 | pub trait BossPlugin: Send + Sync {
103 |     fn id(&self) -> &str;
```
Running `cargo check --offline` succeeds cleanly in ~41s, verifying that all crates and dependencies are cached locally in the cargo vendor cache and can build/test in offline environments.

---

## 2. Command Dispatcher Architecture & ACK Behavior

### 2.1 Component Mapping
The command subsystem spans three crates:

| Layer | File & Location | Key Symbols | Responsibility |
|---|---|---|---|
| **Data Contract** | `shared/crates/beercade-shared/src/models.rs:145-205` | `CabinetCommand`, `CommandRecord`, `CommandStatus`, `CommandAck` | Unified serialization contract shared between BOSS and ASTRO |
| **Command Store & Queue** | `boss/src/state.rs:149-151, 255-283` | `AppState::commands`, `AppState::pending_commands`, `AppState::enqueue_command` | Thread-safe FIFO queueing (`VecDeque<String>`), SSE broadcasting |
| **Command HTTP Routes** | `boss/src/routes.rs:460-575, 577-650` | `poll_commands`, `ack_command`, `command_status`, `credits_add`, `cabinet_ping`, `reboot_cabinet`, `cabinet_launch_game` | Ingestion from admin/kiosk, drainage by node, status & outcome tracking |
| **Node Command Loop** | `astro/daemon/src/main.rs:821-885` | Background `tokio::spawn` loop | Polls `GET /api/v1/cabinet/:node_id/commands` every 2s |
| **Command Execution** | `astro/daemon/src/main.rs:336-571` | `execute_cabinet_command` | Executes command action, mutates daemon state, launches games, returns `(bool, Option<Value>)` |
| **Node ACK Sender** | `astro/daemon/src/main.rs:574-598` | `ack_command` | Posts outcome back to `POST /api/v1/cabinet/:node_id/commands/ack` |

### 2.2 End-to-End Command Lifecycle Trace
```
 ┌─────────────┐                      ┌─────────────┐                      ┌──────────────┐
 │ BOSS Kiosk/ │                      │ B.O.S.S.    │                      │ A.S.T.R.O.   │
 │ Supervisor  │                      │ Server      │                      │ Node Daemon  │
 └──────┬──────┘                      └──────┬──────┘                      └──────┬───────┘
        │                                    │                                    │
        │ 1. POST /api/v1/credits/add        │                                    │
        │───────────────────────────────────>│                                    │
        │    (or cabinet_ping, etc.)         │                                    │
        │                                    │ State: Queued                      │
        │                                    │ Append ID to pending FIFO          │
        │ <─── 200 OK { command_id: "..." } ─│                                    │
        │                                    │                                    │
        │                                    │ 2. GET /cabinet/:node_id/commands  │
        │                                    │<───────────────────────────────────│ (Poll loop every 2s)
        │                                    │                                    │
        │                                    │ State: Dispatched                  │
        │                                    │ Set dispatched_at_epoch_ms         │
        │                                    │ Drain pending IDs                  │
        │                                    │─── 200 OK { commands: [...] } ────>│
        │                                    │                                    │
        │                                    │                                    │ 3. execute_cabinet_command()
        │                                    │                                    │    - Ping -> echo seq
        │                                    │                                    │    - AddCredits -> add_credits()
        │                                    │                                    │      and persist_credits()
        │                                    │                                    │    - StartSession -> Managed mode
        │                                    │                                    │    - EndSession -> Locked mode
        │                                    │                                    │    - LaunchGame -> exec RetroArch
        │                                    │                                    │
        │                                    │ 4. POST /cabinet/:node_id/.../ack  │
        │                                    │<───────────────────────────────────│
        │                                    │    { command_id, success, detail } │
        │                                    │                                    │
        │                                    │ If node_id mismatch -> 403         │
        │                                    │ If unknown cmd_id -> 404           │
        │                                    │ State: Acknowledged or Failed      │
        │                                    │ Set completed_at_epoch_ms & result │
        │                                    │ Publish SSE & PluginEvent          │
        │                                    │─── 200 OK ────────────────────────>│
        │                                    │                                    │
        │ 5. GET /api/v1/commands/:id        │                                    │
        │───────────────────────────────────>│                                    │
        │ <── 200 OK { status: "acknowledged", result: { ... } }                  │
        v                                    v                                    v
```

### 2.3 Command Execution Details in ASTRO Daemon
Examining `astro/daemon/src/main.rs:336-571`:
- **`Ping { seq }`**:
  - Immediately logs and returns `(true, Some(json!({ "seq": seq })))`.
- **`AddCredits { credits }`**:
  - Locks `state_arc.write().await`, invokes `st.add_credits(*credits)`, which increments `credits_available` and `credits_inserted_total`, sets `last_coin_epoch_ms`, calls `persist_credits()`.
  - Returns `(true, Some(json!({ "credits_added": credits, "credits_available": available })))`.
- **`StartSession { session_id, duration_secs }`**:
  - Mutates `st.mode = StationMode::Managed { session_id, time_remaining_sec: *duration_secs }`.
  - Returns `(true, Some(json!({ "session_id": session_id, "duration_secs": duration_secs })))`.
- **`EndSession`**:
  - Mutates `st.mode = StationMode::Locked { reason: "Session ended by supervisor" }`.
  - Calls `terminate_running_game().await` and `resume_frontend().await`.
  - Returns `(true, Some(json!({ "status": "session_ended" })))`.
- **`Reboot { delay_secs }`**:
  - Spawns background task sleeping `delay_secs` then executes `systemctl reboot` (on Linux).
  - Returns `(true, Some(json!({ "delay_secs": delay })))`.
- **`LaunchGame { core, rom, netplay }`**:
  - Checks if `core` or `rom` is empty; if so, returns `(false, Some(json!({ "error": "core and rom must not be empty" })))`.
  - Resolves core and rom paths, pauses attract frontend, records `st.active_game`, spawns launcher script.
  - If process fails to spawn: logs fatal error, reports node error to BOSS, returns `(false, Some(json!({ "error": err_msg })))`.
  - If spawn succeeds: returns `(true, Some(json!({ "status": "game_spawned", ... })))`.

### 2.4 Command ACK Edge Cases in B.O.S.S.
Examining `boss/src/routes.rs:619-650`:
1. **Node Authorization**: The endpoint checks `record.node_id != node_id`. If a different node tries to ACK a command, it returns `StatusCode::FORBIDDEN` (403).
2. **Missing Command**: If `command_id` is not in `state.commands`, it returns `StatusCode::NOT_FOUND` (404).
3. **Success vs. Failure**:
   - `ack.success == true` sets status to `CommandStatus::Acknowledged`.
   - `ack.success == false` sets status to `CommandStatus::Failed`.
4. **Timestamps & Payloads**:
   - `record.completed_at_epoch_ms` is stamped with current wall-clock epoch ms.
   - `record.result` stores `ack.detail` (JSON object returned by execution).
5. **Event Propagation**:
   - Broadcasts the updated record over SSE (`SseChannel::Commands`).
   - Dispatches `PluginEvent::CommandUpdated(snapshot)` to the plugin bus.

### 2.5 Required Automated Tests for Command Plane
1. **ASTRO `execute_cabinet_command` Unit Tests**:
   - Test `Ping`: verify `success == true`, echo payload matches `seq`.
   - Test `AddCredits`: verify state reflects updated balances, persistence called, response contains correct balances.
   - Test `StartSession`: verify `StationMode::Managed` set with session id and time.
   - Test `EndSession`: verify `StationMode::Locked` set.
   - Test `LaunchGame`: verify empty core/rom returns `(false, ...)`.
2. **B.O.S.S. Command Queueing & Polling Tests**:
   - Enqueue multiple commands for node `ORION-01` and verify FIFO order in `pending_commands`.
   - Drain via `poll_commands`: verify status transitions from `Queued` to `Dispatched`, `dispatched_at_epoch_ms` is set, and pending queue is empty.
   - Second poll returns empty list.
3. **B.O.S.S. ACK Handling Tests**:
   - ACK with `success: true` -> record status transitions to `Acknowledged`, detail stored in `result`.
   - ACK with `success: false` -> record status transitions to `Failed`, error detail stored in `result`.
   - ACK with invalid `node_id` -> returns `403 Forbidden`, record remains untouched.
   - ACK with non-existent `command_id` -> returns `404 Not Found`.

---

## 3. Test Suites for Requirements R1–R5

### 3.1 Requirement R1: OTA Staging Rejection with Missing/Bad SHA256 Header

#### Current Implementation Analysis
In `astro/daemon/src/ota.rs:70-82`:
```rust
let expected_sha256 = response
    .headers()
    .get("x-ota-sha256")
    .and_then(|v| v.to_str().ok())
    .map(|s| s.trim().to_lowercase());

if let Some(expected) = expected_sha256 {
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        let _ = tokio::fs::remove_file(STAGING_PATH).await;
        return Err(format!("SHA-256 mismatch: expected {}, got {}", expected, actual));
    }
}
```
**Vulnerability**: If `X-OTA-SHA256` header is omitted or empty, `expected_sha256` is `None`! The daemon skips all checksum verification, stages the payload, and flashes it to disk!

#### Concrete Test Cases Required
1. **`test_ota_staging_rejects_missing_header`**:
   - Mock response / stream with valid bytes, but no `X-OTA-SHA256` header.
   - Assertion: returns `Err`, error text explicitly notes missing header, staging file is unlinked / deleted from disk.
2. **`test_ota_staging_rejects_empty_header`**:
   - Mock response with `X-OTA-SHA256: ""` or `"   "`.
   - Assertion: returns `Err`, staging file deleted.
3. **`test_ota_staging_rejects_checksum_mismatch`**:
   - Mock response with payload `b"corrupt payload"` and `X-OTA-SHA256: "0000000000000000000000000000000000000000000000000000000000000000"`.
   - Assertion: returns `Err` containing `"SHA-256 mismatch"`, staging file removed.
4. **`test_ota_staging_accepts_valid_checksum`**:
   - Mock response with payload and matching SHA-256 hex header.
   - Assertion: returns `Ok(total_bytes)`, staging file exists with exact byte count.

#### Test Design & Mock Strategy
- Extract the staging and verification logic into an isolatable function (e.g. `stage_and_verify_payload`) that accepts the destination path, payload stream, content length, and header value.
- Use `FakeRoot` (temporary directory) to ensure tests never write to `/data/beercade/sync/staging.sqsh` or execute `dd`/`reboot`.

---

### 3.2 Requirement R2: Credit Persistence Atomic Write, Truncated File Recovery, Rapid Counter Updates

#### Current Implementation Analysis
In `astro/daemon/src/main.rs:40-56`:
```rust
pub fn load_credits(path: &Path) -> CreditCounters {
    if let Ok(data) = std::fs::read_to_string(path) {
        if let Ok(c) = serde_json::from_str::<CreditCounters>(&data) {
            return c;
        }
    }
    CreditCounters::default()
}

pub fn persist_credits(&self) {
    if let Some(parent) = self.credits_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&self.credits) {
        let _ = std::fs::write(&self.credits_path, json);
    }
}
```
**Deficiencies**:
1. `std::fs::write` directly writes to `credits.json` in place without staging. A power cut or panic mid-write results in a truncated 0-byte or incomplete JSON file.
2. No `sync_all` is called on the file descriptor to flush OS write buffers to non-volatile storage.
3. `load_credits` silently falls back to `CreditCounters::default()`, erasing all coins and accounting records if `credits.json` is corrupted.

#### Concrete Test Cases Required
1. **`test_persist_credits_atomic_write`**:
   - Call `persist_credits`.
   - Verify `credits.json` exists, is valid JSON, and no stale `.tmp` files linger.
2. **`test_load_credits_recovers_from_truncated_file`**:
   - Write partial JSON `b"{\"credits_available\": 12, \"credits_inserted_total\":"` into `credits.json`.
   - Create a valid backup in `credits.json.bak` with 12 credits.
   - Call `load_credits`.
   - Assertion: No panic; recovers 12 credits from `.bak`; heals `credits.json`.
3. **`test_load_credits_handles_missing_file`**:
   - Target path does not exist.
   - Assertion: Returns `CreditCounters::default()` without panic.
4. **`test_load_credits_corrupt_file_and_missing_backup`**:
   - Write completely random garbage bytes (`b"\xDE\xAD\xBE\xEF"`) into `credits.json`, no backup file.
   - Assertion: Returns `CreditCounters::default()` gracefully without crashing or panicking.
5. **`test_rapid_credit_counter_updates`**:
   - Concurrently or rapidly in a loop (e.g. 100 iterations), perform `add_credits(2)` and `consume_credit()`.
   - Assertion: After 100 cycles, `credits_available` is exactly 100, `credits_dispensed_today` is exactly 100, `credits.json` on disk is well-formed JSON, and reading it back yields identical values.

---

### 3.3 Requirement R4: Clean Telemetry Contract for Thermals

#### Current Implementation Analysis
- In `beercade-shared/src/models.rs:80-81, 109-114`:
  ```rust
  pub cpu_temp_c: f32,
  pub gpu_temp_c: f32,
  ...
  if !(-20.0..=120.0).contains(&self.cpu_temp_c) {
      return Err("cpu_temp_c out of plausible range");
  }
  ```
- In `astro/daemon/src/main.rs:132-138`:
  ```rust
  let (mut cpu_temp, mut gpu_temp) = netplay::sample_temps();
  if cpu_temp.is_nan() || !(-20.0..=120.0).contains(&cpu_temp) {
      cpu_temp = 42.0;
  }
  ```
- In `boss/src/routes.rs:76-79`:
  ```rust
  if let Err(reason) = payload.validate() {
      tracing::warn!(node_id = %payload.node_id, %reason, "rejected invalid telemetry");
      return StatusCode::UNPROCESSABLE_ENTITY;
  }
  ```
**Deficiency**: Missing hardware sensors (common on x86_64 PCs without supported hwmon drivers, or Raspberry Pi setups without GPU sensors) are either forced to fake 42.0°C values, or if represented as NaN, fail `validate()` and cause BOSS to reject the heartbeat with HTTP 422 Unprocessable Entity.

#### Concrete Test Cases Required
1. **`test_telemetry_validation_accepts_none_thermals`**:
   - Construct `NodeTelemetry` with `cpu_temp_c: None` and `gpu_temp_c: None`.
   - Call `validate()`.
   - Assertion: Returns `Ok(())`.
2. **`test_telemetry_validation_accepts_partial_thermals`**:
   - Construct `NodeTelemetry` with `cpu_temp_c: Some(52.5)` and `gpu_temp_c: None`.
   - Assertion: Returns `Ok(())`.
3. **`test_telemetry_validation_rejects_out_of_range_thermals`**:
   - Construct `NodeTelemetry` with `cpu_temp_c: Some(150.0)`.
   - Assertion: Returns `Err("cpu_temp_c out of plausible range")`.
4. **`test_telemetry_json_serde_backwards_compatibility`**:
   - Deserialize older telemetry JSON containing numeric temperatures: `{ "node_id": "ORION-01", "cpu_temp_c": 55.0, ... }` -> parsed as `Some(55.0)`.
   - Deserialize telemetry JSON with missing or `null` temperatures: `{ "node_id": "ORION-01", "cpu_temp_c": null, ... }` -> parsed as `None`.
5. **`test_boss_heartbeat_ingest_with_none_thermals`**:
   - In `boss-server`, post heartbeat with `cpu_temp_c: None`.
   - Assertion: Route returns `StatusCode::OK` / `StatusCode::NO_CONTENT` (not 422).
   - Thermal alert plugin does not emit false alarms when temperatures are `None`.

---

### 3.4 Requirement R5: B.O.S.S. Operator PIN Brute-Force Rate Limiting

#### Current Implementation Analysis
In `boss/src/routes.rs:142-172`:
```rust
pub async fn verify_operator_pin(
    State(state): State<SharedState>,
    Json(req): Json<PinUnlockRequest>,
) -> Response {
    let candidate = req.pin.trim();
    if constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes()) {
        let (token, expires_at_epoch_ms) = state.mint_operator_session().await;
        (StatusCode::OK, Json(json!({ "ok": true, "token": token, ... }))).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "ok": false, "error": "invalid_pin" }))).into_response()
    }
}
```
**Vulnerability**: No rate limiting or lockout tracking. An attacker on the local network can brute force 1,000,000 PIN combinations unrestricted.

#### Concrete Test Cases Required
1. **`test_operator_pin_unlock_success`**:
   - Submit correct PIN with 0 prior failures.
   - Assertion: HTTP 200 OK, valid session token returned.
2. **`test_operator_pin_incremental_failures`**:
   - Submit incorrect PIN 4 times.
   - Assertion: Each attempt returns HTTP 401 Unauthorized, indicates failure count or remaining attempts.
3. **`test_operator_pin_lockout_trigger`**:
   - Submit 5th consecutive incorrect PIN.
   - Assertion: Returns HTTP 429 Too Many Requests (or lockout status JSON).
4. **`test_operator_pin_lockout_enforcement`**:
   - Immediately submit the CORRECT PIN while lockout window is active.
   - Assertion: Returns HTTP 429 Too Many Requests / rejected; no session token minted.
5. **`test_operator_pin_success_resets_failure_counter`**:
   - Submit 3 incorrect PINs (failure count = 3).
   - Submit CORRECT PIN (success).
   - Submit 1 incorrect PIN.
   - Assertion: Failure count is now 1 (not 4); user is not prematurely locked out.
6. **`test_operator_pin_lockout_expiry`**:
   - After simulated expiration of the lockout duration, submitting the correct PIN succeeds with HTTP 200.

---

## 4. Test Harnesses & Tooling Infrastructure

### 4.1 Dependency Audit & Constraints
An audit of `Cargo.lock` and workspace `Cargo.toml` files reveals:
- **`tempfile`**: NOT in the dependency graph. We must NOT depend on adding `tempfile` to `[dependencies]`.
- **`tokio`**: Present in `boss-server` and `astro-daemon` with feature `full` (includes `tokio::test`, `tokio::net::TcpListener`, `tokio::sync::RwLock`, channels, and async timers).
- **`reqwest`**: Present in `boss-server` and `astro-daemon` (`0.12`, `json`, `rustls-tls`). Can execute HTTP client requests in tests.
- **`axum`**: Present in `boss-server` (`0.7`, `ws`). Can be bound to ephemeral ports (`127.0.0.1:0`) in tests.
- **`sha2` & `serde_json`**: Available across all crates.

### 4.2 The RAII `FakeRoot` Harness Pattern
To test file persistence (credits, OTA payloads) without touching production paths (`/data/beercade`) or needing third-party tempdir crates, we replicate the existing pattern from `beercade-hal/src/tests.rs`:

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct FakeRoot(pub PathBuf);

impl FakeRoot {
    pub fn new(prefix: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("beercade-test-{prefix}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for FakeRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
```
*Benefits*:
- 100% standard library (no `tempfile` dependency required).
- Unique per test and process ID (zero test collision when running `cargo test -- --test-threads=...`).
- Self-cleaning upon drop even on assertion panic.

### 4.3 In-Memory Axum Test Server Harness Pattern
To test B.O.S.S. routes (`POST /api/v1/auth/pin`, `GET /api/v1/cabinet/:node_id/commands`, `POST .../commands/ack`, `POST .../heartbeat`) with real HTTP semantics without external scripts:

```rust
pub async fn spawn_test_boss(config: Config) -> (String, Arc<AppState>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(AppState::new(config));
    let app = crate::router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, state, handle)
}
```
*Benefits*:
- Directly tests Axum routing, serialization, headers, status codes, and state mutations.
- Uses OS-allocated ephemeral port (`127.0.0.1:0`), avoiding port conflicts.
- Terminates cleanly when the test process finishes.

---

## 5. Comprehensive Recommendation & Implementation Plan for R6

To satisfy R6 and ensure all acceptance criteria pass cleanly:

### Phase 1: Shared Models & Thermal Contract (R4)
- Update `beercade-shared/src/models.rs`:
  - `cpu_temp_c: Option<f32>` and `gpu_temp_c: Option<f32>`.
  - Add `#[serde(default)]`.
  - Update `validate()` to accept `None` and only check range if `Some`.
- Add unit tests in `beercade-shared/src/models.rs`:
  - Validate with `None`, `Some`, and invalid range.
  - Serde roundtrips for `CabinetCommand`, `CommandRecord`, and `CommandAck`.

### Phase 2: Credit Persistence & Recovery (R2)
- In `astro/daemon/src/main.rs`:
  - Refactor `persist_credits`: write to `.tmp`, flush with `sync_all()`, atomic `rename()`, maintain `.bak`.
  - Refactor `load_credits`: if main file is missing or corrupted, attempt recovery from `.bak`.
- Add unit tests in `astro/daemon/src/main.rs` (using `FakeRoot`):
  - Normal persistence.
  - Recovery from truncated file via backup.
  - Recovery from corrupted file without panic.
  - Rapid sequential/concurrent updates.

### Phase 3: Mandatory OTA Header Verification (R1)
- In `astro/daemon/src/ota.rs`:
  - Reject update if `x-ota-sha256` header is missing, empty, or checksum mismatches.
  - Cleanly delete staging file on any rejection.
- Add unit tests in `astro/daemon/src/ota.rs` (or test module):
  - Test missing header rejection & file cleanup.
  - Test empty header rejection & file cleanup.
  - Test checksum mismatch rejection & file cleanup.
  - Test matching checksum acceptance.

### Phase 4: Operator PIN Rate Limiting & Lockout (R5)
- In `boss/src/state.rs`:
  - Add `pin_failures: RwLock<u32>` and `pin_locked_until: RwLock<u64>`.
  - Add helper methods `record_pin_failure()`, `record_pin_success()`, `check_pin_lockout()`.
- In `boss/src/routes.rs`:
  - Guard `verify_operator_pin` with lockout check -> return HTTP 429.
  - Increment failure on invalid PIN; lock if threshold reached.
- Add unit tests in `boss/src/routes.rs`:
  - Success on first try.
  - Incremental failures.
  - 5th failure lockout (HTTP 429).
  - Rejection during lockout.
  - Counter reset on success.

### Phase 5: Command Dispatcher Execution & ACK Behavior (R6)
- In `astro/daemon/src/main.rs`:
  - Add unit tests for `execute_cabinet_command`: Ping, AddCredits, StartSession, EndSession, LaunchGame validation.
- In `boss/src/routes.rs`:
  - Add unit tests for command queuing (`enqueue_command`), draining (`poll_commands`), and ACK status updates (`ack_command` for success, failure, wrong node, not found).

---

## 6. Verification Method

Once implemented, the test suite can be verified independently via:
```powershell
# Run the complete automated test suite across all workspace crates
cargo test

# Verify specific crate test suites
cargo test -p beercade-shared
cargo test -p astro-daemon
cargo test -p boss-server
cargo test -p beercade-hal
```
*Expected Result*: All tests pass cleanly (target ≥60 total automated tests across the workspace), with zero segmentation faults, zero panics, and zero target OS image build steps executed.
