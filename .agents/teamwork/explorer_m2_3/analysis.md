# B.O.S.S. Server Test Architecture & Harness Analysis (Requirement R6)

**Agent**: Explorer M2-3  
**Date**: 2026-10-27  
**Scope**: Test Infrastructure, Axum 0.7 In-Memory Harness (`tower::ServiceExt::oneshot`), Client IP / `ConnectInfo` Mocking, F5 (Operator PIN Rate Limiting) Test Suite, F6 (Command Plane) Test Suite, and Test Organization Strategy  
**Repository**: `d:\data\dev\beercade`

---

## 1. Executive Summary

This investigation establishes the test architecture and harness design for `boss-server` (B.E.E.R.C.A.D.E. Operations & Supervisory System), directly supporting Milestone 2 Requirements R5 (Operator PIN Brute-Force Rate Limiting) and R6 (Command Plane Queue, Polling, ACK, and Automated Test Coverage).

### Key Takeaways
1. **Existing Test Suite Baseline**:
   - `cargo test -p boss-server` executes 7 tests across 4 modules (`auth.rs`, `ota_sync.rs`, `netplay_relay.rs`, `plugins.rs`).
   - `boss/src/routes.rs` and `boss/src/state.rs` contain **0 automated tests**.
   - No integration test directory (`boss/tests/`) exists.
   - `boss-server` is configured as a binary-only crate (`[[bin]]`). It has no library target (`[lib]` or `src/lib.rs`).
2. **Axum 0.7 Router Extraction**:
   - Currently, the HTTP route tree is assembled inline inside `boss/src/main.rs:146-186` within `async fn main()`. It cannot be instantiated by tests.
   - Refactoring proposal: Extract `pub fn build_router(state: SharedState) -> Router` in `boss/src/routes.rs`. `main()` then simply merges this router with `netplay_relay::router(relay_state)`.
3. **In-Memory Testing via `tower::ServiceExt::oneshot`**:
   - Tests can invoke `app.clone().oneshot(req)` in memory without opening TCP sockets.
   - Requires adding `tower = { version = "0.5", features = ["util"] }` to `boss/Cargo.toml` under `[dev-dependencies]`.
   - Response body reading is handled via `axum::body::to_bytes(res.into_body(), usize::MAX).await`, requiring zero additional dependencies.
4. **Client IP & `ConnectInfo<SocketAddr>` Mocking**:
   - Axum 0.7's `ConnectInfo<T>` extractor inspects request extensions for `ConnectInfo<T>` or `MockConnectInfo<T>`.
   - In tests, client IP addresses can be injected directly per request:
     `req.extensions_mut().insert(axum::extract::ConnectInfo(client_addr));`
   - Additionally, reverse-proxy headers (`CF-Connecting-IP`, `X-Forwarded-For`, `X-Real-IP`) can be set on request builders, matching the handler's extraction fallback chain.
5. **Test Organization**:
   - Because `boss-server` is a binary crate, external integration tests in `boss/tests/` cannot import `boss_server::...` without restructuring the package into a library.
   - In accordance with the repository's convention (`auth.rs`, `netplay_relay.rs`, `plugins.rs`, `ota_sync.rs`), tests should be organized as:
     - **In-crate HTTP route integration tests**: `boss/src/routes.rs` under `#[cfg(test)] mod tests`.
     - **State unit tests**: `boss/src/state.rs` under `#[cfg(test)] mod tests` for `PinAttemptTracker` logic.

---

## 2. Existing Test Suite Inventory

Executing `cargo test -p boss-server` produces:

```
running 7 tests
test auth::tests::test_constant_time_eq_delegation ... ok
test ota_sync::tests::test_sanitize_filename ... ok
test plugins::tests::test_thermal_alert_plugin_with_normal_thermals ... ok
test plugins::tests::test_thermal_alert_plugin_with_none_thermals ... ok
test plugins::tests::test_thermal_alert_plugin_formats_missing_sensor_as_na ... ok
test netplay_relay::tests::relay_discovery_and_forwarding ... ok
test netplay_relay::tests::relay_health_report_ingest ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

### Module Breakdown
| File | Test Module | Test Name | Target / Nature |
|---|---|---|---|
| `boss/src/auth.rs:92-101` | `mod tests` | `test_constant_time_eq_delegation` | Unit test for constant-time byte slice comparison |
| `boss/src/ota_sync.rs:263-273` | `mod tests` | `test_sanitize_filename` | Unit test for path traversal prevention |
| `boss/src/plugins.rs:2639-2716` | `mod tests` | `test_thermal_alert_plugin_with_normal_thermals` | Unit test for thermal threshold alerting |
| `boss/src/plugins.rs:2665-2675` | `mod tests` | `test_thermal_alert_plugin_with_none_thermals` | Unit test for Option<f32> None thermals (M1) |
| `boss/src/plugins.rs:2688-2716` | `mod tests` | `test_thermal_alert_plugin_formats_missing_sensor_as_na` | Unit test for formatting missing sensors (M1) |
| `boss/src/netplay_relay.rs:443-487` | `mod tests` | `relay_discovery_and_forwarding` | Async UDP relay discovery & packet forwarding |
| `boss/src/netplay_relay.rs:490-542` | `mod tests` | `relay_health_report_ingest` | Integration test spinning up TCP listener on `127.0.0.1:0` and doing raw HTTP socket roundtrip |

### Gap Analysis
- **`boss/src/routes.rs`**: 721 lines of code, covering 15+ API routes. **0 tests**.
- **`boss/src/state.rs`**: 347 lines of code, managing node registry, command FIFO queues, operator sessions, SSE broadcast buses. **0 tests**.
- **`boss/tests/`**: Directory does not exist.

---

## 3. Axum 0.7 In-Memory Test Architecture (`tower::ServiceExt::oneshot`)

### 3.1 The Problem: Monolithic Router Construction
In `boss/src/main.rs:146-186`, the `Router` is declared inside `async fn main()`:

```rust
// boss/src/main.rs:146-186
let app = Router::new()
    .route("/api/health", get(|| async { "ok" }))
    .route("/api/v1/telemetry", post(routes::ingest_telemetry))
    // ... 15+ routes ...
    .route("/api/v1/auth/pin", post(routes::verify_operator_pin))
    .route("/api/v1/cabinet/:node_id/commands", get(routes::poll_commands))
    .route("/api/v1/cabinet/:node_id/commands/ack", post(routes::ack_command))
    .layer(TraceLayer::new_for_http())
    .layer(cors)
    .with_state(state)
    .merge(netplay_relay::router(relay_state));
```
Because `app` is local to `main()`, test code cannot construct this router without duplicating all routes.

### 3.2 The Solution: Extract `build_router`
In `boss/src/routes.rs`, define:

```rust
/// Build the core B.O.S.S. Axum HTTP router bound to the provided application state.
pub fn build_router(state: SharedState) -> axum::Router {
    use axum::routing::{get, post};
    use tower_http::cors::{Any, CorsLayer};
    use tower_http::trace::TraceLayer;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    axum::Router::new()
        // Health check
        .route("/api/health", get(|| async { "ok" }))
        // Telemetry pipeline
        .route("/api/v1/telemetry", post(ingest_telemetry))
        .route("/api/telemetry", get(get_telemetry_snapshot))
        // Dual-Mode Operator Authentication & Overview
        .route("/api/v1/auth/pin", post(verify_operator_pin))
        .route("/api/v1/auth/verify", get(verify_operator_session))
        .route("/api/v1/auth/logout", post(revoke_operator_session))
        .route("/api/v1/admin/overview", get(get_admin_overview))
        // Authenticated cabinet RPC
        .route("/api/v1/cabinet/:node_id/reboot", post(cabinet_reboot))
        .route("/api/v1/cabinet/:node_id/ping", post(cabinet_ping))
        .route("/api/v1/cabinet/:node_id/launch_game", post(cabinet_launch_game))
        .route("/api/v1/credits/add", post(credits_add))
        .route("/api/v1/commands/:command_id", get(command_status))
        // Node command plane
        .route("/api/v1/cabinet/:node_id/commands", get(poll_commands))
        .route("/api/v1/cabinet/:node_id/commands/ack", post(ack_command))
        // Node events
        .route("/api/v1/events/error", post(report_error))
        .route("/api/v1/scores", post(submit_score))
        // Venue UI theme
        .route("/api/v1/ui/theme", get(crate::ui_theme::get_theme).post(crate::ui_theme::set_theme))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
```

In `boss/src/main.rs:146`, `main()` simplifies to:
```rust
let app = routes::build_router(state)
    .merge(netplay_relay::router(relay_state));
```

### 3.3 Invoking via `tower::ServiceExt::oneshot`
When a router has state applied (`Router<()>` via `.with_state(state)`), it implements `tower::Service<Request<Body>>`.
To invoke it in tests:

```rust
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use tower::ServiceExt; // from tower crate

let state = Arc::new(AppState::new(Config::from_env()));
let app = build_router(state.clone());

let req = Request::builder()
    .method("POST")
    .uri("/api/v1/auth/pin")
    .header(header::CONTENT_TYPE, "application/json")
    .body(Body::from(r#"{"pin":"898900"}"#))
    .unwrap();

// Note: oneshot consumes self, so clone the router handle for sequential requests!
let response = app.clone().oneshot(req).await.unwrap();
assert_eq!(response.status(), StatusCode::OK);
```

### 3.4 Reading Response Bodies Without Extra Dependencies
In Axum 0.7, `axum::body::to_bytes` is built-in:
```rust
let status = response.status();
let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
let json_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
```
This avoids needing `http-body-util` or manual stream pinning.

### 3.5 Required Dependency Configuration
In `boss/Cargo.toml`, add:
```toml
[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
```
`tower` is already present in `Cargo.lock` (v0.5.3 as a dependency of `axum`), so adding this dev-dependency involves zero crate downloading or compilation overhead.

---

## 4. Client IP Injection & `ConnectInfo` Mocking Architecture

### 4.1 How Axum 0.7 `ConnectInfo` Works
In `axum-0.7.9/src/extract/connect_info.rs:150-158`:
```rust
async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    match Extension::<Self>::from_request_parts(parts, state).await {
        Ok(Extension(connect_info)) => Ok(connect_info),
        Err(err) => match parts.extensions.get::<MockConnectInfo<T>>() {
            Some(MockConnectInfo(connect_info)) => Ok(Self(connect_info.clone())),
            None => Err(err),
        },
    }
}
```

The extractor checks:
1. `parts.extensions.get::<ConnectInfo<T>>()`
2. `parts.extensions.get::<MockConnectInfo<T>>()`

### 4.2 Three Methods to Supply Fake Client IP in Tests

#### Method A: Direct Per-Request `ConnectInfo` Insertion (Recommended for Multi-Client Tests)
```rust
use axum::extract::ConnectInfo;
use std::net::SocketAddr;

let mut req = Request::builder()
    .method("POST")
    .uri("/api/v1/auth/pin")
    .header(header::CONTENT_TYPE, "application/json")
    .body(Body::from(r#"{"pin":"000000"}"#))
    .unwrap();

let fake_ip: SocketAddr = "192.168.1.100:12345".parse().unwrap();
req.extensions_mut().insert(ConnectInfo(fake_ip));
```
**Advantage**: Each request can specify a completely different `SocketAddr` while targeting the same router instance `app.clone()`. This allows multi-client lockout isolation testing in a single sequential test!

#### Method B: `MockConnectInfo` Layer
```rust
use axum::extract::connect_info::MockConnectInfo;
use std::net::SocketAddr;

let fake_ip: SocketAddr = "192.168.1.100:12345".parse().unwrap();
let test_app = app.layer(MockConnectInfo(fake_ip));
```
**Advantage**: All requests through `test_app` inherit the mocked socket address without mutating request extensions.

#### Method C: Reverse-Proxy Headers (`CF-Connecting-IP`, `X-Forwarded-For`)
Because B.O.S.S. operates behind Cloudflare Tunnel (or reverse proxies on kiosks), route handlers should extract client IP using a robust fallback chain:
1. `CF-Connecting-IP` header
2. `X-Forwarded-For` header (first IP in CSV)
3. `X-Real-IP` header
4. `ConnectInfo<SocketAddr>` peer address
5. Fallback `"127.0.0.1"`

In tests, setting headers is trivial:
```rust
let req = Request::builder()
    .method("POST")
    .uri("/api/v1/auth/pin")
    .header("CF-Connecting-IP", "10.0.0.99")
    .header(header::CONTENT_TYPE, "application/json")
    .body(Body::from(r#"{"pin":"000000"}"#))
    .unwrap();
```

---

## 5. Test Case Specification: Feature F5 (Operator PIN Rate Limiting)

Requirement R5 specifies:
- `POST /api/v1/auth/pin` accepts `{"pin": "string"}`.
- Correct PIN returns 200 OK with `{"ok": true, "token": "...", "expires_at_epoch_ms": ..., "venue": "..."}`.
- Bad PIN (<5 failures) returns 401 Unauthorized with `{"ok": false, "error": "invalid_pin", "detail": "...", "remaining_attempts": u32}`.
- 5th failed attempt triggers 429 Too Many Requests with `{"ok": false, "error": "rate_limited", "detail": "...", "retry_after_secs": u64}` and HTTP header `Retry-After: <seconds>`.
- Lockout duration: 60 seconds.
- Successful unlock resets failure count for that client IP.
- Multi-client isolation: Client A locked out does NOT block Client B.

### Test Matrix for F5
| Test ID | Test Function Name | Tier | Precondition | Request | Expected Status | Key Assertions |
|---|---|---|---|---|---|---|
| F5.1 | `test_pin_unlock_success` | 1 | Clean state | Correct PIN (`"898900"`) | 200 OK | `ok == true`, `token` non-empty string starting with `boss_sess_`, `expires_at_epoch_ms > now`, `venue == config.venue_name`. Token accepted by `/api/v1/auth/verify`. |
| F5.2 | `test_pin_unlock_bad_pin_first_attempt` | 1 | Clean state | Bad PIN (`"000000"`) | 401 Unauthorized | `ok == false`, `error == "invalid_pin"`, `remaining_attempts == 4`. No `Retry-After` header. |
| F5.3 | `test_pin_unlock_decrementing_attempts` | 1 | Clean state | 4 consecutive bad PINs | 401 Unauthorized (x4) | `remaining_attempts` sequentially returns 4, 3, 2, 1. |
| F5.4 | `test_pin_unlock_5th_failure_triggers_429` | 2 | 4 prior failures | 5th bad PIN | 429 Too Many Requests | `ok == false`, `error == "rate_limited"`, `retry_after_secs >= 59 && <= 60`. Header `Retry-After` present with value `60` (or `59`). |
| F5.5 | `test_pin_unlock_locked_out_subsequent_attempts` | 2 | Client locked out (5 failures) | 6th attempt with bad PIN | 429 Too Many Requests | Immediate 429 with `Retry-After` header. |
| F5.6 | `test_pin_unlock_lockout_blocks_even_correct_pin` | 2 | Client locked out (5 failures) | Correct PIN (`"898900"`) | 429 Too Many Requests | Crucial security check: Attacker cannot bypass lockout by guessing right after locking out. |
| F5.7 | `test_pin_unlock_success_resets_counter` | 2 | 3 failed attempts (remaining: 2) | Correct PIN (`"898900"`) | 200 OK | Success resets counter. Next bad PIN returns 401 with `remaining_attempts == 4` (not 1). |
| F5.8 | `test_pin_unlock_multi_client_isolation` | 3 | Attacker locked out at IP `10.0.0.1` | Legitimate client at IP `10.0.0.2` sends correct PIN | 200 OK | Legitimate client succeeds immediately. Attacker at `10.0.0.1` remains 429 locked out. |
| F5.9 | `test_pin_unlock_whitespace_trimmed` | 3 | Clean state | PIN with whitespace (`"  898900  "`) | 200 OK | Whitespace trimmed before constant-time comparison. |
| F5.10 | `test_pin_unlock_empty_pin_rejected` | 3 | Clean state | Empty string (`""`) | 401 Unauthorized | Rejected as bad PIN, decrements `remaining_attempts` to 4. |

---

## 6. Test Case Specification: Feature F6 (Command Queue, Poll, and ACK)

Requirement R6 specifies:
- Supervisory endpoints enqueue commands for specific arcade nodes (`reboot`, `ping`, `launch_game`, `credits/add`).
- Commands are tracked in-memory with lifecycle status (`Queued` -> `Dispatched` -> `Acknowledged` / `Failed`).
- Nodes poll `GET /api/v1/cabinet/:node_id/commands` with node auth token; polling drains the pending queue in FIFO order.
- Nodes acknowledge commands via `POST /api/v1/cabinet/:node_id/commands/ack`.
- Security checks: Mismatched `node_id` in ACK returns 403 Forbidden; missing command ID returns 404 Not Found; missing auth returns 401 Unauthorized.

### Test Matrix for F6
| Test ID | Test Function Name | Tier | Precondition | Request | Expected Status | Key Assertions |
|---|---|---|---|---|---|---|
| F6.1 | `test_command_enqueue_reboot` | 1 | Clean state | `POST /api/v1/cabinet/ORION-01/reboot` with RPC token | 200 OK | Returns `command_id` UUID, `status == "queued"`. Polling `GET /api/v1/commands/:id` shows status `Queued`, `dispatched_at_epoch_ms == null`. |
| F6.2 | `test_command_enqueue_credits_add` | 1 | Clean state | `POST /api/v1/credits/add` with `credits: 5` and RPC token | 200 OK | Returns `command_id`, `credits == 5`, `projected_balance == 5`. |
| F6.3 | `test_command_poll_empty_queue` | 1 | No commands queued | `GET /api/v1/cabinet/ORION-01/commands` with node token | 200 OK | Returns `{"node_id": "ORION-01", "commands": []}`. |
| F6.4 | `test_command_poll_fifo_order_and_drain` | 1 | 3 commands enqueued in order: Ping, Credits, Reboot | `GET /api/v1/cabinet/ORION-01/commands` with node token | 200 OK | Commands returned in exact FIFO order. All 3 marked `status: "dispatched"`, `dispatched_at_epoch_ms` set. Immediate 2nd poll returns `commands: []`. |
| F6.5 | `test_command_ack_success` | 1 | Command polled/dispatched | `POST /api/v1/cabinet/ORION-01/commands/ack` with `success: true, detail: {"ok": true}` | 200 OK | Status updated to `Acknowledged`, `completed_at_epoch_ms` set, `result` matches detail. |
| F6.6 | `test_command_ack_failure` | 1 | Command polled/dispatched | `POST /api/v1/cabinet/ORION-01/commands/ack` with `success: false, detail: {"error": "ROM missing"}` | 200 OK | Status updated to `Failed`, `completed_at_epoch_ms` set, `result` matches error detail. |
| F6.7 | `test_command_ack_unknown_id` | 2 | Clean state | ACK with random non-existent UUID | 404 Not Found | Explicit 404 response. |
| F6.8 | `test_command_ack_node_mismatch` | 2 | Command enqueued for `ORION-01` | ACK sent to `/api/v1/cabinet/ORION-02/commands/ack` | 403 Forbidden | Explicit 403 response. Original command in state remains unaffected. |
| F6.9 | `test_command_node_queue_isolation` | 3 | Command A for `ORION-01`, Command B for `ORION-02` | `ORION-01` polls | 200 OK | `ORION-01` receives only Command A. `ORION-02`'s queue remains intact with Command B. |
| F6.10 | `test_command_auth_guards` | 3 | Any command route | Requests missing `Authorization: Bearer` header | 401 Unauthorized | Enqueue requires `RequireRpcToken`. Poll and ACK require `RequireNodeToken`. |

---

## 7. Exact Test Organization Strategy

### 7.1 Option Comparison

#### Option 1: External Integration Tests in `boss/tests/api_tests.rs`
- **Rust/Cargo Behavior**: In Cargo, integration test files under `tests/*.rs` are compiled as separate external crates. Cargo requires the parent package to provide a library target (`[lib]` or `src/lib.rs`).
- **Fatal Obstacle**: `boss-server` has `Cargo.toml` configured only with `[[bin]]` (`src/main.rs`). It does NOT have a library artifact. Running `cargo test --test api_tests` will fail with:
  `error[E0463]: can't find crate for 'boss_server'`
- **Conclusion**: Avoid unless refactoring the entire crate into dual lib/bin.

#### Option 2: In-Crate Route Tests in `boss/src/routes.rs` (`#[cfg(test)] mod tests`) (RECOMMENDED)
- **Rust/Cargo Behavior**: `#[cfg(test)] mod tests` inside `boss/src/routes.rs` compiles directly as part of the binary's unit test runner (`unittests src\main.rs`).
- **Advantages**:
  - Direct access to `routes::build_router(state)`.
  - Direct access to `SharedState`, `Config`, `AppState`, `CommandAck`, `CommandRecord`.
  - Zero changes to crate target structure (`Cargo.toml` targets stay untouched).
  - Matches the established pattern in `boss/src/auth.rs`, `boss/src/ota_sync.rs`, `boss/src/netplay_relay.rs`, and `boss/src/plugins.rs`.
  - Executed with standard `cargo test -p boss-server`.

#### Option 3: Dedicated State Unit Tests in `boss/src/state.rs`
- In addition to route-level HTTP tests in `routes.rs`, pure unit tests for `PinAttemptTracker` (time window calculations, counter decrements, expiry) should be placed in `boss/src/state.rs` under `#[cfg(test)] mod tests`. This isolates state machine logic from HTTP serialization.

---

## 8. Implementation Code Architecture for Milestone 2 Workers

Here is the exact, ready-to-implement code structure for the test harness and test suites:

### 8.1 Test Helper Functions (to include in `boss/src/routes.rs` tests module)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{header, Request, StatusCode},
    };
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::state::{AppState, Config};

    fn test_state() -> Arc<AppState> {
        let mut config = Config::from_env();
        config.operator_pin = "898900".to_string();
        config.rpc_token = "test_rpc_token_secret".to_string();
        config.node_token = "test_node_token_secret".to_string();
        config.venue_name = "Test Venue Arcade".to_string();
        Arc::new(AppState::new(config))
    }

    async fn send_request(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        auth_token: Option<&str>,
        client_ip: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);

        if let Some(token) = auth_token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        if let Some(ip) = client_ip {
            // Supply via header for reverse-proxy compatibility
            builder = builder.header("X-Forwarded-For", ip);
        }

        let body_bytes = match body {
            Some(json) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&json).unwrap())
            }
            None => Body::empty(),
        };

        let mut req = builder.body(body_bytes).unwrap();

        // Also inject ConnectInfo extension for socket-level extractor compatibility
        if let Some(ip) = client_ip {
            let addr: SocketAddr = format!("{ip}:12345").parse().unwrap();
            req.extensions_mut().insert(ConnectInfo(addr));
        }

        let res = app.oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);

        (status, headers, json)
    }
```

---

## 9. Verification Commands

Workers and sentinels can verify this test architecture with the following commands:

```powershell
# 1. Run all unit and integration tests in boss-server
cargo test -p boss-server

# 2. Run specifically route and PIN / command plane tests
cargo test -p boss-server -- routes::tests

# 3. Run all tests in the workspace without building disk images
cargo test --workspace
```
