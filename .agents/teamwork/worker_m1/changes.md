# Summary of Changes — worker_m1 (Requirement R4: Clean Telemetry Contract for Thermals)

## 1. `shared/crates/beercade-shared/src/models.rs`
- **Model Refactoring**: Changed `cpu_temp_c` and `gpu_temp_c` fields on `NodeTelemetry` from `f32` to `Option<f32>` annotated with `#[serde(default)]`.
- **Validation**: Updated `NodeTelemetry::validate()` so that `None` values evaluate to `Ok(())`. If `Some(temp)`, strictly validates that `!temp.is_nan()` and `(-20.0..=120.0).contains(&temp)`.
- **Unit Tests Added**:
  - `test_telemetry_validate_with_none_thermals`: Confirms `None` for CPU and GPU passes validation (`Ok(())`).
  - `test_telemetry_validate_with_valid_thermals`: Confirms typical and boundary temperatures (`-20.0`, `120.0`) pass.
  - `test_telemetry_validate_with_out_of_range_thermals`: Confirms rejection of `> 120.0`, `< -20.0`, and `f32::NAN` for both CPU and GPU.
  - `test_telemetry_serde_deserialization`: Confirms deserialization of explicit floats (`Some(f32)`), explicit JSON `null` (`None`), omitted fields (`None` via default), and partial presence.

## 2. `astro/daemon/src/netplay.rs`
- **Clean Sensor Sampling**: Refactored `sample_temps() -> (Option<f32>, Option<f32>)`. No longer duplicates CPU temperature if GPU sensor is missing (`t.gpu_c` is checked independently; returns `None` when absent).
- **Watchdog Thermal Logic**: Updated `hot` and `cool` evaluation in watchdog loop:
  - `hot = cpu.map_or(false, |c| c > CPU_TEMP_LIMIT_C) || gpu.map_or(false, |g| g > GPU_TEMP_LIMIT_C)`
  - `cool = match (cpu, gpu) { (Some(c), Some(g)) => c < CPU_TEMP_HYST_C && g < GPU_TEMP_HYST_C, (Some(c), None) => c < CPU_TEMP_HYST_C, (None, Some(g)) => g < GPU_TEMP_HYST_C, (None, None) => false }`
- **Watchdog Packet & Telemetry**:
  - Updated `send_watchdog(&self, cpu: Option<f32>, gpu: Option<f32>)` to map `None` to `0` and valid temps to `u16` centidegrees.
  - Updated `NetplayHealthReport` to declare `cpu_temp_c: Option<f32>` and `gpu_temp_c: Option<f32>` with `#[serde(default)]`.

## 3. `astro/daemon/src/main.rs`
- **Removed 42.0°C Clamp**: Deleted artificial fallback clamp lines in `sample_telemetry()` (`cpu_temp = 42.0` / `gpu_temp = 42.0`). Directly passes `(cpu_temp_c, gpu_temp_c)` from `netplay::sample_temps()` into `NodeTelemetry`.

## 4. `boss/src/plugins.rs`
- **Plugin Thermal Alert Handling**:
  - In `PluginEvent::TelemetryHeartbeat`, safely compute `max_temp` across `Option<f32>` (`hb.cpu_temp_c` and `hb.gpu_temp_c`).
  - Formatted alert messages using `"N/A"` if sensor reading is `None` (`format!("CPU {} / GPU {} exceeded {:.1}°C threshold", cpu_str, gpu_str, threshold)`).
- **Unit Tests Added**:
  - `test_thermal_alert_plugin_with_none_thermals`: Verifies no alerts emitted when temperatures are `None`.
  - `test_thermal_alert_plugin_with_normal_thermals`: Verifies no alerts emitted when temperatures are below threshold.
  - `test_thermal_alert_plugin_formats_missing_sensor_as_na`: Verifies formatting produces `"CPU 85.0°C / GPU N/A exceeded ..."` and `"CPU N/A / GPU 88.5°C exceeded ..."`.

## 5. Verification
- `cargo test -p beercade-shared`: 11 passed, 0 failed.
- `cargo test -p astro-daemon`: 10 passed, 0 failed.
- `cargo test -p boss-server`: 7 passed, 0 failed.
- `cargo test --workspace`: All 37 workspace tests passed cleanly.
