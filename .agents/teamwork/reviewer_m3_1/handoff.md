# Reviewer M3-1 Handoff Report: Milestone 3 (A.S.T.R.O. Daemon Robustness & Network)

**Agent**: Reviewer M3-1 (`reviewer_m3_1`)  
**Role**: Reviewer & Adversarial Critic  
**Working Directory**: `d:\data\dev\beercade\.agents\teamwork\reviewer_m3_1`  
**Recipient**: Orchestrator (`bd770878-29b7-48ec-a1e1-b5265681a707`)  
**Target Milestone**: Milestone 3  
**Verdict**: **`APPROVE`**  
**Handoff Type**: Hard (Review Complete)

---

## 1. Observation

1. **Test Suite Execution (`cargo test`)**:
   - Running `cargo test -p astro-daemon` in `d:\data\dev\beercade`:
     ```text
     running 37 tests
     test netplay::tests::crc32_known_vectors ... ok
     test netplay::tests::crc32_incremental_matches_oneshot ... ok
     test netplay::tests::packet_roundtrip_all_variants ... ok
     test netplay::tests::drift_controller_converges_and_is_jury_stable ... ok
     test netplay::tests::synthetic_core_is_deterministic_and_order_sensitive ... ok
     test netplay::tests::ring_buffer_wraparound ... ok
     test tests::test_cmd_ping_execution ... ok
     test tests::test_cmd_reboot_non_blocking ... ok
     test tests::test_cmd_start_and_end_session ... ok
     test tests::test_cmd_launch_game_validation ... ok
     test netplay::tests::rollback_with_history_gap_escalates ... ok
     test tests::test_command_deserialization_and_ack_payload ... ok
     test tests::test_r3_network_config_no_static_ip ... ok
     test netplay::tests::late_input_below_resync_frame_is_ignored ... ok
     test tests::test_r3_static_net_script_no_static_ip ... ok
     test tests::test_credit_fallback_when_both_files_corrupt_no_panic ... ok
     test tests::test_command_ack_dispatch_mock_server ... ok
     test ota::tests::test_ota_staging_rejected_when_sha256_header_missing ... ok
     test ota::tests::test_ota_staging_rejected_when_sha256_header_empty ... ok
     test ota::tests::test_ota_staging_rejected_on_invalid_utf8_header ... ok
     test ota::tests::test_ota_staging_rejected_on_server_error_status ... ok
     test ota::tests::test_ota_staging_cleanly_deletes_preexisting_file_on_missing_header ... ok
     test tests::test_cmd_add_credits_execution ... ok
     test tests::test_atomic_credit_persistence_happy_path ... ok
     test ota::tests::test_ota_staging_rejected_when_payload_is_empty ... ok
     test tests::test_credit_recovery_from_corrupted_garbage_primary ... ok
     test tests::test_credit_recovery_when_primary_deleted ... ok
     test tests::test_credit_recovery_from_truncated_primary ... ok
     test tests::test_credit_recovery_from_zero_byte_primary ... ok
     test ota::tests::test_ota_staging_rejected_on_size_mismatch ... ok
     test ota::tests::test_ota_staging_success_when_hash_matches ... ok
     test ota::tests::test_ota_staging_accepts_uppercase_sha256_header ... ok
     test ota::tests::test_ota_staging_rejected_when_sha256_mismatched ... ok
     test ota::tests::test_ota_staging_cleanly_deletes_preexisting_file_on_rejection ... ok
     test tests::test_rapid_concurrent_counter_updates ... ok
     test netplay::tests::hard_resync_via_state_transfer ... ok
     test netplay::tests::loopback_rollback_convergence ... ok

     test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.25s
     ```
   - Running `cargo test --workspace` in `d:\data\dev\beercade`:
     - `astro-daemon`: 37 passed, 0 failed.
     - `astro_tune`: 5 passed, 0 failed.
     - `beercade_hal`: 9 passed, 0 failed.
     - `beercade_shared`: 11 passed, 0 failed.
     - `boss-server`: 28 passed, 0 failed.
     - Total: 90 passed, 0 failed, 0 errors.

2. **OTA Staging and Hash Verification (`astro/daemon/src/ota.rs`)**:
   - `stage_ota_payload` (lines 45-64):
     ```rust
     let expected_sha256 = match response.headers().get("x-ota-sha256") {
         None => {
             let _ = tokio::fs::remove_file(staging_path).await;
             return Err("Missing mandatory X-OTA-SHA256 header in OTA response".to_string());
         }
         Some(val) => match val.to_str() {
             Ok(s) => {
                 let trimmed = s.trim().to_ascii_lowercase();
                 if trimmed.is_empty() {
                     let _ = tokio::fs::remove_file(staging_path).await;
                     return Err("Empty X-OTA-SHA256 header in OTA response".to_string());
                 }
                 trimmed
             }
             Err(_) => {
                 let _ = tokio::fs::remove_file(staging_path).await;
                 return Err("Invalid UTF-8 in X-OTA-SHA256 header".to_string());
             }
         },
     };
     ```
   - Lines 96-108: Any chunk streaming or write failure drops file handle `dest` and deletes `staging_path`.
   - Line 119: File handle is explicitly dropped (`drop(dest)`) before hash validation or cleanup, ensuring Windows file locks are released without `ERROR_SHARING_VIOLATION`.
   - Lines 121-143: Checks `total_bytes == 0` (empty payload rejection), `expected_bytes > 0 && total_bytes != expected_bytes` (size mismatch rejection), and compares computed hash with expected hash using `beercade_shared::auth::constant_time_eq`. Any mismatch cleans up `staging_path`.

3. **Credit Persistence and Crash-Safe Recovery (`astro/daemon/src/main.rs`)**:
   - `persist_credits_to` (lines 52-85):
     - Computes temporary staging file `credits.json.tmp` and backup `credits.json.bak`.
     - Opens `tmp`, writes serialized JSON, and calls `file.sync_all()?`.
     - Explicitly drops `file` handle prior to rename.
     - Calls `std::fs::rename(&tmp, path)?` for atomic swap.
     - Redundantly copies intact primary to `credits.json.bak`.
   - `load_credits` (lines 93-176):
     - Tier 1: Attempts reading and deserializing primary `credits.json`. If intact, verifies `.bak` exists and returns counters.
     - Tier 2: If primary is missing, 0 bytes, truncated, or invalid JSON, logs warning and attempts loading from `credits.json.bak`. If `.bak` is valid, self-heals primary by calling `persist_credits_to(path, &recovered)`.
     - Tier 3: If both primary and backup are corrupt or missing, logs error and returns `CreditCounters::default()` without panicking.

4. **Network Configuration Verification (`astro/rootfs/`)**:
   - `astro/rootfs/etc/systemd/network/10-wired.network`:
     - Lines 6-12 contain:
       ```ini
       DHCP=yes
       LinkLocalAddressing=fallback
       DNSSEC=no
       MulticastDNS=yes
       LLDP=no
       EmitLLDP=no
       IPv6AcceptRA=no
       ```
     - Static `Address=192.168.100.2/24` and `Gateway=192.168.100.1` are completely removed.
   - `astro/rootfs/usr/local/bin/boss_static_net.sh`:
     - Replaced `ip addr replace 192.168.100.2/24` with link-up only: `ip link set "$IFACE" up || true`.
   - `git grep "192.168.100.2" astro/rootfs/`: Exit code 1 (0 matches).
   - `grep_search` across `d:\data\dev\beercade\astro\rootfs`: 0 matches for `192.168.100.2` and 0 matches for `192.168.100`.

5. **Image Build Verification**:
   - `Get-Process | Where-Object { $_.ProcessName -match "docker|mksquashfs|qemu" }`: Returned 0 running processes.
   - Command history and logs confirmed zero executions of `make image-x86`, `mksquashfs`, or Docker rootfs export commands.

6. **Compiler Cleanliness**:
   - Running `cargo check -p astro-daemon --tests`: Clean finish with 0 errors and 0 warnings.

---

## 2. Logic Chain

1. **R1 Conformance (Mandatory SHA-256 Verification & Cleanup)**:
   - *Observation 2* demonstrates that `stage_ota_payload` unconditionally requires `X-OTA-SHA256`. If the header is missing, empty/whitespace, or invalid UTF-8, it returns an explicit error and removes any staged file.
   - Streaming is hashed in chunks via `Sha256::new()`, flushed via `sync_all()`, and verified in constant time.
   - File handles are explicitly closed before deletion or hash comparison, preventing Windows sharing lock deadlocks.
   - 11 unit tests in `ota.rs` independently confirm these behaviors across negative, boundary, and positive scenarios.
   - *Conclusion*: Requirement R1 is fully met and robustly implemented.

2. **R2 Conformance (Atomic & Crash-Safe Credit Persistence)**:
   - *Observation 3* demonstrates that `persist_credits_to` writes to `.tmp`, flushes to physical storage with `sync_all()`, drops file descriptor, atomically renames `.tmp` -> `credits.json`, and backs up to `credits.json.bak`.
   - *Observation 3* demonstrates that `load_credits` implements 3-tiered crash recovery: primary -> backup with automatic self-healing -> default zero without panic.
   - Unit tests verify recovery from half-truncated JSON, 0-byte primary, corrupted garbage bytes, and missing primary, as well as 20 concurrent writer tasks without race conditions or data corruption.
   - *Conclusion*: Requirement R2 is fully met and crash-safe.

3. **R3 Conformance (Dynamic Network Assignment & No IP Conflicts)**:
   - *Observation 4* proves that the hardcoded static IP `192.168.100.2` and gateway `192.168.100.1` have been eradicated from both `10-wired.network` and `boss_static_net.sh`.
   - The networkd config enforces `DHCP=yes` and `LinkLocalAddressing=fallback` for link-local fallback.
   - Automated regression tests `test_r3_network_config_no_static_ip` and `test_r3_static_net_script_no_static_ip` ensure future edits will not reintroduce hardcoded static addresses.
   - *Conclusion*: Requirement R3 is fully met.

4. **R6 Conformance (Command Dispatcher & ACK Behavior)**:
   - *Observation 1* shows that `main.rs` implements `execute_cabinet_command` and `ack_command`, handling `Ping`, `AddCredits`, `StartSession`, `EndSession`, `Reboot`, and `LaunchGame`.
   - Deserialization errors and command execution outcomes are properly captured and ACKed back to BOSS with bearer token authorization.
   - Tested thoroughly against mock TCP servers and state transitions.
   - *Conclusion*: Requirement R6 is fully met.

5. **Integrity & Cheating Check**:
   - Actively evaluated all 5 integrity violation checks:
     - No hardcoded test results embedded in source code.
     - No dummy or facade implementations (real disk I/O, real TCP networking, real SHA-256 calculation).
     - No shortcuts or bypassed requirements.
     - No fabricated test outputs (independently run and verified).
     - No self-certifying work without independent verification.
   - *Conclusion*: Zero integrity violations.

---

## 3. Caveats

1. **Hardware-Specific Partition Flashing**:
   - `execute_ota_update` delegates low-level partition writes to `dd` and boot slot commit to `Platform::commit_boot_slot`. In host test environments (Windows/macOS/Linux user space), testing properly targets `stage_ota_payload` and verifies that the payload is safely downloaded, verified, and staged without requiring physical raw block device access.
2. **Reboot Execution**:
   - The `CabinetCommand::Reboot` handler schedules `systemctl reboot` via tokio background spawn guarded with `#[cfg(target_os = "linux")]`, ensuring that running test suites on Windows dev machines does not reboot the developer workstation.

---

## 4. Conclusion & Verdict

**Verdict**: **`APPROVE`**

Milestone 3 is verified complete, correct, and robust. All requirements (R1, R2, R3, R6) are satisfied with 100% test pass rates (37/37 tests in `astro-daemon`, 90/90 tests across workspace), zero lingering static IP references, zero image build violations, and zero integrity defects.

---

## 5. Verification Method

To independently reproduce this verification:

1. **Run ASTRO Daemon Test Suite**:
   ```powershell
   cargo test -p astro-daemon
   ```
   *Expected Result*: 37 tests pass, 0 fail.

2. **Run Workspace Test Suite**:
   ```powershell
   cargo test --workspace
   ```
   *Expected Result*: 90 tests pass, 0 fail.

3. **Verify Static IP Elimination**:
   ```powershell
   git grep "192.168.100.2" astro/rootfs/
   ```
   *Expected Result*: Exit code 1 (no matches).

4. **Check Compiler and Linter Warnings**:
   ```powershell
   cargo check -p astro-daemon --tests
   ```
   *Expected Result*: Exit code 0, no errors, no warnings.

5. **Invalidation Conditions**:
   - Any test failure in `cargo test -p astro-daemon` or `cargo test --workspace`.
   - Any recurrence of `192.168.100.2` in `astro/rootfs/`.
   - Staging file remaining on disk after an OTA checksum or header failure.
   - Unhandled panic on corrupted `credits.json`.
