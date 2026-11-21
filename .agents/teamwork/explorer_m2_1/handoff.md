# Handoff Report: Explorer M2-1 (Requirement R5 Investigation)

**Date**: 2026-10-27T23:08:00Z  
**Author**: Explorer M2-1 (`explorer_m2_1`)  
**Recipient**: Orchestrator (`bd770878-29b7-48ec-a1e1-b5265681a707`) / Implementer M2-1  
**Milestone**: Milestone 2 (B.O.S.S. Server Security & Command Plane)  
**Task**: Requirement R5 (Operator PIN Brute-Force Rate Limiting) Analysis

---

## 1. Observation

1. **Current Operator PIN Handler**:
   - Location: `boss/src/routes.rs:142–172`
   - Current implementation:
     ```rust
     pub async fn verify_operator_pin(
         State(state): State<SharedState>,
         Json(req): Json<PinUnlockRequest>,
     ) -> Response {
         let candidate = req.pin.trim();
         if constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes()) {
             let (token, expires_at_epoch_ms) = state.mint_operator_session().await;
             tracing::info!("Operator kiosk session unlocked via PIN");
             (
                 StatusCode::OK,
                 Json(json!({
                     "ok": true,
                     "token": token,
                     "expires_at_epoch_ms": expires_at_epoch_ms,
                     "venue": state.config.venue_name,
                 })),
             )
                 .into_response()
         } else {
             tracing::warn!("Rejected invalid operator PIN attempt");
             (
                 StatusCode::UNAUTHORIZED,
                 Json(json!({
                     "ok": false,
                     "error": "invalid_pin",
                     "detail": "Incorrect 6-digit operator PIN."
                 })),
             )
                 .into_response()
         }
     }
     ```
   - Observations:
     - The handler accepts only `State(state)` and `Json(req)`.
     - No client IP, headers, or connection info is extracted.
     - No attempt counter, failure tracker, or lockout timer exists.
     - On failure, returns `401 Unauthorized` without `remaining_attempts`.
     - `429 Too Many Requests` and `Retry-After` HTTP headers are never generated.

2. **Current `AppState` Structure & Concurrency**:
   - Location: `boss/src/state.rs:144–168`
   - `SharedState` is `Arc<AppState>`.
   - `AppState` manages:
     - `operator_sessions: RwLock<HashMap<String, u64>>`
     - `edge_buffer: Mutex<VecDeque<EdgeItem>>`
     - `nodes: RwLock<HashMap<String, (NodeTelemetry, u64)>>`
     - `commands: RwLock<HashMap<String, CommandRecord>>`
     - `pending_commands: RwLock<HashMap<String, VecDeque<String>>>`
   - Timestamp helper `models::now_epoch_ms()` is used across all collections (`u64` Unix epoch ms).
   - There is no `PinAttemptTracker` or failure rate-limiting struct in `boss/src/state.rs`.

3. **Existing Header Usage in Routes**:
   - In `boss/src/routes.rs:81–85`:
     ```rust
     let cf_ip = headers
         .get("CF-Connecting-IP")
         .and_then(|h| h.to_str().ok())
         .unwrap_or("direct");
     ```
   - Demonstrates that `headers: HeaderMap` is already idiomatic in `boss/src/routes.rs` for extracting client IPs from Cloudflare.

4. **Axum Version and Dependencies**:
   - Location: `boss/Cargo.toml:9`, `Cargo.lock:94–97`
   - `axum = { version = "0.7", features = ["ws"] }` (exact resolved version: `0.7.9`).
   - `boss/src/main.rs:219`: `axum::serve(listener, app).await.unwrap();`.
   - `ConnectInfo<SocketAddr>` is supported in Axum 0.7 by wrapping in `Option<ConnectInfo<SocketAddr>>` or configuring `app.into_make_service_with_connect_info::<SocketAddr>()`.

5. **Existing Test Suite Baseline**:
   - `cargo test -p boss-server` runs 7 tests in `main.rs` (all passing).
   - Currently, there are 0 tests in `boss/src/routes.rs` or `boss/src/state.rs` for PIN rate limiting or command polling/acking.

---

## 2. Logic Chain

1. **Vulnerability to Brute-Force Attacks** (from Observation 1):
   Because `verify_operator_pin` performs no tracking, an attacker on the local venue network can generate automated requests to guess the 6-digit operator PIN (`1,000,000` possible combinations). At typical local HTTP latency (<1ms), brute force would succeed in minutes without detection or throttling.

2. **Placement of Failure Tracker in `AppState`** (from Observation 2):
   `AppState` is shared by all Axum worker tasks via `Arc<AppState>`. To share failure state across requests from the same client IP while maintaining thread safety, `AppState` must own the tracker. Adding `pub pin_tracker: Mutex<PinAttemptTracker>` to `AppState` provides safe mutable access that aligns with the pattern used for `edge_buffer: Mutex<VecDeque<EdgeItem>>`.

3. **In-Memory Tracking & Timestamp Choice** (from Observation 2):
   `AppState` uses `now_epoch_ms()` for `operator_sessions`, `nodes`, and `commands`. Using `now_epoch_ms()` allows straightforward lockout calculation (`locked_until_epoch_ms = now + (60 * 1000)`), exact `retry_after_secs = ((until - now + 999) / 1000).max(1)`, and deterministic test assertions without clock drift.

4. **Client IP Identification Strategy** (from Observations 1, 3, and 4):
   Kiosk requests, LAN operator tablets, Cloudflare tunnel connections, and unit tests interact with B.O.S.S. under different networking conditions. Extracting the client identifier by checking `CF-Connecting-IP` -> `X-Forwarded-For` (first IP) -> `X-Real-IP` -> `Option<ConnectInfo<SocketAddr>>` -> fallback `"127.0.0.1"` guarantees:
   - Zero extractor failures/panics.
   - Compatibility with reverse proxies and Cloudflare Tunnels.
   - Immediate testability in unit/integration tests using `tower::ServiceExt::oneshot` with custom headers.
   - Independent tracking per client IP (allowing legitimate clients to log in while an attacker is locked out).

5. **Lockout and Threshold Evaluation Order** (from Observation 1):
   - **Step 1 (Pre-Check)**: Check if the client is already locked out (`now < locked_until`). If locked out, immediately return `429 Too Many Requests` with `Retry-After: <secs>`. Do NOT evaluate candidate PIN.
   - **Step 2 (Evaluate PIN)**: If not locked out, compare candidate PIN with `state.config.operator_pin` using `constant_time_eq`.
   - **Step 3 (Success)**: On match, clear failure history for this client (`reset(&client_ip)`), mint session token, and return `200 OK`.
   - **Step 4 (Failure)**: On mismatch, increment failure count. If failure count reaches 5:
     - Set lockout deadline to `now + 60,000 ms`.
     - Return `429 Too Many Requests` with `Retry-After: 60` and `retry_after_secs: 60`.
     If failure count < 5:
     - Return `401 Unauthorized` with `remaining_attempts: 5 - failure_count`.

---

## 3. Caveats

1. **Ephemeral In-Memory Tracking**:
   The tracker state is in-memory only and is reset on B.O.S.S. server restart. This is standard and expected for rate-limiting daemons in this architecture; persistent storage across reboots is not required by §R5.
2. **IP Spoofing on Direct LAN**:
   If an attacker can spoof raw IP packets at Layer 3 on the local Ethernet switch, IP-based tracking could theoretically track spoofed IPs; however, TCP 3-way handshake completion prevents spoofed IP exploitation over HTTP.
3. **Memory Bounding**:
   To prevent memory exhaustion attacks from thousands of spoofed origins, the tracker should maintain an eviction threshold (e.g., maximum 4,096 entries, pruning expired records).
4. **No Other Areas Uninvestigated**:
   The requirements, route handler, state sharing, response schemas, and test matrices are fully scoped.

---

## 4. Conclusion

Requirement R5 is clear, well-isolated, and ready for immediate implementation in Milestone 2:
- Define `PinAttemptTracker` in `boss/src/state.rs` using `now_epoch_ms()` and a 5-failure / 60-second lockout threshold.
- Add `pub pin_tracker: Mutex<PinAttemptTracker>` to `AppState`.
- Update `verify_operator_pin` in `boss/src/routes.rs` to extract client IP via `CF-Connecting-IP`, `X-Forwarded-For`, `X-Real-IP`, or `ConnectInfo`, check active lockouts (429), verify candidate PIN, reset on success (200), and count failures (401 with `remaining_attempts` down to 429 with `Retry-After`).
- Implement unit and integration tests verifying 401 response counts, 429 lockout trigger, `Retry-After` header value, unlock reset, and multi-client isolation.

---

## 5. Verification Method

To independently verify this investigation and the upcoming implementation:
1. **Source Inspection**:
   - Check `boss/src/routes.rs:142–172` for the current unconstrained PIN verification.
   - Check `boss/src/state.rs:144–192` for `AppState` fields and lock types.
2. **Automated Unit & Integration Tests**:
   - Run `cargo test -p boss-server` to confirm current 7 passing baseline tests.
   - When R5 tests are implemented:
     `cargo test -p boss-server -- test_pin` (or run full suite `cargo test -p boss-server`).
   - Tests will assert:
     - POST valid PIN -> `StatusCode::OK`, `ok: true`, valid token.
     - 1st to 4th bad PINs -> `StatusCode::UNAUTHORIZED`, `remaining_attempts: 4, 3, 2, 1`.
     - 5th bad PIN -> `StatusCode::TOO_MANY_REQUESTS`, header `Retry-After: 60`, `retry_after_secs: 60`.
     - Subsequent attempts while locked out -> `StatusCode::TOO_MANY_REQUESTS`.
     - Concurrent attempt from another IP -> `StatusCode::OK` (client isolation).
     - Advancing time past 60s -> Lockout expires, attempts permitted again.
3. **Invalidation Conditions**:
   - If `POST /api/v1/auth/pin` returns 401 on the 5th failed attempt instead of 429.
   - If the `Retry-After` HTTP header is missing from 429 responses.
   - If failure tracking locks out legitimate clients across different IP addresses.
