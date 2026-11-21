# Architectural Analysis Report: R4 (Thermal Telemetry Contract) & R5 (B.O.S.S. Operator PIN Rate Limiting)

**Date**: 2026-10-27  
**Author**: explorer_survey_2 (Boss & Telemetry Explorer)  
**Target Milestone**: RC1 Core Robustness & Security Fixes  
**Status**: Completed (Read-Only Investigation)  

---

## 1. Executive Summary

This report delivers a comprehensive code-level analysis and implementation blueprint for Requirements **R4** (Clean Telemetry Contract for Thermals) and **R5** (B.O.S.S. Operator PIN Brute-Force Rate Limiting) in the B.E.E.R.C.A.D.E. workspace.

1. **R4 (Clean Telemetry Contract)**:
   - **Root Cause**: The artificial fallback value `42.0°C` is clamped in `astro/daemon/src/main.rs` (lines 134 and 137). It was introduced because `beercade-shared::models::NodeTelemetry` declared `cpu_temp_c: f32` and `gpu_temp_c: f32` as non-nullable primitives, and `NodeTelemetry::validate()` strictly rejected any value outside `-20.0..=120.0` (including `f32::NAN`). When hardware sensors were missing (such as on standard x86 dev machines or VMs), the daemon clamped thermals to 42.0°C to prevent B.O.S.S. from rejecting heartbeats with HTTP `422 Unprocessable Entity`.
   - **Solution**: Refactor `cpu_temp_c` and `gpu_temp_c` in `beercade-shared::models::NodeTelemetry` to `Option<f32>` with `#[serde(default)]`. Update `NodeTelemetry::validate()` so that `None` values pass sanity validation without error. Remove the 42.0°C clamp in `astro/daemon/src/main.rs`, update `astro/daemon/src/netplay.rs` to yield `Option<f32>`, and update `boss/src/plugins.rs` to compute thermal warnings safely over optional values.

2. **R5 (Operator PIN Rate Limiting)**:
   - **Vulnerability**: `boss/src/routes.rs` (`verify_operator_pin`, lines 142–172) validates candidate PINs using constant-time equality against `state.config.operator_pin`, but has **zero rate limiting, attempt counters, or lockout state**. An attacker on the local network (LAN) can brute-force all 1,000,000 possible 6-digit PIN combinations in minutes.
   - **Solution**: Introduce a `PinAttemptTracker` in `boss/src/state.rs` managed under a thread-safe `RwLock<HashMap<String, PinAttemptTracker>>` keyed by client IP/origin. Enforce a threshold of **5 consecutive failed attempts**, triggering a **60-second lockout window** with a **10-minute failure decay window**. Locked requests immediately return HTTP `429 Too Many Requests` with a `Retry-After: <seconds>` header and a JSON payload compatible with `boss/kiosk_ui/src/lib/bossApi.ts` and `PinKeypadModal.svelte`.

---

## 2. Requirement R4: Clean Telemetry Contract for Thermals

### 2.1 Trace of Current Data Structures & Sensor Pipeline

#### 1. Hardware Abstraction Layer (`beercade-hal`)
- **File**: `shared/crates/beercade-hal/src/lib.rs` (lines 61–66)
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thermals {
    pub cpu_c: f32,
    /// `None` when the board exposes no separate GPU sensor.
    pub gpu_c: Option<f32>,
}
```
- In `Platform::read_thermals_at(&self, root: &Path) -> Result<Thermals, HalError>` (lines 105–124):
  - On x86_64, probes `/sys/class/hwmon` for `coretemp`, `k10temp`, or `zenpower`. If none exist, returns `Err(HalError::NoSensor)`. Probes for discrete GPU (`amdgpu`, `i915`, `xe`, `nouveau`), setting `gpu_c` to `Option<f32>`.
  - On ARM (RK3588, BCM2712, BCM2711), reads `thermal_zone0` (CPU) and optionally `thermal_zone1` (GPU).
  - **Crucial Observation**: `beercade-hal` **never clamps to 42.0°C**. It returns a clean `Result<Thermals, HalError>`.

#### 2. Netplay Sensor Sampler (`astro-daemon::netplay`)
- **File**: `astro/daemon/src/netplay.rs` (lines 1634–1642)
```rust
pub fn sample_temps() -> (f32, f32) {
    match beercade_hal::Platform::detect().read_thermals() {
        Ok(t) => (t.cpu_c, t.gpu_c.unwrap_or(t.cpu_c)),
        Err(e) => {
            warn!(error = %e, "thermal read failed");
            (f32::NAN, f32::NAN)
        }
    }
}
```
- **Flaws Observed**:
  1. If `Platform::detect().read_thermals()` fails (no hwmon sensor), it returns `(f32::NAN, f32::NAN)`.
  2. If the platform has no discrete GPU sensor (e.g. Raspberry Pi 5 or integrated x86), `t.gpu_c.unwrap_or(t.cpu_c)` artificially duplicates the CPU reading as the GPU reading, fabricating false GPU telemetry.

#### 3. Daemon Telemetry Loop (`astro-daemon::main`)
- **File**: `astro/daemon/src/main.rs` (lines 131–160)
```rust
fn sample_telemetry(state: &DaemonSharedState) -> NodeTelemetry {
    let (mut cpu_temp, mut gpu_temp) = netplay::sample_temps();
    if cpu_temp.is_nan() || !(-20.0..=120.0).contains(&cpu_temp) {
        cpu_temp = 42.0;
    }
    if gpu_temp.is_nan() || !(-20.0..=120.0).contains(&gpu_temp) {
        gpu_temp = 42.0;
    }
...
```
- **Exact Clamp Location**: Lines 134 and 137 in `astro/daemon/src/main.rs`.
- **Reason**: The developer encountered validation rejection in B.O.S.S. when running on machines without temperature sensors and patched over it with the constant `42.0`.

#### 4. Shared Models & Ingest Validation (`beercade-shared`)
- **File**: `shared/crates/beercade-shared/src/models.rs` (lines 76–127)
```rust
pub struct NodeTelemetry {
    pub node_id: String,
    pub hardware: String,
    pub soc: String,
    pub cpu_temp_c: f32,
    pub gpu_temp_c: f32,
    ...
}

impl NodeTelemetry {
    pub fn validate(&self) -> Result<(), &'static str> {
        ...
        if !(-20.0..=120.0).contains(&self.cpu_temp_c) {
            return Err("cpu_temp_c out of plausible range");
        }
        if !(-20.0..=120.0).contains(&self.gpu_temp_c) {
            return Err("gpu_temp_c out of plausible range");
        }
        ...
    }
}
```
- Because Rust's `RangeInclusive::contains(&f32::NAN)` returns `false`, any `f32::NAN` fails validation.

#### 5. B.O.S.S. Heartbeat Ingest & Plugin Alerting (`boss`)
- **Files**:
  - `boss/src/routes.rs` (lines 70–79):
    ```rust
    pub async fn ingest_telemetry(...) -> StatusCode {
        if let Err(reason) = payload.validate() {
            tracing::warn!(node_id = %payload.node_id, %reason, "rejected invalid telemetry");
            return StatusCode::UNPROCESSABLE_ENTITY; // 422
        }
        ...
    ```
  - `boss/src/ws_telemetry.rs` (lines 26–30):
    ```rust
    if let Ok(payload) = serde_json::from_str::<NodeTelemetry>(&text) {
        if let Err(reason) = payload.validate() {
            tracing::warn!(... "rejected invalid telemetry via WS");
            continue;
        }
    ```
  - `boss/src/plugins.rs` (lines 2254–2256):
    ```rust
    PluginEvent::TelemetryHeartbeat(hb) => {
        let max_temp = hb.cpu_temp_c.max(hb.gpu_temp_c);
        if max_temp >= st.thermal_warn_temp_c { ... }
    ```
    If `hb.cpu_temp_c` and `hb.gpu_temp_c` become `Option<f32>`, line 2255 requires adaptation so it does not fail compilation or cause runtime panics.

---

### 2.2 Proposed Solution & Implementation Blueprint

#### Step 1: Update `NodeTelemetry` in `beercade-shared/src/models.rs`
Change `cpu_temp_c` and `gpu_temp_c` to `Option<f32>` with `#[serde(default)]`:

```rust
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct NodeTelemetry {
    pub node_id: String,
    pub hardware: String,
    pub soc: String,
    #[serde(default)]
    pub cpu_temp_c: Option<f32>,
    #[serde(default)]
    pub gpu_temp_c: Option<f32>,
    #[serde(default)]
    pub npu_load_pct: f32,
    pub active_core: String,
    pub active_rom: String,
    pub frame_time_ms: f32,
    #[serde(default)]
    pub fps: f32,
    #[serde(default)]
    pub input_jitter: InputJitterStats,
    #[serde(default)]
    pub netplay: Option<NetplayStats>,
    #[serde(default)]
    pub credits: CreditCounters,
    #[serde(default)]
    pub mode: serde_json::Value,
    pub timestamp_epoch_ms: u64,
}
```

Update `validate()` in `beercade-shared/src/models.rs`:
```rust
impl NodeTelemetry {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.node_id.trim().is_empty() {
            return Err("node_id must not be empty");
        }
        if self.node_id.len() > 64 {
            return Err("node_id too long (max 64 chars)");
        }
        if let Some(temp) = self.cpu_temp_c {
            if temp.is_nan() || !(-20.0..=120.0).contains(&temp) {
                return Err("cpu_temp_c out of plausible range");
            }
        }
        if let Some(temp) = self.gpu_temp_c {
            if temp.is_nan() || !(-20.0..=120.0).contains(&temp) {
                return Err("gpu_temp_c out of plausible range");
            }
        }
        if !(0.0..=100.0).contains(&self.npu_load_pct) {
            return Err("npu_load_pct must be within 0..=100");
        }
        if self.frame_time_ms < 0.0 || self.frame_time_ms > 10_000.0 {
            return Err("frame_time_ms out of plausible range");
        }
        if let Some(np) = &self.netplay {
            if np.rtt_ms < 0.0 || np.packet_loss_pct < 0.0 || np.packet_loss_pct > 100.0 {
                return Err("netplay stats out of plausible range");
            }
        }
        Ok(())
    }
}
```

#### Step 2: Clean Thermal Sampling in `astro/daemon/src/netplay.rs`
Update `sample_temps()` to return `(Option<f32>, Option<f32>)`:
```rust
pub fn sample_temps() -> (Option<f32>, Option<f32>) {
    match beercade_hal::Platform::detect().read_thermals() {
        Ok(t) => {
            let cpu = if !t.cpu_c.is_nan() && (-20.0..=120.0).contains(&t.cpu_c) {
                Some(t.cpu_c)
            } else {
                None
            };
            let gpu = t.gpu_c.filter(|g| !g.is_nan() && (-20.0..=120.0).contains(g));
            (cpu, gpu)
        }
        Err(e) => {
            warn!(error = %e, "thermal read failed");
            (None, None)
        }
    }
}
```

In `netplay.rs` line 1535 (watchdog thermal evaluation):
```rust
let (cpu, gpu) = sample_temps();
let hot = cpu.map_or(false, |c| c > CPU_TEMP_LIMIT_C) || gpu.map_or(false, |g| g > GPU_TEMP_LIMIT_C);
let cool = cpu.map_or(false, |c| c < CPU_TEMP_HYST_C) && gpu.map_or(true, |g| g < GPU_TEMP_HYST_C);
```
In `netplay.rs` line 1589 (`send_watchdog`):
```rust
async fn send_watchdog(&self, cpu: Option<f32>, gpu: Option<f32>) {
    self.send(Packet::Watchdog {
        state: self.health.as_u8(),
        loss_pct_x100: (self.loss_ewma_pct * 100.0) as u16,
        cpu_temp_x100: cpu.map_or(0, |c| (c * 100.0) as u16),
        gpu_temp_x100: gpu.map_or(0, |g| (g * 100.0) as u16),
    })
    .await;
}
```
In `netplay.rs` line 745 (`NetplayHealthReport`):
Update `pub cpu_temp_c: Option<f32>, pub gpu_temp_c: Option<f32>`.

#### Step 3: Remove 42.0°C Clamp in `astro/daemon/src/main.rs`
Lines 131–160 become:
```rust
fn sample_telemetry(state: &DaemonSharedState) -> NodeTelemetry {
    let (cpu_temp_c, gpu_temp_c) = netplay::sample_temps();
    let (hardware, soc) = beercade_hal::Platform::detect().hardware_info();

    let (active_core, active_rom, frame_time_ms, fps) = if let Some(ref game) = state.active_game {
        let fps = game.measured_fps.unwrap_or(60.0);
        let ft = game.frame_time_ms.unwrap_or(1000.0 / fps);
        (game.core.clone(), game.rom.clone(), ft, fps)
    } else {
        (
            "none".to_string(),
            "idle".to_string(),
            0.0,
            0.0,
        )
    };

    NodeTelemetry {
        node_id: state.node_id.clone(),
        hardware: hardware.to_string(),
        soc: soc.to_string(),
        cpu_temp_c,
        gpu_temp_c,
        npu_load_pct: 0.0,
        active_core,
        active_rom,
        frame_time_ms,
        fps,
        input_jitter: sample_input_jitter(),
        netplay: None,
        credits: state.credits.clone(),
        mode: serde_json::to_value(&state.mode).unwrap_or_default(),
        timestamp_epoch_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    }
}
```

#### Step 4: Adapt `boss/src/plugins.rs` Plugin Event Handling
In `boss/src/plugins.rs` (lines 2254–2279):
```rust
PluginEvent::TelemetryHeartbeat(hb) => {
    let max_temp = match (hb.cpu_temp_c, hb.gpu_temp_c) {
        (Some(c), Some(g)) => Some(c.max(g)),
        (Some(c), None) => Some(c),
        (None, Some(g)) => Some(g),
        (None, None) => None,
    };

    if let Some(max_temp) = max_temp {
        if max_temp >= st.thermal_warn_temp_c {
            let now = now_epoch_ms();
            let last = st
                .last_thermal_alert_by_node
                .get(&hb.node_id)
                .copied()
                .unwrap_or(0);
            // 60-second per-node cooldown
            if now.saturating_sub(last) >= 60_000 {
                st.last_thermal_alert_by_node.insert(hb.node_id.clone(), now);
                let threshold = st.thermal_warn_temp_c;
                let cpu_str = hb.cpu_temp_c.map(|c| format!("{:.1}°C", c)).unwrap_or_else(|| "N/A".to_string());
                let gpu_str = hb.gpu_temp_c.map(|g| format!("{:.1}°C", g)).unwrap_or_else(|| "N/A".to_string());
                Self::record_delivery(
                    &mut st,
                    "thermal",
                    format!("Thermal Warning on {}", hb.node_id),
                    format!(
                        "CPU {} / GPU {} exceeded {:.1}°C threshold",
                        cpu_str, gpu_str, threshold
                    ),
                    &hb.node_id,
                );
            }
        }
    }
}
```

### 2.3 Serialization & Deserialization Compatibility Matrix

| JSON Representation | Serde Deserialization (`Option<f32>`) | `validate()` Result | Notes |
|---|---|---|---|
| `{"cpu_temp_c": 54.2}` | `Some(54.2)` | `Ok(())` | Standard sensor telemetry |
| `{"cpu_temp_c": null}` | `None` | `Ok(())` | Clean explicit absence from web clients |
| `{}` (key omitted) | `None` (via `#[serde(default)]`) | `Ok(())` | Compact payloads from minimal nodes |
| `{"cpu_temp_c": 150.0}` | `Some(150.0)` | `Err("cpu_temp_c out of plausible range")` | Correctly caught abnormal spike |
| `{"cpu_temp_c": "NaN"}` | Deserialization error | Rejected at JSON parse | Serde rejects invalid numeric strings |

---

## 3. Requirement R5: B.O.S.S. Operator PIN Brute-Force Rate Limiting

### 3.1 Vulnerability Analysis of `POST /api/v1/auth/pin`

- **Endpoint**: `boss/src/routes.rs::verify_operator_pin` (lines 142–172)
- **Current Behavior**:
  ```rust
  pub async fn verify_operator_pin(
      State(state): State<SharedState>,
      Json(req): Json<PinUnlockRequest>,
  ) -> Response {
      let candidate = req.pin.trim();
      if constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes()) {
          let (token, expires_at_epoch_ms) = state.mint_operator_session().await;
          ...
      } else {
          tracing::warn!("Rejected invalid operator PIN attempt");
          (StatusCode::UNAUTHORIZED, Json(...)).into_response()
      }
  }
  ```
- **Exploit Scenario**:
  An attacker connected to the venue WiFi/LAN or machine network can issue automated HTTP requests against `http://<boss-ip>:8989/api/v1/auth/pin`.
  - The PIN space is $10^6$ (000000 to 999999).
  - Without rate limiting, a local script sending 2,000 requests/sec cracks the operator PIN in under 8 minutes, granting full supervisory control over all arcade nodes (reboot, launch game, add credits, inject payloads).

### 3.2 State in `boss/src/state.rs`
- Currently, `AppState` only maintains `operator_sessions: RwLock<HashMap<String, u64>>` (token -> expiry timestamp).
- There is no tracking of:
  - Total failed attempts
  - Timestamps of recent failures
  - Lockout windows or active lock status

### 3.3 Proposed Rate Limiting Architecture

#### 1. Rate Limiting Policy
- **Maximum Consecutive Failures**: `5` attempts.
- **Lockout Duration**: `60,000` ms (60 seconds).
- **Failure Decay / Reset Window**: `600,000` ms (10 minutes). If no failures occur for 10 minutes, previous failed attempts are forgiven.
- **Immediate Reset on Success**: When the correct PIN is provided (prior to lockout), the failure counter for that origin is immediately reset to 0.
- **Locked State Rejection**: While lockout is active, **all** PIN submissions from that origin (even if the candidate PIN is correct) are rejected immediately with HTTP `429 Too Many Requests`.

#### 2. Keying & Client Origin Identification
To prevent a malicious host on the LAN from locking out the physical touchscreen kiosk, attempts are tracked per client origin:
```rust
let client_ip = headers
    .get("CF-Connecting-IP")
    .or_else(|| headers.get("X-Forwarded-For"))
    .or_else(|| headers.get("X-Real-IP"))
    .and_then(|h| h.to_str().ok())
    .map(|s| s.split(',').next().unwrap_or(s).trim())
    .unwrap_or("direct");
```
- Physical touch kiosk running locally/direct maps to `"direct"` or `"127.0.0.1"`.
- Remote LAN or tunnel clients map to their respective IPs.

#### 3. State Data Model (`boss/src/state.rs`)
```rust
#[derive(Debug, Clone, Default)]
pub struct PinAttemptTracker {
    pub consecutive_failures: u32,
    pub last_failure_epoch_ms: u64,
    pub locked_until_epoch_ms: u64,
}

pub const MAX_PIN_FAILURES: u32 = 5;
pub const PIN_LOCKOUT_DURATION_MS: u64 = 60_000;      // 60 seconds
pub const PIN_FAILURE_DECAY_MS: u64 = 10 * 60 * 1_000; // 10 minutes
```

Add to `AppState`:
```rust
pub struct AppState {
    ...
    /// Rate limiter for operator PIN unlock: client_key -> PinAttemptTracker
    pub pin_rate_limiter: RwLock<HashMap<String, PinAttemptTracker>>,
    ...
}
```

Add helper methods to `AppState`:
```rust
impl AppState {
    /// Check if the given client origin is currently locked out.
    /// Returns Ok(()) if allowed, or Err(remaining_secs) if locked.
    pub async fn check_pin_rate_limit(&self, client_key: &str) -> Result<(), u64> {
        let now = now_epoch_ms();
        let limiter = self.pin_rate_limiter.read().await;
        if let Some(tracker) = limiter.get(client_key) {
            if tracker.locked_until_epoch_ms > now {
                let remaining = (tracker.locked_until_epoch_ms.saturating_sub(now) + 999) / 1000;
                return Err(remaining);
            }
        }
        Ok(())
    }

    /// Record a failed attempt. Returns (remaining_attempts, Option<lockout_duration_secs>).
    pub async fn record_pin_failure(&self, client_key: &str) -> (u32, Option<u64>) {
        let now = now_epoch_ms();
        let mut limiter = self.pin_rate_limiter.write().await;
        
        // Memory hygiene: prune expired entries occasionally if map grows
        if limiter.len() > 1000 {
            limiter.retain(|_, t| t.locked_until_epoch_ms > now || now.saturating_sub(t.last_failure_epoch_ms) < PIN_FAILURE_DECAY_MS);
        }

        let tracker = limiter.entry(client_key.to_string()).or_default();

        // Decay old failures
        if tracker.last_failure_epoch_ms > 0 && now.saturating_sub(tracker.last_failure_epoch_ms) > PIN_FAILURE_DECAY_MS {
            tracker.consecutive_failures = 0;
            tracker.locked_until_epoch_ms = 0;
        }

        tracker.consecutive_failures += 1;
        tracker.last_failure_epoch_ms = now;

        if tracker.consecutive_failures >= MAX_PIN_FAILURES {
            tracker.locked_until_epoch_ms = now + PIN_LOCKOUT_DURATION_MS;
            let secs = PIN_LOCKOUT_DURATION_MS / 1000;
            (0, Some(secs))
        } else {
            let remaining = MAX_PIN_FAILURES.saturating_sub(tracker.consecutive_failures);
            (remaining, None)
        }
    }

    /// Reset failure tracking on successful PIN unlock.
    pub async fn reset_pin_rate_limit(&self, client_key: &str) {
        let mut limiter = self.pin_rate_limiter.write().await;
        limiter.remove(client_key);
    }
}
```

#### 4. Route Implementation (`boss/src/routes.rs`)
Refactor `verify_operator_pin` to use `HeaderMap`:
```rust
pub async fn verify_operator_pin(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<PinUnlockRequest>,
) -> Response {
    let client_ip = headers
        .get("CF-Connecting-IP")
        .or_else(|| headers.get("X-Forwarded-For"))
        .or_else(|| headers.get("X-Real-IP"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim())
        .unwrap_or("direct");

    // 1. Check if client is already locked out
    if let Err(remaining_secs) = state.check_pin_rate_limit(client_ip).await {
        tracing::warn!(client = %client_ip, remaining_secs, "Rejected PIN attempt: rate limited");
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, remaining_secs.to_string())],
            Json(json!({
                "ok": false,
                "error": "rate_limited",
                "detail": format!("Too many failed attempts. Operator PIN locked for {} seconds.", remaining_secs),
                "retry_after_secs": remaining_secs
            })),
        ).into_response();
    }

    // 2. Validate PIN candidate
    let candidate = req.pin.trim();
    if constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes()) {
        state.reset_pin_rate_limit(client_ip).await;
        let (token, expires_at_epoch_ms) = state.mint_operator_session().await;
        tracing::info!(client = %client_ip, "Operator kiosk session unlocked via PIN");
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
        let (remaining, lockout) = state.record_pin_failure(client_ip).await;
        if let Some(lockout_secs) = lockout {
            tracing::warn!(client = %client_ip, lockout_secs, "Operator PIN lockout triggered");
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, lockout_secs.to_string())],
                Json(json!({
                    "ok": false,
                    "error": "rate_limited",
                    "detail": format!("Too many failed attempts. Operator PIN locked for {} seconds.", lockout_secs),
                    "retry_after_secs": lockout_secs
                })),
            ).into_response()
        } else {
            tracing::warn!(client = %client_ip, remaining, "Rejected invalid operator PIN attempt");
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "ok": false,
                    "error": "invalid_pin",
                    "detail": format!("Incorrect 6-digit operator PIN. {} attempts remaining before lockout.", remaining),
                    "remaining_attempts": remaining
                })),
            ).into_response()
        }
    }
}
```

### 3.4 Frontend Kiosk UI Compatibility
In `boss/kiosk_ui/src/lib/bossApi.ts`:
```typescript
const res = await fetch(`${getApiBase()}/api/v1/auth/pin`, { ... });
const data = await res.json();
if (res.ok && data.ok && data.token) {
    setStoredOperatorToken(data.token, data.expires_at_epoch_ms);
    return data;
}
return { ok: false, detail: data.detail || 'Incorrect 6-digit operator PIN.' };
```
- When `429 Too Many Requests` is returned, `res.ok` is `false`.
- `data.detail` carries `"Too many failed attempts. Operator PIN locked for 60 seconds."`.
- In `PinKeypadModal.svelte`:
  `triggerShake(res.detail || 'Invalid 6-digit operator PIN.');`
  The modal shakes and displays the exact lockout message. No frontend breaking changes occur!

---

## 4. Test Expansion Plan

### 4.1 R4 Automated Tests (Shared & Ingest)
1. **`test_telemetry_validates_none_thermals`** (`beercade-shared`):
   Verify that a `NodeTelemetry` instance with `cpu_temp_c: None, gpu_temp_c: None` passes `validate()` with `Ok(())`.
2. **`test_telemetry_validates_some_thermals`** (`beercade-shared`):
   Verify that normal temperatures (`Some(45.0)`) pass `validate()`, while out-of-range (`Some(135.0)` or `Some(-30.0)`) or `Some(f32::NAN)` return `Err`.
3. **`test_telemetry_json_serde_none_and_null`** (`beercade-shared`):
   Verify that JSON with omitted thermal fields or explicit `null` cleanly deserializes into `None`.
4. **`test_ingest_telemetry_accepts_missing_thermals`** (`boss`):
   Send an HTTP POST to `/api/v1/telemetry` with `{"cpu_temp_c": null, "gpu_temp_c": null}`. Verify HTTP `200 OK` (not `422`).

### 4.2 R5 Automated Tests (Operator PIN Rate Limiting)
1. **`test_operator_pin_success_resets_failures`** (`boss`):
   Submit invalid PIN 3 times (remaining = 2). Submit correct PIN. Verify HTTP 200 and minted token. Submit invalid PIN once more; verify remaining attempts is 4 (confirming full reset).
2. **`test_operator_pin_lockout_threshold`** (`boss`):
   Submit invalid PIN 5 consecutive times. Verify the 5th attempt returns HTTP `429 Too Many Requests` with `Retry-After: 60`.
3. **`test_operator_pin_locked_rejection_even_with_valid_pin`** (`boss`):
   Trigger lockout with 5 invalid attempts. While locked out, submit the **correct** PIN. Verify it is still rejected with HTTP 429 and does not issue a token.
4. **`test_operator_pin_per_client_isolation`** (`boss`):
   Client A (`192.168.1.10`) fails 5 times and is locked. Client B (`192.168.1.20`) submits the correct PIN. Verify Client B receives HTTP 200 and is not affected by Client A's lockout.

---

## 5. Artifact & File Change Map

| Crate | Target File | Nature of Change |
|---|---|---|
| `beercade-shared` | `src/models.rs` | Refactor `cpu_temp_c`/`gpu_temp_c` to `Option<f32>` with `#[serde(default)]`; update `validate()` |
| `astro-daemon` | `src/netplay.rs` | Refactor `sample_temps()` to return `(Option<f32>, Option<f32>)`; update watchdog thermal checks |
| `astro-daemon` | `src/main.rs` | Remove lines 133–138 (42.0°C artificial clamp) in `sample_telemetry()` |
| `boss-server` | `src/state.rs` | Add `PinAttemptTracker`, constants, and rate-limiting helper methods to `AppState` |
| `boss-server` | `src/routes.rs` | Add client IP extraction and lockout checks/reporting in `verify_operator_pin` |
| `boss-server` | `src/plugins.rs` | Adapt `PluginEvent::TelemetryHeartbeat` thermal warning calculation for `Option<f32>` |
| `boss-server` | `src/routes.rs` or `tests/` | Add unit/integration tests for PIN rate limiting |
| `beercade-shared` | `src/models.rs` | Add unit tests for `Option<f32>` validation and serde |
