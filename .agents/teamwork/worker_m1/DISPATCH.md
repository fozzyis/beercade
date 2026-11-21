# Dispatch for worker_m1

- Role: Rust Systems Worker (Shared Models & Thermal Contract - Milestone 1)
- Working Directory: d:\data\dev\beercade\.agents\teamwork\worker_m1
- Scope: Requirement R4 (Clean Telemetry Contract for Thermals across beercade-shared, astro-daemon, and boss)
- Project Scope Document: d:\data\dev\beercade\.agents\teamwork\PROJECT.md
- Requirements Document: d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md
- Analysis References:
  - d:\data\dev\beercade\.agents\teamwork\explorer_survey_2\analysis.md (Section 2)
  - d:\data\dev\beercade\.agents\teamwork\explorer_survey_3\analysis.md (Section 3.3)

Exclusive File Ownership:
- `shared/crates/beercade-shared/src/models.rs`
- `astro/daemon/src/netplay.rs`
- `astro/daemon/src/main.rs` (telemetry section lines 131-160)
- `boss/src/plugins.rs` (thermal alert plugin lines 2254-2279)

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

STRICTLY FORBIDDEN:
Do NOT run image builds (make image-x86, mksquashfs, Docker rootfs exports). Keep scope strictly on code and tests.


## 2026-10-27T14:01:47Z
You are worker_m1 (Thermal Contract Worker).
Your working directory is: d:\data\dev\beercade\.agents\teamwork\worker_m1
Read d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md, d:\data\dev\beercade\.agents\teamwork\PROJECT.md, and d:\data\dev\beercade\.agents\teamwork\worker_m1\DISPATCH.md before starting work.
Refer to detailed findings in d:\data\dev\beercade\.agents\teamwork\explorer_survey_2\analysis.md (Section 2) and d:\data\dev\beercade\.agents\teamwork\explorer_survey_3\analysis.md (Section 3.3).

Mission: Implement Milestone 1 - Requirement R4: Clean Telemetry Contract for Thermals across beercade-shared, astro-daemon, and boss.

Exclusive File Ownership:
- `shared/crates/beercade-shared/src/models.rs`
- `astro/daemon/src/netplay.rs`
- `astro/daemon/src/main.rs` (telemetry section lines 131-160)
- `boss/src/plugins.rs` (thermal alert plugin lines 2254-2279)

Tasks:
1. In `shared/crates/beercade-shared/src/models.rs`:
   - Change `cpu_temp_c` and `gpu_temp_c` in `NodeTelemetry` from `f32` to `Option<f32>` with `#[serde(default)]`.
   - Update `NodeTelemetry::validate()`: accept `None` as valid (`Ok(())`). If `Some(temp)`, reject if NaN or outside `(-20.0..=120.0)`.
   - Add unit tests in `models.rs` testing:
     - `validate()` with `None` thermals -> `Ok(())`
     - `validate()` with `Some(valid)` -> `Ok(())`
     - `validate()` with `Some(out_of_range)` -> `Err`
     - Serde deserialization from JSON with explicit numbers, `null`, and omitted fields.
2. In `astro/daemon/src/netplay.rs`:
   - Update `sample_temps()` to return `(Option<f32>, Option<f32>)`. Do not duplicate CPU temp if GPU sensor is missing (`None`).
   - Update watchdog thermal checks and `NetplayHealthReport` to work cleanly with `Option<f32>`.
3. In `astro/daemon/src/main.rs`:
   - Remove the artificial 42.0°C clamp in `sample_telemetry()` (lines 133-138). Pass `(cpu_temp_c, gpu_temp_c)` directly into `NodeTelemetry`.
4. In `boss/src/plugins.rs`:
   - In `PluginEvent::TelemetryHeartbeat`, compute `max_temp` across `Option<f32>` safely without assuming scalar primitives. Format alerts as "N/A" if sensor reading is `None`.
5. Run builds and tests:
   - Run `cargo test -p beercade-shared`
   - Run `cargo test -p astro-daemon`
   - Run `cargo test -p boss-server`
   - Verify that all tests pass cleanly with zero compiler errors.
6. Documentation & Handoff:
   - Write summary of changes to d:\data\dev\beercade\.agents\teamwork\worker_m1\changes.md
   - Write handoff report to d:\data\dev\beercade\.agents\teamwork\worker_m1\handoff.md
   - Report completion and test results back to orchestrator via send_message.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

STRICTLY FORBIDDEN:
Do NOT run image builds (make image-x86, mksquashfs, Docker rootfs exports). Keep scope strictly on code and tests.


## 2026-10-27T14:19:05Z
URGENT USER INSTRUCTION: Cease work briefly until user gives the go ahead. Pause all active worker runs and edits immediately. Do not modify files or run tests. Stand by.
