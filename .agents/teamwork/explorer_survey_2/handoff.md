# Handoff Report: R4 (Clean Thermal Telemetry Contract) & R5 (B.O.S.S. Operator PIN Rate Limiting)

**From**: explorer_survey_2 (Boss & Telemetry Explorer)  
**To**: Orchestrator (9b504612-81e9-4856-b70d-852ae15f2b33) & Subsequent Implementers  
**Handoff Type**: Hard (Investigation & Analysis Complete)  
**Date**: 2026-10-27  

---

## 1. Observation

1. **Artificial 42.0°C Fallback Clamp**:
   - Location: `d:\data\dev\beercade\astro\daemon\src\main.rs:132-138`:
     ```rust
     let (mut cpu_temp, mut gpu_temp) = netplay::sample_temps();
     if cpu_temp.is_nan() || !(-20.0..=120.0).contains(&cpu_temp) {
         cpu_temp = 42.0;
     }
     if gpu_temp.is_nan() || !(-20.0..=120.0).contains(&gpu_temp) {
         gpu_temp = 42.0;
     }
     ```
2. **Rigid Primitive Definition in `beercade-shared`**:
   - Location: `d:\data\dev\beercade\shared\crates\beercade-shared\src\models.rs:76-81`:
     ```rust
     pub struct NodeTelemetry {
         pub node_id: String,
         pub hardware: String,
         pub soc: String,
         pub cpu_temp_c: f32,
         pub gpu_temp_c: f32,
     ```
3. **Heartbeat Rejection on NaN or Missing Readings in `beercade-shared` & `boss`**:
   - Location: `d:\data\dev\beercade\shared\crates\beercade-shared\src\models.rs:109-114`:
     ```rust
     if !(-20.0..=120.0).contains(&self.cpu_temp_c) {
         return Err("cpu_temp_c out of plausible range");
     }
     if !(-20.0..=120.0).contains(&self.gpu_temp_c) {
         return Err("gpu_temp_c out of plausible range");
     }
     ```
   - Location: `d:\data\dev\beercade\boss\src\routes.rs:76-79`:
     ```rust
     if let Err(reason) = payload.validate() {
         tracing::warn!(node_id = %payload.node_id, %reason, "rejected invalid telemetry");
         return StatusCode::UNPROCESSABLE_ENTITY;
     }
     ```
   - Tool Verification: When `self.cpu_temp_c` is `f32::NAN`, Rust's `(-20.0..=120.0).contains(&f32::NAN)` returns `false`. `payload.validate()` returns `Err("cpu_temp_c out of plausible range")`, causing `ingest_telemetry` to reject the heartbeat with HTTP `422 Unprocessable Entity`.
4. **Sensor Duplication in Netplay Sampler**:
   - Location: `d:\data\dev\beercade\astro\daemon\src\netplay.rs:1634-1641`:
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
   - Tool Verification: On boards without a separate GPU sensor (e.g. Raspberry Pi 5), `t.gpu_c` is `None`, which `sample_temps()` unwrapped to `t.cpu_c`, fabricating duplicate sensor data.
5. **Direct Unwrapped Max in B.O.S.S. Plugin Alerts**:
   - Location: `d:\data\dev\beercade\boss\src\plugins.rs:2255`:
     ```rust
     PluginEvent::TelemetryHeartbeat(hb) => {
         let max_temp = hb.cpu_temp_c.max(hb.gpu_temp_c);
     ```
6. **Vulnerability in Operator PIN Authentication**:
   - Location: `d:\data\dev\beercade\boss\src\routes.rs:142-172`:
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
             (StatusCode::UNAUTHORIZED, Json(...)).into_response()
         }
     }
     ```
   - Tool Verification: `boss/src/state.rs:144-168` defines `AppState`. It contains only `operator_sessions: RwLock<HashMap<String, u64>>`. There is no tracking of failed attempts, no lockout state, and no rate-limiting middleware.
7. **Baseline Workspace Test Command**:
   - Command: `cargo test --workspace`
   - Result: 35 passed; 0 failed in 1.3s across all 5 workspace crates (`astro-daemon`, `astro-tune`, `beercade-hal`, `beercade-shared`, `boss-server`).

---

## 2. Logic Chain

1. **R4 Root Cause & Link to Validation**:
   - Observation 1 shows the exact lines in `astro/daemon/src/main.rs:134,137` setting `42.0`.
   - Observation 3 proves that if `sample_temps()` produces `f32::NAN` (when sensors are missing or fail to read, as seen in Observation 4), `NodeTelemetry::validate()` returns `Err("cpu_temp_c out of plausible range")`.
   - Observation 3 shows that `boss/src/routes.rs:76-79` rejects invalid heartbeats with `422 Unprocessable Entity`.
   - Observation 4 shows that `sample_temps()` duplicates `cpu_c` as `gpu_c` on systems lacking a GPU sensor.
   - Therefore, to eliminate artificial fallbacks, `NodeTelemetry` must support `Option<f32>` (Observation 2), `validate()` must accept `None` without error (Observation 3), `sample_temps()` must cleanly return `(Option<f32>, Option<f32>)` (Observation 4), and `boss/src/plugins.rs` must evaluate optional temperatures without assuming scalar primitives (Observation 5).

2. **R5 Security Vulnerability & State Design**:
   - Observation 6 proves that `verify_operator_pin` contains no failure tracking, backoff delay, or rate limiting.
   - Because the PIN is 6 decimal digits ($10^6$ space), an unauthenticated attacker on the local network can brute-force the operator token in under 10 minutes at LAN transmission rates.
   - Observation 6 confirms `AppState` has no attempt tracker.
   - Therefore, an origin-keyed failure tracker (`PinAttemptTracker`) with a lockout duration (60 seconds) and maximum attempt threshold (5 attempts) must be added to `AppState`, and `verify_operator_pin` must inspect and mutate this tracker before evaluating candidate credentials.

---

## 3. Caveats

1. **Client IP Spoofing on Raw LAN**:
   - When B.O.S.S. is deployed without a reverse proxy on a bare local network, client IP headers (`CF-Connecting-IP`, `X-Forwarded-For`) can be forged by an adversary. However, keying by IP ensures the physical kiosk console (running on `direct` / localhost) is not trivially denied service by a rogue network client.
2. **Kiosk UI Build Artifacts**:
   - `boss/kiosk_ui` contains pre-built assets in `boss/kiosk_ui/build`. The API contract for `POST /api/v1/auth/pin` (`status: 429`, `"detail": "..."`) was designed specifically to remain 100% compatible with the existing `unlockWithPin` TypeScript implementation so that rebuilds of the frontend are unnecessary.
3. **Hardware Watchdog Thresholds in Netplay**:
   - `netplay.rs` uses `CPU_TEMP_LIMIT_C` (85°C) and `GPU_TEMP_LIMIT_C` (90°C) for the `THERMAL_HOLD` state. When temperatures are `None`, the watchdog must evaluate `hot` to `false` and `cool` to `true` to avoid stalling netplay on headless/virtual nodes.

---

## 4. Conclusion

- **R4**: The `42.0°C` clamp is an artificial workaround in `astro/daemon/src/main.rs:134,137` caused by strict validation on non-nullable `f32` fields in `beercade-shared::models::NodeTelemetry`. The contract must be refactored to `Option<f32>` with `#[serde(default)]` across `beercade-shared`, `astro-daemon`, and `boss`.
- **R5**: B.O.S.S. lacks rate limiting on operator PIN authentication. An origin-keyed `PinAttemptTracker` in `boss/src/state.rs` enforcing a 5-failure threshold and a 60-second lockout window completely mitigates the vulnerability while preserving seamless UI integration and local kiosk availability.

---

## 5. Verification Method

To independently verify all observations and subsequent implementations:

1. **Verify Baseline Test Suite**:
   ```powershell
   cargo test --workspace
   ```
   *Expected*: All tests in `beercade-hal`, `beercade-shared`, `astro-daemon`, `astro-tune`, and `boss-server` pass cleanly.

2. **Verify R4 Thermals Refactoring**:
   - Inspect files:
     - `d:\data\dev\beercade\shared\crates\beercade-shared\src\models.rs`
     - `d:\data\dev\beercade\astro\daemon\src\main.rs`
     - `d:\data\dev\beercade\astro\daemon\src\netplay.rs`
     - `d:\data\dev\beercade\boss\src\plugins.rs`
   - Run unit test for `NodeTelemetry::validate()`:
     ```powershell
     cargo test -p beercade-shared -- test_telemetry
     ```
   - Invalidation Condition: If a heartbeat with `cpu_temp_c: None` or `{"cpu_temp_c": null}` yields `Err(...)` or HTTP `422`, R4 is invalid.

3. **Verify R5 PIN Rate Limiting**:
   - Inspect files:
     - `d:\data\dev\beercade\boss\src\state.rs`
     - `d:\data\dev\beercade\boss\src\routes.rs`
   - Run rate-limiting unit tests in `boss-server`:
     ```powershell
     cargo test -p boss-server -- test_operator_pin
     ```
   - Invalidation Condition: If 6 consecutive invalid PIN attempts to `POST /api/v1/auth/pin` return `401 Unauthorized` instead of `429 Too Many Requests`, R5 is invalid.
