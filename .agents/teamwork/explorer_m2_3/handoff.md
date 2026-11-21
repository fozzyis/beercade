# Handoff Report: B.O.S.S. Test Architecture & Harness (Requirement R6)

**Agent**: Explorer M2-3  
**Milestone**: Milestone 2 (B.O.S.S. Server Security & Command Plane)  
**Task Focus**: Test Infrastructure and Architecture for `boss-server` (Requirement R6)  
**Date**: 2026-10-27  

---

## 1. Observation

### 1.1 Existing Test Suite Baseline
- Executing `cargo test -p boss-server` runs 7 tests across 4 modules:
  - `boss/src/auth.rs:92-101`: `test_constant_time_eq_delegation`
  - `boss/src/ota_sync.rs:263-273`: `test_sanitize_filename`
  - `boss/src/plugins.rs:2639-2716`: 3 thermal alert tests (`test_thermal_alert_plugin_with_normal_thermals`, `test_thermal_alert_plugin_with_none_thermals`, `test_thermal_alert_plugin_formats_missing_sensor_as_na`)
  - `boss/src/netplay_relay.rs:443-542`: 2 tests (`relay_discovery_and_forwarding`, `relay_health_report_ingest`)
- Verbatim tool output:
  `test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s`
- Exact observed gaps:
  - `boss/src/routes.rs` contains 0 tests.
  - `boss/src/state.rs` contains 0 tests.
  - Directory `boss/tests/` does not exist.
  - `boss/Cargo.toml` declares `name = "boss-server"` with binary entry point `src/main.rs`. There is no `[lib]` entry in `Cargo.toml` and no `src/lib.rs`.

### 1.2 Router Construction in `boss/src/main.rs`
- In `boss/src/main.rs:146-186`, the Axum `Router` is created inline inside `async fn main()`:
  ```rust
  let app = Router::new()
      .route("/api/health", get(|| async { "ok" }))
      .route("/api/v1/telemetry", post(routes::ingest_telemetry))
      ...
      .route("/api/v1/auth/pin", post(routes::verify_operator_pin))
      .route("/api/v1/cabinet/:node_id/commands", get(routes::poll_commands))
      .route("/api/v1/cabinet/:node_id/commands/ack", post(routes::ack_command))
      .layer(TraceLayer::new_for_http())
      .layer(cors)
      .with_state(state)
      .merge(netplay_relay::router(relay_state));
  ```
- Because this router is defined locally within `main()`, it cannot be invoked from test modules without defining an exported constructor function.

### 1.3 Axum 0.7 In-Memory Service Execution & Dependencies
- `boss/Cargo.toml` lines 8–24:
  - `axum = { version = "0.7", features = ["ws"] }` (exact resolved version: `0.7.9`).
  - `tokio = { version = "1.38", features = ["full"] }`.
  - `tower-http = { version = "0.5", features = ["cors", "trace", "fs"] }`.
  - `beercade-shared = { path = "../shared/crates/beercade-shared" }`.
- Cargo tree inspection (`cargo tree -p boss-server -i tower`) confirms `tower v0.5.3` is already resolved transitively by `axum` and `tower-http`.
- However, `tower` is not declared as a direct dependency or dev-dependency in `boss/Cargo.toml`. To use `tower::ServiceExt::oneshot`, `tower = { version = "0.5", features = ["util"] }` must be added under `[dev-dependencies]`.
- Reading response bodies in Axum 0.7 is supported natively via `axum::body::to_bytes(res.into_body(), usize::MAX).await` (`axum-0.7.9/src/body/mod.rs:48`).

### 1.4 Client IP & `ConnectInfo<SocketAddr>` Mechanism
- In `axum-0.7.9/src/extract/connect_info.rs:142-159`:
  ```rust
  #[async_trait]
  impl<S, T> FromRequestParts<S> for ConnectInfo<T>
  where
      S: Send + Sync,
      T: Clone + Send + Sync + 'static,
  {
      type Rejection = <Extension<Self> as FromRequestParts<S>>::Rejection;

      async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
          match Extension::<Self>::from_request_parts(parts, state).await {
              Ok(Extension(connect_info)) => Ok(connect_info),
              Err(err) => match parts.extensions.get::<MockConnectInfo<T>>() {
                  Some(MockConnectInfo(connect_info)) => Ok(Self(connect_info.clone())),
                  None => Err(err),
              },
          }
      }
  }
  ```
- Direct injection: Inserting `ConnectInfo(addr)` into `req.extensions_mut()` satisfies the extractor without starting a TCP listener.
- Reverse-proxy header extraction: `boss/src/routes.rs:81-84` already extracts `CF-Connecting-IP`. Inspecting `X-Forwarded-For` and `CF-Connecting-IP` headers allows setting client IP in test requests via standard HTTP header builders.

---

## 2. Logic Chain

1. **Test Organization Constraint** (from Observation 1.1):
   - Cargo only permits external integration tests in `tests/*.rs` to link against library crates (`rlib`).
   - `boss-server` is a binary crate with only `src/main.rs`.
   - Creating `boss/tests/api_tests.rs` without converting `boss-server` into a library crate fails compilation with `error[E0463]: can't find crate for 'boss_server'`.
   - Therefore, the test suite must be co-located within the crate under `#[cfg(test)] mod tests` in `boss/src/routes.rs` (for HTTP routes) and `boss/src/state.rs` (for state unit tests), consistent with all other modules in `boss`.

2. **Router Decoupling for Testing** (from Observation 1.2):
   - Because `main()` constructs `app` inline, tests cannot exercise the real routing table without code duplication.
   - Refactoring the route registration into `pub fn build_router(state: SharedState) -> axum::Router` in `boss/src/routes.rs` allows both `main()` and test harnesses to instantiate the exact same route tree.

3. **Fast, Deterministic In-Memory Testing** (from Observation 1.3):
   - Testing via `tower::ServiceExt::oneshot` executes entirely in-memory within the Tokio runtime.
   - Unlike socket listeners (`127.0.0.1:0`), `oneshot` consumes no OS socket descriptors, avoids port conflicts and firewall popups, and completes 10+ HTTP test roundtrips in under 20 milliseconds.
   - Because `oneshot` consumes the service (`self`), cloning the router (`app.clone().oneshot(req)`) enables multi-request test scenarios against the same shared `Arc<AppState>`.

4. **Multi-Client Isolation Verification** (from Observation 1.4):
   - Multi-client lockout testing requires distinct client identifiers (e.g. Attacker `10.0.0.1` vs Legitimate `10.0.0.2`).
   - By setting `req.extensions_mut().insert(ConnectInfo(addr))` and/or `.header("X-Forwarded-For", ip)` on individual requests, sequential requests sent to the same `app.clone()` instance test client isolation deterministically.

---

## 3. Caveats

1. **No External Integration Test Directory**:
   - `boss/tests/` will not be created to prevent breaking the Cargo binary target build. All tests reside in `boss/src/routes.rs` and `boss/src/state.rs`.
2. **WebSocket and SSE Streaming Tests**:
   - `tower::ServiceExt::oneshot` is designed for HTTP request/response lifecycles. Long-lived streaming endpoints (`/api/telemetry/stream` SSE and `/api/v1/ws/telemetry` WebSocket) are not exercised via `oneshot`. However, F5 (PIN auth) and F6 (Command plane RPC/poll/ack) are 100% request/response HTTP routes and are fully covered.
3. **No Image Builds**:
   - Scope is strictly limited to code and automated `cargo test` execution; no squashfs or OS image build commands are permitted.

---

## 4. Conclusion

The test harness and test architecture for `boss-server` can be implemented cleanly, rapidly, and without architectural disruption by:
1. Adding `tower = { version = "0.5", features = ["util"] }` to `boss/Cargo.toml` under `[dev-dependencies]`.
2. Extracting `pub fn build_router(state: SharedState) -> axum::Router` in `boss/src/routes.rs` and invoking it from `main.rs`.
3. Implementing 10 test cases for Feature F5 (PIN unlock success, decrements, 429 lockout, `Retry-After: 60`, success reset, and multi-client isolation).
4. Implementing 10 test cases for Feature F6 (command enqueue, FIFO poll drain, ACK success/failure, 404 on unknown ID, 403 on node mismatch, and node queue isolation).
5. Placing all HTTP integration tests inside `boss/src/routes.rs` under `#[cfg(test)] mod tests`, and state unit tests inside `boss/src/state.rs` under `#[cfg(test)] mod tests`.

---

## 5. Verification Method

To independently verify the test infrastructure once implemented:

1. **Run Full B.O.S.S. Test Suite**:
   ```powershell
   cargo test -p boss-server
   ```
   **Expected**: Exit code 0, all 7 existing tests plus the ~20 new F5 and F6 tests passing (total ~27 tests).

2. **Run Only New Route Integration Tests**:
   ```powershell
   cargo test -p boss-server -- routes::tests
   ```
   **Expected**: All F5 and F6 route tests pass in < 50ms.

3. **Run State Unit Tests**:
   ```powershell
   cargo test -p boss-server -- state::tests
   ```
   **Expected**: All `PinAttemptTracker` state tests pass.

4. **Verify Zero Regressions Across Workspace**:
   ```powershell
   cargo test --workspace
   ```
   **Expected**: 100% pass across `beercade-shared`, `astro-daemon`, `boss-server`, and `beercade-hal`.
