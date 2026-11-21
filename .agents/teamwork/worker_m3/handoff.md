# Handoff Report: Milestone 3 (A.S.T.R.O. Daemon Robustness & Network)

**Agent**: Worker M3 (`worker_m3`)  
**Date**: 2026-10-28T00:04:00Z  
**Recipient**: Orchestrator (`bd770878-29b7-48ec-a1e1-b5265681a707`)  
**Milestone**: Milestone 3  
**Handoff Type**: Hard (Task Complete)  

---

## 1. Observation

1. **Original State of `astro/daemon/src/ota.rs`**:
   - Lines 70-82 originally conditionally checked the `x-ota-sha256` header:
     ```rust
     let expected_sha256 = response
         .headers()
         .get("x-ota-sha256")
         .and_then(|v| v.to_str().ok())
         .map(|s| s.trim().to_lowercase());

     if let Some(expected) = expected_sha256 {
         let actual = format!("{:x}", hasher.finalize());
         if actual != expected {
             let _ = tokio::fs::remove_file(STAGING_PATH).await;
             return Err(format!("SHA-256 mismatch: expected {}, got {}", expected, actual));
         }
     }
     ```
   - If the header was omitted or invalid UTF-8, `expected_sha256` evaluated to `None`, skipping all verification and flashing unverified SquashFS payload onto disk.
   - If `actual != expected` failed, `tokio::fs::remove_file` was called while `dest: tokio::fs::File` was still open, triggering Windows `ERROR_SHARING_VIOLATION` (error 32).
   - If the header was empty, it returned a generic `"SHA-256 mismatch: expected , got <actual>"` rather than an explicit empty header rejection.

2. **Original State of `astro/daemon/src/main.rs` (Credit Persistence & Recovery)**:
   - Lines 40-56 originally performed in-place truncate-and-write:
     ```rust
     pub fn load_credits(path: &Path) -> CreditCounters {
         if let Ok(data) = std::fs::read_to_string(path) {
             if let Ok(c) = serde_json::from_str::<CreditCounters>(&data) {
                 return c;
             }
         }
         CreditCounters::default()
     }

     pub fn persist_credits(&self) {
         if let Some(parent) = self.credits_path.parent() {
             let _ = std::fs::create_dir_all(parent);
         }
         if let Ok(json) = serde_json::to_string_pretty(&self.credits) {
             let _ = std::fs::write(&self.credits_path, json);
         }
     }
     ```
   - No `.tmp` staging file was written.
   - No `sync_all()` was called on the file descriptor.
   - No atomic rename was used.
   - No `.bak` file was created or maintained.
   - Any corruption or power loss during write resulted in a 0-byte or corrupted `credits.json`.
   - On daemon restart, `load_credits` silently discarded all coin history and reset counters to zero with no fallback recovery.

3. **Original State of `astro/rootfs` Network Configuration**:
   - `astro/rootfs/etc/systemd/network/10-wired.network` configured:
     ```ini
     [Network]
     Address=192.168.100.2/24
     Gateway=192.168.100.1
     DNS=192.168.100.1 1.1.1.1 8.8.8.8
     ```
   - `astro/rootfs/usr/local/bin/boss_static_net.sh` configured:
     ```sh
     ip addr replace 192.168.100.2/24 dev "$IFACE" || true
     ip route replace default via 192.168.100.1 dev "$IFACE" || true
     ```
   - Any multi-cabinet arcade deployment on the same LAN subnet suffered IP collisions, duplicate ARP warnings, and broken networking.

4. **Test Suite Baseline**:
   - Running `cargo test -p astro-daemon` initially executed exactly 10 tests, all in `netplay.rs`.
   - Zero tests existed for `ota.rs`, credit persistence, command dispatcher execution, or rootfs network configuration.

5. **Post-Implementation Test Execution Results**:
   - Running `cargo test -p astro-daemon`:
     ```text
     running 37 tests
     test netplay::tests::crc32_incremental_matches_oneshot ... ok
     test netplay::tests::crc32_known_vectors ... ok
     test netplay::tests::drift_controller_converges_and_is_jury_stable ... ok
     test netplay::tests::packet_roundtrip_all_variants ... ok
     test netplay::tests::ring_buffer_wraparound ... ok
     test netplay::tests::synthetic_core_is_deterministic_and_order_sensitive ... ok
     test tests::test_command_deserialization_and_ack_payload ... ok
     test netplay::tests::late_input_below_resync_frame_is_ignored ... ok
     test netplay::tests::rollback_with_history_gap_escalates ... ok
     test tests::test_cmd_launch_game_validation ... ok
     test tests::test_cmd_ping_execution ... ok
     test tests::test_cmd_start_and_end_session ... ok
     test tests::test_cmd_reboot_non_blocking ... ok
     test tests::test_r3_network_config_no_static_ip ... ok
     test tests::test_r3_static_net_script_no_static_ip ... ok
     test tests::test_command_ack_dispatch_mock_server ... ok
     test ota::tests::test_ota_staging_rejected_when_sha256_header_missing ... ok
     test ota::tests::test_ota_staging_rejected_on_invalid_utf8_header ... ok
     test tests::test_atomic_credit_persistence_happy_path ... ok
     test ota::tests::test_ota_staging_rejected_on_server_error_status ... ok
     test ota::tests::test_ota_staging_rejected_when_sha256_header_empty ... ok
     test ota::tests::test_ota_staging_cleanly_deletes_preexisting_file_on_missing_header ... ok
     test tests::test_credit_fallback_when_both_files_corrupt_no_panic ... ok
     test ota::tests::test_ota_staging_rejected_when_payload_is_empty ... ok
     test tests::test_cmd_add_credits_execution ... ok
     test tests::test_credit_recovery_when_primary_deleted ... ok
     test tests::test_credit_recovery_from_zero_byte_primary ... ok
     test tests::test_credit_recovery_from_truncated_primary ... ok
     test tests::test_credit_recovery_from_corrupted_garbage_primary ... ok
     test ota::tests::test_ota_staging_success_when_hash_matches ... ok
     test ota::tests::test_ota_staging_rejected_on_size_mismatch ... ok
     test ota::tests::test_ota_staging_rejected_when_sha256_mismatched ... ok
     test ota::tests::test_ota_staging_accepts_uppercase_sha256_header ... ok
     test ota::tests::test_ota_staging_cleanly_deletes_preexisting_file_on_rejection ... ok
     test tests::test_rapid_concurrent_counter_updates ... ok
     test netplay::tests::hard_resync_via_state_transfer ... ok
     test netplay::tests::loopback_rollback_convergence ... ok

     test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.26s
     ```
   - Running `cargo test --workspace`:
     - `astro-daemon`: 37 passed, 0 failed.
     - `astro_tune`: 5 passed, 0 failed.
     - `beercade_hal`: 9 passed, 0 failed.
     - `beercade_shared`: 11 passed, 0 failed.
     - `boss-server`: 28 passed, 0 failed.
     - Total: 90 passed, 0 failed, 0 errors.

6. **Static IP Search**:
   - Running `grep_search` for `192.168.100.2` in `d:\data\dev\beercade\astro\rootfs`:
     `No results found`.

---

## 2. Logic Chain

1. **Requirement R1 (Mandatory OTA SHA-256 Verification & Cleanup)**:
   - *Observation 1* revealed that missing/invalid headers allowed unverified OTA payloads to be flashed.
   - Refactored `execute_ota_update` to delegate staging to `pub async fn stage_ota_payload(boss_url: &str, filename: &str, boss_token: &str, staging_path: &Path) -> Result<u64, String>`.
   - Enforced strict presence check on `X-OTA-SHA256`:
     - Header absent: returns `Err("Missing mandatory X-OTA-SHA256 header in OTA response")` and deletes `staging_path`.
     - Header empty / whitespace: returns `Err("Empty X-OTA-SHA256 header in OTA response")` and deletes `staging_path`.
     - Header non-UTF8: returns `Err("Invalid UTF-8 in X-OTA-SHA256 header")` and deletes `staging_path`.
   - Normalizes hash with `.to_ascii_lowercase()` and verifies against computed SHA-256 with `beercade_shared::constant_time_eq`.
   - Explicitly drops `dest` handle (`drop(dest)`) prior to deletion or hash comparison to ensure Windows file locks are freed, avoiding `ERROR_SHARING_VIOLATION`.
   - Handles network streaming failures, sync errors, 0-byte payload refusal, and size mismatch errors by cleaning up `staging_path`.
   - Added 11 unit tests in `astro/daemon/src/ota.rs` utilizing mock HTTP listeners (`127.0.0.1:0`) and RAII `FakeRoot`.

2. **Requirement R2 (Atomic Credit Persistence & Crash-Safe Backup Recovery)**:
   - *Observation 2* demonstrated that in-place `std::fs::write` caused total credit loss upon power cut.
   - Implemented `DaemonSharedState::persist_credits_to`:
     - Computes temporary path (`credits.json.tmp`) and backup path (`credits.json.bak`).
     - Serializes pretty JSON, writes to `.tmp`, and calls `file.sync_all()?` to flush kernel page buffers and disk write cache.
     - Drops file descriptor handle before rename.
     - Atomically renames `.tmp` -> `credits.json` via `std::fs::rename`.
     - Updates redundant backup copy by copying `credits.json` -> `credits.json.bak`.
   - Implemented tiered recovery in `DaemonSharedState::load_credits`:
     - Tier 1: checks primary `credits.json`. If valid and non-empty, ensures `.bak` exists and returns counters.
     - Tier 2: if primary is missing, 0-byte, truncated, or invalid JSON, logs `tracing::warn!`, attempts reading `credits.json.bak`. If `.bak` is valid, logs recovery, self-heals primary `credits.json` by persisting recovered counters, and returns counters.
     - Tier 3: if both primary and backup are corrupted or missing, logs `tracing::error!` and safely returns `CreditCounters::default()` without panicking.
   - Added 7 unit tests in `astro/daemon/src/main.rs` covering happy path atomic write, truncated primary recovery with self-healing, 0-byte file recovery, corrupted garbage recovery, missing primary recovery, double corruption fallback, and 20 concurrent tasks performing simultaneous credit updates.

3. **Requirement R3 (Dynamic Network Configuration)**:
   - *Observation 3* showed hardcoded `192.168.100.2/24` and gateway `192.168.100.1` causing LAN collisions.
   - Modified `astro/rootfs/etc/systemd/network/10-wired.network` to remove `Address=`, `Gateway=`, and `DNS=`, retaining `DHCP=yes` and `LinkLocalAddressing=fallback`.
   - Modified `astro/rootfs/usr/local/bin/boss_static_net.sh` to remove `ip addr replace 192.168.100.2/24` and `ip route replace default`.
   - Confirmed via `grep_search` and automated unit tests (`test_r3_network_config_no_static_ip` and `test_r3_static_net_script_no_static_ip`) that no references to `192.168.100.2` remain.

4. **Requirement R6 (Command Dispatcher Execution & ACK Behavior)**:
   - Imported `CommandAck` into `astro/daemon/src/main.rs`.
   - Added comprehensive unit tests in `astro/daemon/src/main.rs` exercising `execute_cabinet_command` and `ack_command` across:
     - `Ping` sequence echo
     - `AddCredits` state accumulation and persistence
     - `StartSession` and `EndSession` lifecycle transitions
     - Non-blocking `Reboot` command scheduling
     - `LaunchGame` validation rejecting empty core and empty rom
     - Command deserialization error handling and ACK payload construction
     - Mock HTTP server verification of `ack_command` payload delivery with bearer token authentication.

---

## 3. Caveats

1. **Hardware Bootloader Execution (`dd` / `bootctl` / `reboot`)**:
   - `execute_ota_update` delegates hardware partition writes to `dd` and bootloader switching to `bootctl` / `extlinux`. In automated unit testing, test cases test `stage_ota_payload` against mock HTTP listeners, while hardware calls are guarded or tested on staging artifacts.
2. **Strict Isolation**:
   - All tests run against ephemeral loopback addresses (`127.0.0.1:0`) and temp directories (`std::env::temp_dir()`). No external network requests or disk modifications outside temp files were made.
3. **No Image Builds**:
   - In accordance with the mandate, no image build commands (`make image-x86`, `mksquashfs`, Docker exports) were run.

---

## 4. Conclusion

Milestone 3 is completely implemented, verified, and passing 100%:
- Requirement R1: OTA hash verification is mandatory; missing, empty, or mismatched checksums immediately reject payload staging and delete files from disk with proper handle cleanup.
- Requirement R2: Credit persistence is crash-safe with atomic rename, fsync (`sync_all()`), redundant backup (`.bak`), and self-healing recovery.
- Requirement R3: Static `192.168.100.2` network conflict has been eliminated from rootfs files, enabling pure dynamic DHCP + link-local fallback.
- Requirement R6: Full test suite expansion implemented; 37 tests passing in `astro-daemon` and 90 tests passing across the entire workspace.
- Exclusive file ownership rules strictly obeyed: only the 4 designated files (`astro/daemon/src/ota.rs`, `astro/daemon/src/main.rs`, `astro/rootfs/etc/systemd/network/10-wired.network`, `astro/rootfs/usr/local/bin/boss_static_net.sh`) and agent metadata files were modified.

---

## 5. Verification Method

To independently reproduce and verify this milestone:

1. **Run ASTRO Daemon Test Suite**:
   ```powershell
   cargo test -p astro-daemon
   ```
   *Expected Output*: 37 tests passed; 0 failed; finished in ~1.3s.

2. **Run Full Workspace Test Suite**:
   ```powershell
   cargo test --workspace
   ```
   *Expected Output*: 90 tests passed across `astro-daemon`, `astro_tune`, `beercade_hal`, `beercade_shared`, and `boss-server`; 0 failed.

3. **Verify Absence of Static IP in RootFS**:
   ```powershell
   git grep "192.168.100.2" astro/rootfs/
   ```
   *Expected Output*: Exit code 1 (no occurrences found).

4. **Verify Compiler and Linter Cleanliness**:
   ```powershell
   cargo check -p astro-daemon --tests
   ```
   *Expected Output*: Zero errors, zero warnings.

5. **Invalidation Conditions**:
   - Any failure in `cargo test -p astro-daemon` or `cargo test --workspace`.
   - Any recurrence of `192.168.100.2` in `astro/rootfs/`.
   - Staged OTA file left on disk after checksum mismatch or missing header.
   - Daemon panic on truncated or corrupted `credits.json`.
