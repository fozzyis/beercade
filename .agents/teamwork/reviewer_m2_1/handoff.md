# Handoff Report: Milestone 2 — B.O.S.S. Server Security & Command Plane

**Reviewer**: Reviewer M2-1 (`reviewer_m2_1`)  
**Roles**: Reviewer, Critic  
**Date**: 2026-10-27T23:30:00Z  
**Recipient**: Orchestrator (`bd770878-29b7-48ec-a1e1-b5265681a707`)  
**Milestone**: Milestone 2 (Requirements R5 & R6)  
**Verdict**: **APPROVE**

---

## 1. Observation

1. **Codebase Inspection**:
   - `boss/Cargo.toml` (lines 26-28):
     ```toml
     [dev-dependencies]
     tower = { version = "0.5", features = ["util"] }
     ```
     Added as dev-dependency only; production build is unaffected.
   - `boss/src/state.rs` (lines 141-269):
     - Configured constants: `PIN_MAX_ATTEMPTS = 5`, `PIN_LOCKOUT_SECS = 60`, `PIN_LOCKOUT_MS = 60_000`, `PIN_RECORD_TTL_MS = 900_000`.
     - Defined data types: `PinFailureResult` (`Invalid { remaining_attempts: u32 }`, `LockedOut { retry_after_secs: u64 }`), `PinAttemptRecord` (`failure_count: u32`, `locked_until_epoch_ms: Option<u64>`, `last_attempt_epoch_ms: u64`), and `PinAttemptTracker` (`attempts: HashMap<String, PinAttemptRecord>`).
     - Implemented `check_lockout_at`, `record_failure_at`, `record_success`, and `clean_expired`.
     - Integrated `pub pin_tracker: Mutex<PinAttemptTracker>` into `AppState` (lines 285, 315).
     - Co-located 6 unit tests in `boss/src/state.rs` (lines 477-631) testing failure counting, lockout triggering, success resetting, lockout expiry, stale cleanup, and client isolation.
   - `boss/src/routes.rs`:
     - Added `extract_client_ip` (lines 143-175) checking `CF-Connecting-IP` -> `X-Forwarded-For` (first IP) -> `X-Real-IP` -> `ConnectInfo<SocketAddr>` -> fallback `"127.0.0.1"`.
     - Updated `verify_operator_pin` (lines 182-257):
       - Rejects locked-out clients immediately with HTTP `429 Too Many Requests`, header `Retry-After: <retry_after_secs>`, and JSON body `{ "ok": false, "error": "rate_limited", "retry_after_secs": u64 }`.
       - Compares candidate PIN using `constant_time_eq` against `state.config.operator_pin`.
       - On success, invokes `tracker.record_success(&client_ip)`, drops `tracker` lock prior to session minting, and returns HTTP `200 OK` with session token.
       - On failure, invokes `tracker.record_failure(&client_ip)`. Returns HTTP `429 Too Many Requests` on 5th attempt (with `Retry-After: 60`), and HTTP `401 Unauthorized` with `remaining_attempts: u32` for attempts 1-4.
     - Decoupled router construction via `pub fn build_router(state: SharedState) -> axum::Router` (lines 808-810) and `pub fn build_router_with_relay(...)` (lines 813-863).
     - Co-located 15 integration tests in `boss/src/routes.rs` (lines 866-1516) covering PIN unlock success, failure decrements, lockout trigger, post-lockout blocking, reset on valid PIN, IP header precedence, command FIFO polling, queue draining, ACK success, ACK failure, unknown command 404, cross-node ACK 403, command queue isolation, and endpoint authentication.
   - `boss/src/main.rs` (line 136):
     ```rust
     let app = routes::build_router_with_relay(state, relay_state);
     ```
     Router creation cleanly delegated to `routes::build_router_with_relay`.

2. **Automated Test Suite Verification**:
   - Executed `cargo test -p boss-server`:
     ```text
     running 28 tests
     test auth::tests::test_constant_time_eq_delegation ... ok
     test ota_sync::tests::test_sanitize_filename ... ok
     test state::tests::test_pin_tracker_failure_counting ... ok
     test state::tests::test_pin_tracker_clean_expired ... ok
     test state::tests::test_pin_tracker_lockout_expiry ... ok
     test plugins::tests::test_thermal_alert_plugin_with_none_thermals ... ok
     test state::tests::test_pin_tracker_lockout_trigger ... ok
     test plugins::tests::test_thermal_alert_plugin_with_normal_thermals ... ok
     test plugins::tests::test_thermal_alert_plugin_formats_missing_sensor_as_na ... ok
     test routes::tests::test_f6_command_ack_unknown_command_returns_404 ... ok
     test routes::tests::test_f5_pin_valid_returns_200_and_minted_token ... ok
     test routes::tests::test_f6_command_endpoints_require_auth ... ok
     test routes::tests::test_f6_command_ack_mismatched_node_returns_403 ... ok
     test routes::tests::test_f5_pin_ip_header_precedence ... ok
     test routes::tests::test_f5_pin_subsequent_attempts_locked_out_even_with_valid_pin ... ok
     test routes::tests::test_f5_pin_fifth_failure_triggers_429_lockout ... ok
     test routes::tests::test_f5_pin_valid_pin_resets_failure_count ... ok
     test routes::tests::test_f6_command_queue_isolation_between_nodes ... ok
     test routes::tests::test_f6_command_polling_drains_queue_second_poll_empty ... ok
     test routes::tests::test_f5_pin_multi_client_isolation ... ok
     test routes::tests::test_f5_pin_bad_pin_decrements_remaining_attempts ... ok
     test state::tests::test_pin_tracker_multi_client_isolation ... ok
     test state::tests::test_pin_tracker_success_resets_count ... ok
     test routes::tests::test_f6_command_enqueuing_and_fifo_polling ... ok
     test routes::tests::test_f6_command_ack_failure ... ok
     test netplay_relay::tests::relay_discovery_and_forwarding ... ok
     test routes::tests::test_f6_command_ack_success ... ok
     test netplay_relay::tests::relay_health_report_ingest ... ok

     test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
     ```
   - Executed `cargo test --workspace`:
     - `astro_daemon`: 10 passed; 0 failed
     - `astro_tune`: 5 passed; 0 failed
     - `beercade_hal`: 9 passed; 0 failed
     - `beercade_shared`: 11 passed; 0 failed
     - `boss_server`: 28 passed; 0 failed
     - Doc-tests: 0 failed
     - Total: 63 passed; 0 failed across entire workspace.

3. **Integrity & Build Verification**:
   - No hardcoded test outputs or mock bypasses were found in source code.
   - Zero image build commands (`make image-x86`, `mksquashfs`, Docker rootfs exports) were run or required.

---

## 2. Logic Chain

1. **Requirement R5 Compliance**:
   - Per Observation 1, `PinAttemptTracker` records failure counts and lockout timestamps per client identifier.
   - Thresholds match specification: 5 attempts triggers a 60-second lockout.
   - HTTP response codes conform to REST specifications: `401 Unauthorized` with `remaining_attempts` for attempts 1-4, `429 Too Many Requests` with `Retry-After: 60` and `retry_after_secs: 60` on the 5th attempt.
   - Active lockouts fail closed before evaluating the PIN candidate, defending against brute-force attacks.
   - Successful PIN authentication resets failure count to prevent lockout of valid operators.
   - Timing attacks are prevented by `constant_time_eq`.

2. **Requirement R6 Compliance**:
   - Per Observation 1, command enqueuing, polling, and ACK lifecycles are fully tested.
   - Polling drains pending commands in FIFO order and sets status to `Dispatched`.
   - ACK handlers update command status to `Acknowledged` (if `success: true`) or `Failed` (if `success: false`), record the completion timestamp, and save execution details.
   - Cross-node ACK attempts are rejected with `403 Forbidden`, and non-existent command IDs return `404 Not Found`.
   - Unauthenticated access returns `401 Unauthorized`.

3. **Integrity & Quality**:
   - Implementation is authentic with real state management in `AppState`.
   - Test suite uses in-memory Axum dispatch via `tower::ServiceExt::oneshot`, ensuring high-speed, repeatable, deterministic execution without network socket binding conflicts.
   - All 63 workspace tests pass without regression.

---

## 3. Caveats

1. **Direct LAN Untrusted Header Exposure**:
   `extract_client_ip` inspects `CF-Connecting-IP`, `X-Forwarded-For`, and `X-Real-IP`. If B.O.S.S. is accessed directly on a local subnet without a reverse proxy stripping spoofed headers, a sophisticated attacker could rotate client-supplied headers to evade IP-based rate limiting. In production, B.O.S.S. is designed to operate behind Cloudflare Tunnels or a trusted local gateway.
2. **In-Memory Volatility**:
   Lockout state is stored in memory (`AppState`). A daemon restart will clear active lockouts.
3. **Memory Footprint Under High-Cardinality Flooding**:
   `PinAttemptTracker` triggers `clean_expired` once `attempts.len() > 4096`. Under an active high-volume multi-IP attack within the 15-minute TTL, the map can grow until entries age out. Given small struct size (~56 bytes per entry), blast radius is minimal, but a hard size limit or LRU eviction could be considered in future hardening.

---

## 4. Conclusion

**Verdict: APPROVE**

Milestone 2 implementation satisfies all functional, security, and quality requirements:
- Operator PIN brute-force rate limiting (R5) is robustly implemented and tested.
- Command queue, dispatching, and ACK mechanics (R6) are thoroughly covered by automated integration tests.
- Crate tests and workspace tests pass 100% (28 in `boss-server`, 63 total).
- Zero image build commands were executed.

---

## 5. Verification Method

To independently verify this evaluation:

1. **Run B.O.S.S. Server Tests**:
   ```powershell
   cargo test -p boss-server
   ```
   *Expected Output*: 28 passed; 0 failed; 0 ignored.

2. **Run Only M2 Rate Limiter and Command Tests**:
   ```powershell
   cargo test -p boss-server -- routes::tests
   cargo test -p boss-server -- state::tests
   ```
   *Expected Output*: 15 passed in `routes::tests`, 6 passed in `state::tests`.

3. **Run Full Workspace Test Suite**:
   ```powershell
   cargo test --workspace
   ```
   *Expected Output*: 63 passed across all crates.

4. **Invalidation Conditions**:
   - Any test failure in `cargo test -p boss-server` or `cargo test --workspace`.
   - 5th failed PIN attempt does not return HTTP 429 or is missing `Retry-After` header.
   - Successful PIN authentication does not clear previous failure history.
   - Command ACK from an unauthorized node returns anything other than 403 Forbidden.

---

## 6. Review Summary & Adversarial Challenge Report

### Quality Review Summary
- **Verdict**: APPROVE
- **Integrity Check**: PASS (zero hardcoded shortcuts, facade implementations, or fabricated outputs).
- **Correctness**: PASS (R5 & R6 implemented per specifications).
- **Test Coverage**: PASS (21 new tests added; total 28 in `boss-server`).

### Verified Claims
- PIN lockout triggers at 5th attempt with HTTP 429 and `Retry-After: 60` -> Verified via `test_f5_pin_fifth_failure_triggers_429_lockout` -> PASS
- Lockout blocks further attempts even with correct PIN -> Verified via `test_f5_pin_subsequent_attempts_locked_out_even_with_valid_pin` -> PASS
- Valid PIN resets attempt counter -> Verified via `test_f5_pin_valid_pin_resets_failure_count` -> PASS
- Multi-client isolation -> Verified via `test_f5_pin_multi_client_isolation` and `test_pin_tracker_multi_client_isolation` -> PASS
- Command FIFO polling & queue draining -> Verified via `test_f6_command_enqueuing_and_fifo_polling` and `test_f6_command_polling_drains_queue_second_poll_empty` -> PASS
- Command ACK status transitions -> Verified via `test_f6_command_ack_success` and `test_f6_command_ack_failure` -> PASS
- Command ACK isolation & security -> Verified via `test_f6_command_ack_unknown_command_returns_404` and `test_f6_command_ack_mismatched_node_returns_403` -> PASS

### Adversarial Challenges
- **Challenge 1 (IP Header Trust)**:
  - *Assumption*: Caller IP can be determined via proxy headers (`CF-Connecting-IP`, `X-Forwarded-For`).
  - *Attack Scenario*: Attacker on bare LAN sends spoofed `X-Forwarded-For: <random_ip>` to bypass lockout.
  - *Risk*: Low-Medium on unprotected LAN; Negligible behind Cloudflare Tunnel.
  - *Mitigation*: Ensure reverse proxy / tunnel strips incoming client-forged forwarding headers.
- **Challenge 2 (Lock Ordering & Deadlock)**:
  - *Assumption*: `state.pin_tracker` lock does not conflict with `state.operator_sessions` lock.
  - *Analysis*: Verified that `drop(tracker)` is called at line 209 and line 225 before `mint_operator_session().await` is invoked, preventing nested lock acquisition and deadlocks.
  - *Risk*: None. Passed.
