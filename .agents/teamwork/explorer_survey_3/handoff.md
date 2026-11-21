# Handoff Report: Testing Infrastructure & Command Dispatcher Survey (explorer_survey_3)

## 1. Observation

### Workspace Layout & Crates
- `Cargo.toml` lines 1-8:
  ```toml
  [workspace]
  members = [
      "boss",
      "astro/daemon",
      "astro/tune",
      "shared/crates/beercade-shared",
      "shared/crates/beercade-hal",
  ]
  resolver = "2"
  ```
- Binary packages: `boss-server` (`boss/src/main.rs`), `astro-daemon` (`astro/daemon/src/main.rs`), `astro-tune` (`astro/tune/src/main.rs`).
- Library packages: `beercade-shared` (`shared/crates/beercade-shared/src/lib.rs`), `beercade-hal` (`shared/crates/beercade-hal/src/lib.rs`).

### Current Test Execution & Test Suites
- Command `cargo test` was executed on Windows PowerShell.
- Result: Exited with code 0.
- Summary of 35 tests:
  - `astro-daemon`: 10 passed (`netplay::tests::*`). Zero tests in `main.rs` or `ota.rs`.
  - `astro-tune`: 5 passed (`tests::*`).
  - `beercade-hal`: 9 passed (`tests::*` in `shared/crates/beercade-hal/src/tests.rs`).
  - `beercade-shared`: 7 passed (`auth::tests::*`, `ipc::tests::*`, `crypto::tests::*`). Zero tests in `models.rs`.
  - `boss-server`: 4 passed (`auth::tests::test_constant_time_eq_delegation`, `ota_sync::tests::test_sanitize_filename`, `netplay_relay::tests::relay_discovery_and_forwarding`, `netplay_relay::tests::relay_health_report_ingest`). Zero tests in `routes.rs`, `state.rs`, or `plugins.rs`.
- One warning during build: `warning: method 'id' is never used` in `boss/src/plugins.rs:103:8`.
- Command `cargo check --offline` exited with code 0 in ~41s, verifying all workspace dependencies are cached and offline-capable.

### Command Dispatcher Implementation
- **Data Model** (`shared/crates/beercade-shared/src/models.rs:146-204`):
  - `CabinetCommand`: variants `Reboot { delay_secs }`, `LaunchGame { core, rom, netplay }`, `AddCredits { credits }`, `Ping { seq }`, `StartSession { session_id, duration_secs }`, `EndSession`.
  - `CommandRecord`: `id`, `node_id`, `command`, `status` (`Queued`, `Dispatched`, `Acknowledged`, `Failed`), `issued_at_epoch_ms`, `dispatched_at_epoch_ms`, `completed_at_epoch_ms`, `result`.
  - `CommandAck`: `command_id`, `success`, `detail`.
- **B.O.S.S. Server Side**:
  - `boss/src/state.rs:255-283`: `AppState::enqueue_command` inserts `CommandRecord` into `commands` map and queues `id` into `pending_commands` FIFO (`VecDeque<String>`).
  - `boss/src/routes.rs:594-617`: `poll_commands` drains `pending_commands`, updates status to `CommandStatus::Dispatched`, sets `dispatched_at_epoch_ms`, returns `{ "node_id": node_id, "commands": dispatched }`.
  - `boss/src/routes.rs:619-650`: `ack_command` receives `CommandAck`. If `record.node_id != node_id` returns `403 FORBIDDEN`. If not found returns `404 NOT_FOUND`. If found: updates status to `CommandStatus::Acknowledged` (if `ack.success`) or `CommandStatus::Failed` (if `!ack.success`), sets `completed_at_epoch_ms = now`, stores `record.result = ack.detail`, publishes SSE event, and dispatches `PluginEvent::CommandUpdated`.
- **A.S.T.R.O. Daemon Side**:
  - `astro/daemon/src/main.rs:821-880`: Polling loop calls `GET /api/v1/cabinet/:node_id/commands` every 2s.
  - `astro/daemon/src/main.rs:336-571`: `execute_cabinet_command(cmd, state_arc, client)` executes Ping (echo seq), AddCredits (updates counters and calls `persist_credits()`), StartSession (`StationMode::Managed`), EndSession (`StationMode::Locked`), Reboot (spawns delayed `systemctl reboot`), LaunchGame (validates non-empty core/rom, pauses frontend, spawns launcher).
  - `astro/daemon/src/main.rs:574-598`: `ack_command` posts `{ "command_id": cmd_id, "success": success, "detail": detail }` back to `POST /api/v1/cabinet/:node_id/commands/ack`.

### Test Harnesses & Tooling
- `Cargo.lock` does NOT contain `tempfile`.
- `shared/crates/beercade-hal/src/tests.rs:5-32` already implements a zero-dependency RAII `FakeRoot` using `std::env::temp_dir()`, atomic counters, and `Drop` cleanup.
- `boss/src/netplay_relay.rs:491-497` demonstrates in-memory HTTP server binding with `tokio::net::TcpListener::bind("127.0.0.1:0")` + `axum::serve`.
- Both `boss-server` and `astro-daemon` already include `tokio = { version = "1.38", features = ["full"] }` and `reqwest`.

---

## 2. Logic Chain

1. **Test Infrastructure Void**:
   - *From Observation*: All 35 existing tests in the workspace test peripheral functions (e.g., netplay drift/CRC, CPU list parsing, HAL sysfs parsing, and netplay relay UDP).
   - *Inference*: Core business operations (OTA verification, credit persistence atomic write and recovery, thermal telemetry contract, operator PIN rate limiting, and command dispatch/ACK) have 0% automated test coverage.
2. **Command Dispatcher Verification**:
   - *From Observation*: The command dispatch plane involves a state machine with 4 states (`Queued` -> `Dispatched` -> `Acknowledged` or `Failed`), with authorization checks (`record.node_id != node_id -> 403`), and state transitions in both B.O.S.S. (`routes.rs`, `state.rs`) and A.S.T.R.O. (`execute_cabinet_command`, `ack_command`).
   - *Inference*: Automated testing requires verifying (a) `execute_cabinet_command` outcome and state updates per variant in ASTRO, (b) FIFO queueing and drain-dispatch in BOSS, and (c) ACK state transitions (success, failure, 403 node mismatch, 404 not found) in BOSS.
3. **Requirement R1 Test Strategy**:
   - *From Observation*: `astro/daemon/src/ota.rs:76` checks `if let Some(expected) = expected_sha256`. If the header is missing or empty, it bypasses verification.
   - *Inference*: Tests must supply mock HTTP responses with (1) missing header, (2) empty header, (3) mismatched hash, and (4) valid hash, verifying that cases 1-3 return an explicit error and remove staging files.
4. **Requirement R2 Test Strategy**:
   - *From Observation*: `astro/daemon/src/main.rs:49` writes directly via `std::fs::write(&self.credits_path, json)` without sync or atomic rename. `load_credits` defaults to 0 on any read/parse error.
   - *Inference*: Tests must simulate truncated JSON and corrupted bytes, verifying that `load_credits` recovers from `.bak` without panicking, and that rapid counter updates maintain file integrity.
5. **Requirement R4 Test Strategy**:
   - *From Observation*: `beercade-shared/src/models.rs:109-113` rejects temperatures outside `-20.0..=120.0`, forcing daemon to fake 42.0°C.
   - *Inference*: Tests must verify that `NodeTelemetry::validate()` accepts `cpu_temp_c: None` and `gpu_temp_c: None`, that JSON deserialization handles null/missing fields, and that BOSS `ingest_telemetry` accepts heartbeats with `None` thermals.
6. **Requirement R5 Test Strategy**:
   - *From Observation*: `boss/src/routes.rs:142` has no attempt counter or lockout.
   - *Inference*: Tests must verify that 5 consecutive failed attempts trigger lockout (HTTP 429), that subsequent attempts are blocked during lockout, and that a successful attempt resets the failure counter.
7. **Execution Without Image Builds**:
   - *From Observation*: The `FakeRoot` pattern allows isolated filesystem testing; `TcpListener::bind("127.0.0.1:0")` + `axum::serve` allows full HTTP integration testing; all dependencies (`tokio`, `axum`, `reqwest`) are already present.
   - *Inference*: 100% of the required automated tests can run as fast, pure Rust tests under `cargo test` in user space without requiring any disk image builds (`make image-x86`, `mksquashfs`, Docker) or external dependencies.

---

## 3. Caveats
- No image builds or OS disk packaging jobs were executed, conforming strictly to user constraints.
- `tempfile` crate is omitted to avoid introducing new external dependencies; the existing in-tree RAII `FakeRoot` pattern using `std::env::temp_dir()` must be used.
- Binary crates (`astro-daemon`, `boss-server`) do not have `lib.rs` targets; their tests must be in-crate unit tests (or `#[cfg(test)] mod tests` in the binaries) rather than standalone files in root `tests/`.

---

## 4. Conclusion
The testing infrastructure in B.E.E.R.C.A.D.E. is healthy (35/35 passing, clean offline compile), but lacks test coverage for requirements R1–R5 and the command dispatch plane.
A zero-dependency, in-memory testing approach using `tokio::test`, `FakeRoot` (for crash and OTA file simulation), and `axum::serve` (for in-memory HTTP integration) fulfills all acceptance criteria without external dependencies or OS disk builds.

---

## 5. Verification Method

To independently reproduce observations and verify the test infrastructure:

1. **Run Current Workspace Tests**:
   ```powershell
   cargo test
   ```
   *Expected Result*: 35 tests pass across 5 crates; 1 warning in `boss/src/plugins.rs`.

2. **Verify Offline Capability**:
   ```powershell
   cargo check --offline
   ```
   *Expected Result*: Exits 0, confirming dependencies are vendored/cached.

3. **Verify Planned New Tests (Post-Implementation)**:
   ```powershell
   # beercade-shared: thermal validation with Option<f32>, command serde
   cargo test -p beercade-shared

   # astro-daemon: atomic credits, corrupted recovery, OTA header checks, command execution
   cargo test -p astro-daemon

   # boss-server: PIN lockout & rate limit, command queuing, dispatch & ACK
   cargo test -p boss-server
   ```
   *Expected Result*: All new unit and integration tests pass cleanly with 0 failures.
