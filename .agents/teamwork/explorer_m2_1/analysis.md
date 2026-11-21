# Technical Analysis: Requirement R5 — B.O.S.S. Operator PIN Brute-Force Rate Limiting

**Investigator**: Explorer M2-1  
**Target Milestone**: Milestone 2 (B.O.S.S. Server Security & Command Plane)  
**Target Crates / Files**: `boss/src/routes.rs`, `boss/src/state.rs`, `boss/src/main.rs`  
**Referenced Specs**: `ORIGINAL_REQUEST.md` (§R5, §R6), `PROJECT.md` (Features 4, 5), `TEST_INFRA.md` (Feature F5, Scenario 3)

---

## Executive Summary
This analysis details the exact architecture, data structures, concurrency controls, HTTP contracts, and verification strategy required to implement **Requirement R5: Operator PIN Brute-Force Rate Limiting** in the B.O.S.S. supervisory server.

Currently, `POST /api/v1/auth/pin` validates candidates in constant time but does not track failures, allowing an attacker on the local network (LAN) or a compromised client to execute unconstrained brute-force attempts. We design a robust in-memory `PinAttemptTracker` embedded within `AppState`, providing client IP extraction (with proxy header precedence, direct socket fallback, and unit test compatibility), 5-failure threshold tracking, 60-second lockout, RFC-compliant `429 Too Many Requests` responses with `Retry-After` headers, and per-client isolation.

---

## 1. Current Implementation in `boss/src/routes.rs`

### 1.1 Existing Endpoint Logic
The current handler for `POST /api/v1/auth/pin` in `boss/src/routes.rs` (lines 140–172) is:

```rust
#[derive(Deserialize)]
pub struct PinUnlockRequest {
    pub pin: String,
}

/// POST /api/v1/auth/pin — exchange the 6-digit physical kiosk operator PIN
/// for a short-lived operator session token accepted by `RequireRpcToken`.
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

### 1.2 Identified Deficiencies
1. **No Failure Counting**: Neither failures nor attempts are tracked. An attacker can submit millions of PIN requests without penalty.
2. **Missing Client Identification**: The handler extracts only `State` and `Json<PinUnlockRequest>`, completely ignoring the client origin (IP address, proxy headers, or socket peer).
3. **No Lockout Mechanism**: There is no state representing lockout expiration or remaining attempt counts.
4. **Non-Compliant Error Response**: Returns static `{"ok": false, "error": "invalid_pin", "detail": "Incorrect 6-digit operator PIN."}` without `remaining_attempts`, and never produces `429 Too Many Requests` or `Retry-After`.

---

## 2. Current `AppState` Architecture in `boss/src/state.rs`

### 2.1 State Sharing Pattern
In `boss/src/state.rs`:
```rust
pub type SharedState = Arc<AppState>;
```
`AppState` is instantiated once on startup in `boss/src/main.rs`:
```rust
let config = Config::from_env();
let state = Arc::new(AppState::new(config));
```
And supplied to the Axum router via `.with_state(state)`.

### 2.2 Concurrency Primitives Used in `AppState`
Existing fields in `AppState` demonstrate two synchronization styles:
- **`tokio::sync::RwLock`**: Used for read-heavy state collections:
  - `nodes: RwLock<HashMap<String, (NodeTelemetry, u64)>>`
  - `commands: RwLock<HashMap<String, CommandRecord>>`
  - `pending_commands: RwLock<HashMap<String, VecDeque<String>>>`
  - `operator_sessions: RwLock<HashMap<String, u64>>`
  - `recent_errors: RwLock<VecDeque<ErrorEvent>>`
  - `recent_scores: RwLock<VecDeque<HighScoreEntry>>`
  - `recent_transactions: RwLock<VecDeque<TokenTransaction>>`
- **`tokio::sync::Mutex`**: Used for mutable work queues:
  - `edge_buffer: Mutex<VecDeque<EdgeItem>>`

### 2.3 Integration of `PinAttemptTracker`
Because every incoming PIN verification request must check and/or mutate attempt records (checking lockout status, updating attempt timestamps, incrementing failure counts, or removing client state on success), read-only passes are negligible. 

Adding:
```rust
pub pin_tracker: Mutex<PinAttemptTracker>,
```
directly into `AppState` provides thread-safe, lock-order-safe, and async-safe mutable access across Tokio worker threads.

---

## 3. Client Identification Strategy

### 3.1 Network Topologies in B.E.E.R.C.A.D.E.
Requests hitting `POST /api/v1/auth/pin` originate from multiple distinct contexts:
1. **Localhost Kiosk UI (`/`)**: SvelteKit touch kiosk running locally on the cabinet hardware or mini-PC via loopback (`127.0.0.1`).
2. **Venue LAN Operator Devices**: Staff tablets/laptops navigating directly to `http://<boss-ip>:8989/admin` from the local subnet (e.g., `192.168.1.50`).
3. **Cloudflare Tunnel / Reverse Proxy**: Remote supervisory connections forwarded through cloud tunnels or reverse proxies (providing headers `CF-Connecting-IP`, `X-Forwarded-For`, or `X-Real-IP`).
4. **Automated Unit & Integration Test Suites**: Test harnesses using in-process `tower::ServiceExt::oneshot` where `ConnectInfo` is often absent unless explicitly injected.

### 3.2 Robust Extractor Design: `extract_client_ip`
To ensure that client identification is infallible, never panics, and prioritizes proxy headers while falling back to direct TCP socket peer and default loopback, we define an extraction utility:

```rust
pub fn extract_client_ip(
    headers: &axum::http::HeaderMap,
    peer_addr: Option<std::net::SocketAddr>,
) -> String {
    // 1. Cloudflare Edge Connecting IP
    if let Some(cf_ip) = headers.get("CF-Connecting-IP").and_then(|v| v.to_str().ok()) {
        let trimmed = cf_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    // 2. Standard X-Forwarded-For (take the first client IP in comma-separated list)
    if let Some(xff) = headers.get("X-Forwarded-For").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    // 3. X-Real-IP
    if let Some(real_ip) = headers.get("X-Real-IP").and_then(|v| v.to_str().ok()) {
        let trimmed = real_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    // 4. Direct TCP Peer Socket Address (from ConnectInfo)
    if let Some(addr) = peer_addr {
        return addr.ip().to_string();
    }

    // 5. Fallback for test harnesses and direct local callers without headers
    "127.0.0.1".to_string()
}
```

### 3.3 Handler Signature in `boss/src/routes.rs`
Axum 0.7 supports optional `ConnectInfo` via `Option<axum::extract::ConnectInfo<SocketAddr>>`. In `boss/src/main.rs`, the router can be served with `.into_make_service_with_connect_info::<SocketAddr>()`:
```rust
pub async fn verify_operator_pin(
    State(state): State<SharedState>,
    headers: HeaderMap,
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    Json(req): Json<PinUnlockRequest>,
) -> Response
```
- In real TCP environments: `connect_info` supplies the peer `SocketAddr`.
- In reverse proxy environments: `headers` take precedence.
- In unit tests calling `verify_operator_pin` or `app.oneshot(req)`: if `connect_info` is `None` and headers are omitted, it gracefully falls back without throwing `ExtensionNotFound` 500 errors.

---

## 4. Lockout Mechanics & Data Model

### 4.1 Constants
- `PIN_MAX_FAILED_ATTEMPTS: u32 = 5`
- `PIN_LOCKOUT_SECS: u64 = 60`
- `PIN_ATTEMPT_WINDOW_MS: u64 = 60_000` (or `300_000` for 5-minute rolling window; 60 seconds is standard)
- `MAX_TRACKED_CLIENTS: usize = 4_096` (bounded RAM protection against memory exhaustion)

### 4.2 Data Structures in `boss/src/state.rs`
```rust
#[derive(Debug, Clone)]
pub struct ClientPinAttempt {
    pub failed_count: u32,
    pub last_attempt_epoch_ms: u64,
    pub locked_until_epoch_ms: Option<u64>,
}

impl ClientPinAttempt {
    pub fn is_locked(&self, now: u64) -> bool {
        match self.locked_until_epoch_ms {
            Some(until) => now < until,
            None => false,
        }
    }

    pub fn retry_after_secs(&self, now: u64) -> u64 {
        match self.locked_until_epoch_ms {
            Some(until) if until > now => {
                let diff_ms = until - now;
                // Ceil division: 1ms..1000ms -> 1s
                (diff_ms + 999) / 1000
            }
            _ => 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct PinAttemptTracker {
    pub attempts: HashMap<String, ClientPinAttempt>,
}
```

### 4.3 Outcome Enum for Evaluation
```rust
pub enum PinEvaluationResult {
    /// Client is currently locked out
    LockedOut { retry_after_secs: u64 },
    /// Invalid PIN, but client is not yet locked out
    BadPin { remaining_attempts: u32 },
    /// 5th failed attempt: triggers lockout immediately
    TriggeredLockout { retry_after_secs: u64 },
}
```

### 4.4 Lifecycle & State Transitions
1. **Pre-Check (Is Client Already Locked Out?)**:
   - Before executing `constant_time_eq` on the PIN candidate, check `tracker.is_client_locked(client_ip, now)`.
   - If locked:
     - Return `429 Too Many Requests` immediately.
     - Header: `Retry-After: <retry_after_secs>`.
     - JSON body:
       ```json
       {
         "ok": false,
         "error": "rate_limited",
         "detail": "Too many failed attempts. Operator PIN unlock locked out.",
         "retry_after_secs": <retry_after_secs>
       }
       ```
     - Candidate PIN is NOT evaluated. Attackers cannot even test PIN validity while locked out.
   - If the lockout duration has elapsed (`now >= locked_until`):
     - The lockout is cleared, `failed_count` is reset to 0, and the client may attempt again.

2. **Candidate Verification**:
   - Compare `constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes())`.

3. **Success Path (PIN Correct)**:
   - Call `tracker.record_success(client_ip)`. This removes or resets the client record in `attempts`.
   - Call `state.mint_operator_session().await`.
   - Return `200 OK`:
     ```json
     {
       "ok": true,
       "token": "<token>",
       "expires_at_epoch_ms": <expires_at_epoch_ms>,
       "venue": "<venue_name>"
     }
     ```

4. **Failure Path (PIN Incorrect)**:
   - Call `tracker.record_failure(client_ip, now, PIN_ATTEMPT_WINDOW_MS, PIN_MAX_FAILED_ATTEMPTS, PIN_LOCKOUT_SECS)`.
   - If prior failures happened outside `PIN_ATTEMPT_WINDOW_MS`, reset `failed_count` to 0.
   - Increment `failed_count += 1`.
   - If `failed_count >= 5`:
     - Set `locked_until_epoch_ms = Some(now + PIN_LOCKOUT_SECS * 1000)`.
     - Return `429 Too Many Requests`.
     - Header: `Retry-After: 60`.
     - JSON body:
       ```json
       {
         "ok": false,
         "error": "rate_limited",
         "detail": "Too many failed attempts. Operator PIN unlock locked out.",
         "retry_after_secs": 60
       }
       ```
   - Else (`failed_count < 5`):
     - `remaining_attempts = 5 - failed_count`.
     - Return `401 Unauthorized`.
     - JSON body:
       ```json
       {
         "ok": false,
         "error": "invalid_pin",
         "detail": "Incorrect 6-digit operator PIN.",
         "remaining_attempts": <remaining_attempts>
       }
       ```

---

## 5. Response Schemas & HTTP Header Specification

| Condition | Status Code | HTTP Headers | Response Body |
|---|---|---|---|
| **Success** | `200 OK` | `Content-Type: application/json` | `{"ok": true, "token": "...", "expires_at_epoch_ms": 1730000000, "venue": "..."}` |
| **Bad PIN (<5)** | `401 Unauthorized` | `Content-Type: application/json` | `{"ok": false, "error": "invalid_pin", "detail": "Incorrect 6-digit operator PIN.", "remaining_attempts": 4}` |
| **5th Bad PIN** | `429 Too Many Requests` | `Content-Type: application/json`<br>`Retry-After: 60` | `{"ok": false, "error": "rate_limited", "detail": "Too many failed attempts. Operator PIN unlock locked out.", "retry_after_secs": 60}` |
| **Attempt While Locked Out** | `429 Too Many Requests` | `Content-Type: application/json`<br>`Retry-After: <secs>` | `{"ok": false, "error": "rate_limited", "detail": "Too many failed attempts. Operator PIN unlock locked out.", "retry_after_secs": <secs>}` |

---

## 6. Thread Safety, Concurrency, and Timestamp Decisions

### 6.1 `tokio::sync::Mutex` vs `RwLock` vs `std::sync::Mutex`
- **Why `tokio::sync::Mutex` is Selected**:
  1. **Lock Duration**: The lock is held exclusively for in-memory operations: calculating time, hashmap lookup, integer increment, and timestamp assignment. No I/O, no network calls, and no nested `.await` points occur inside the critical section.
  2. **Consistency**: `AppState` already uses `tokio::sync::Mutex` for `edge_buffer`.
  3. **No Lock Contention**: PIN verification is an infrequent event (patron unlocking touch kiosk or operator administrative login). Contention between distinct client IPs is negligible.
  4. **Deadlock Free**: The lock is taken and immediately released within helper methods on `AppState`, completely isolated from other locks.

### 6.2 Epoch Milliseconds (`now_epoch_ms()`) vs Monotonic `Instant`
- `AppState` uniformly employs `now_epoch_ms()` for node staleness (`STALENESS_MS`), operator session expiration (`OPERATOR_SESSION_TTL_MS`), command lifecycle timestamps, and error events.
- Using `now_epoch_ms()` enables exact calculation of remaining lockout time in seconds, clean serialization into `retry_after_secs: u64`, and seamless testing with mockable or elapsed timestamps.
- All arithmetic utilizes `.saturating_sub()` to protect against any edge-case backwards system clock adjustments.

### 6.3 Memory Boundedness & DoS Protection
To prevent malicious clients from filling server memory with millions of spoofed IP records:
- The tracker maintains a capacity limit (e.g., `MAX_TRACKED_CLIENTS = 4096`).
- An eviction routine purges records where both the attempt window has expired and any active lockout has concluded:
  ```rust
  if tracker.attempts.len() >= MAX_TRACKED_CLIENTS {
      tracker.attempts.retain(|_, record| {
          record.is_locked(now) || (now.saturating_sub(record.last_attempt_epoch_ms) < PIN_ATTEMPT_WINDOW_MS)
      });
  }
  ```

---

## 7. Concrete Code Designs

### 7.1 Methods to Add to `boss/src/state.rs`
```rust
pub const PIN_MAX_FAILED_ATTEMPTS: u32 = 5;
pub const PIN_LOCKOUT_SECS: u64 = 60;
pub const PIN_ATTEMPT_WINDOW_MS: u64 = 60_000;

#[derive(Debug, Clone)]
pub struct ClientPinAttempt {
    pub failed_count: u32,
    pub last_attempt_epoch_ms: u64,
    pub locked_until_epoch_ms: Option<u64>,
}

impl ClientPinAttempt {
    pub fn is_locked(&self, now: u64) -> bool {
        match self.locked_until_epoch_ms {
            Some(until) => now < until,
            None => false,
        }
    }

    pub fn retry_after_secs(&self, now: u64) -> u64 {
        match self.locked_until_epoch_ms {
            Some(until) if until > now => (until - now + 999) / 1000,
            _ => 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct PinAttemptTracker {
    pub attempts: HashMap<String, ClientPinAttempt>,
}

impl PinAttemptTracker {
    pub fn check_lockout(&mut self, client_id: &str, now: u64) -> Option<u64> {
        if let Some(entry) = self.attempts.get_mut(client_id) {
            if entry.is_locked(now) {
                return Some(entry.retry_after_secs(now));
            } else if entry.locked_until_epoch_ms.is_some() {
                // Lockout expired: reset client entry
                entry.failed_count = 0;
                entry.locked_until_epoch_ms = None;
            }
        }
        None
    }

    pub fn record_failure(&mut self, client_id: &str, now: u64) -> (bool, u32, u64) {
        if self.attempts.len() > 4000 {
            self.attempts.retain(|_, e| e.is_locked(now) || (now.saturating_sub(e.last_attempt_epoch_ms) < PIN_ATTEMPT_WINDOW_MS));
        }

        let entry = self.attempts.entry(client_id.to_string()).or_insert(ClientPinAttempt {
            failed_count: 0,
            last_attempt_epoch_ms: now,
            locked_until_epoch_ms: None,
        });

        // If outside window and not locked, reset failure counter
        if now.saturating_sub(entry.last_attempt_epoch_ms) > PIN_ATTEMPT_WINDOW_MS && entry.locked_until_epoch_ms.is_none() {
            entry.failed_count = 0;
        }

        entry.failed_count += 1;
        entry.last_attempt_epoch_ms = now;

        if entry.failed_count >= PIN_MAX_FAILED_ATTEMPTS {
            let lockout_until = now + (PIN_LOCKOUT_SECS * 1000);
            entry.locked_until_epoch_ms = Some(lockout_until);
            (true, 0, PIN_LOCKOUT_SECS)
        } else {
            let remaining = PIN_MAX_FAILED_ATTEMPTS - entry.failed_count;
            (false, remaining, 0)
        }
    }

    pub fn reset(&mut self, client_id: &str) {
        self.attempts.remove(client_id);
    }
}
```

And in `AppState`:
```rust
pub struct AppState {
    pub config: Config,
    pub nodes: RwLock<HashMap<String, (NodeTelemetry, u64)>>,
    pub commands: RwLock<HashMap<String, CommandRecord>>,
    pub pending_commands: RwLock<HashMap<String, VecDeque<String>>>,
    pub buses: HashMap<SseChannel, broadcast::Sender<BusMessage>>,
    pub edge_buffer: Mutex<VecDeque<EdgeItem>>,
    pub operator_sessions: RwLock<HashMap<String, u64>>,
    pub recent_errors: RwLock<VecDeque<ErrorEvent>>,
    pub recent_scores: RwLock<VecDeque<HighScoreEntry>>,
    pub recent_transactions: RwLock<VecDeque<TokenTransaction>>,
    pub plugins: Arc<PluginRegistry>,
    pub ui_theme: RwLock<String>,
    pub pin_tracker: Mutex<PinAttemptTracker>,
}
```

### 7.2 Updated Handler in `boss/src/routes.rs`
```rust
pub async fn verify_operator_pin(
    State(state): State<SharedState>,
    headers: HeaderMap,
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    Json(req): Json<PinUnlockRequest>,
) -> Response {
    let client_ip = extract_client_ip(&headers, connect_info.map(|ci| ci.0));
    let now = now_epoch_ms();

    // 1. Check if client is already locked out
    {
        let mut tracker = state.pin_tracker.lock().await;
        if let Some(retry_after_secs) = tracker.check_lockout(&client_ip, now) {
            tracing::warn!(client_ip = %client_ip, retry_after_secs, "Rejected PIN attempt: client locked out");
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, retry_after_secs.to_string())],
                Json(json!({
                    "ok": false,
                    "error": "rate_limited",
                    "detail": format!("Too many failed attempts. Operator PIN unlock locked out. Retry after {retry_after_secs}s."),
                    "retry_after_secs": retry_after_secs
                })),
            ).into_response();
        }
    }

    // 2. Validate candidate PIN
    let candidate = req.pin.trim();
    if constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes()) {
        // Success: clear failures for this client
        {
            let mut tracker = state.pin_tracker.lock().await;
            tracker.reset(&client_ip);
        }

        let (token, expires_at_epoch_ms) = state.mint_operator_session().await;
        tracing::info!(client_ip = %client_ip, "Operator kiosk session unlocked via PIN");
        (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "token": token,
                "expires_at_epoch_ms": expires_at_epoch_ms,
                "venue": state.config.venue_name,
            })),
        ).into_response()
    } else {
        // Failure: increment failure count & check lockout threshold
        let (locked_out, remaining_attempts, retry_after_secs) = {
            let mut tracker = state.pin_tracker.lock().await;
            tracker.record_failure(&client_ip, now)
        };

        if locked_out {
            tracing::warn!(client_ip = %client_ip, "Operator PIN lockout threshold reached (5 failures)");
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, retry_after_secs.to_string())],
                Json(json!({
                    "ok": false,
                    "error": "rate_limited",
                    "detail": "Too many failed attempts. Operator PIN unlock locked out.",
                    "retry_after_secs": retry_after_secs
                })),
            ).into_response()
        } else {
            tracing::warn!(client_ip = %client_ip, remaining_attempts, "Rejected invalid operator PIN attempt");
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "ok": false,
                    "error": "invalid_pin",
                    "detail": "Incorrect 6-digit operator PIN.",
                    "remaining_attempts": remaining_attempts
                })),
            ).into_response()
        }
    }
}
```

---

## 8. Verification & Test Plan

Per `TEST_INFRA.md` (Feature F5: Operator PIN Rate Limiting), test coverage must span four tiers:

### Tier 1: Unit Tests for `PinAttemptTracker` (`boss/src/state.rs`)
1. `test_initial_state_not_locked`: New client has 0 failures and is not locked.
2. `test_failure_decrements_remaining`: 1st bad attempt returns `remaining_attempts = 4`, 2nd returns 3, 3rd returns 2, 4th returns 1.
3. `test_fifth_failure_triggers_lockout`: 5th bad attempt returns `locked_out = true` with `retry_after_secs = 60`.
4. `test_attempt_during_lockout_returns_remaining_seconds`: Calling `check_lockout` during lockout window returns remaining seconds.
5. `test_successful_unlock_resets_failures`: Valid attempt calls `reset()`, clearing prior failed counts back to clean state.

### Tier 2: Boundary & Timing Tests (`boss/src/state.rs`)
1. `test_lockout_expiration_lifts_ban`: Simulating time advancing past 60s (+60,001 ms) clears lockout and allows new attempts.
2. `test_window_expiration_resets_counter`: Simulating time advancing past attempt window without reaching 5 failures resets `failed_count`.
3. `test_max_tracked_clients_eviction`: Exceeding 4,000 clients successfully evicts inactive entries without memory leakage or panic.

### Tier 3: Client Isolation & Header Priority Tests (`boss/src/routes.rs`)
1. `test_client_isolation`: Client A (`10.0.0.1`) fails 5 times and is locked out; Client B (`10.0.0.2`) on the same server can still unlock successfully with 200 OK.
2. `test_extract_client_ip_precedence`: Verify `CF-Connecting-IP` > `X-Forwarded-For` > `X-Real-IP` > Socket address > Default fallback.

### Tier 4: Real-World HTTP Integration Scenario (`boss/src/routes.rs` / `boss/tests/`)
1. **Scenario 3 Execution**:
   - Send `POST /api/v1/auth/pin` with correct PIN `898900` -> `200 OK`, JSON has `token` and `venue`.
   - Send 4 bad PIN requests from IP `192.168.1.50` -> All return `401 Unauthorized` with `remaining_attempts` decreasing from 4 to 1.
   - Send 5th bad PIN request from IP `192.168.1.50` -> Returns `429 Too Many Requests`, header `Retry-After: 60`, `retry_after_secs: 60`.
   - Send 6th request (even with valid PIN `898900`) from IP `192.168.1.50` -> Returns `429 Too Many Requests`.
   - Send request with valid PIN `898900` from separate IP `192.168.1.51` -> Returns `200 OK` (independent client unaffected).
