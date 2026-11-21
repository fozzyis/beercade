# Handoff Report: Requirement R2 (Atomic Credit Persistence & Recovery)

**Milestone**: Milestone 3 (A.S.T.R.O. Daemon Robustness)  
**Agent**: Explorer M3-2 (`explorer_m3_2`)  
**Target Files**: `astro/daemon/src/main.rs`, `astro/daemon/Cargo.toml`  
**Handoff Type**: Hard (Investigation complete)  

---

## 1. Observation
1. **`astro/daemon/src/main.rs` lines 40-56**:
   ```rust
   impl DaemonSharedState {
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
   ...
   ```
   Directly observed:
   - `persist_credits` uses `std::fs::write(&self.credits_path, json)`. This truncates the file in-place upon open.
   - There is NO temporary file written or used.
   - There is NO `sync_all()` call on the file handle.
   - There is NO atomic rename.
   - There is NO backup file (`credits.json.bak`) created or updated.
   - Any I/O error during persistence is silently dropped (`let _ = std::fs::write(...)`).
   - `load_credits` reads only from `path`. If `path` is corrupted or truncated, `from_str` fails and returns `CreditCounters::default()` with zero logging and no attempt to recover from a backup.

2. **`astro/daemon/src/main.rs` lines 58-81**:
   ```rust
       pub fn add_credits(&mut self, amount: u32) {
           self.credits.credits_available = self.credits.credits_available.saturating_add(amount);
           self.credits.credits_inserted_total =
               self.credits.credits_inserted_total.saturating_add(amount as u64);
           self.credits.last_coin_epoch_ms = Some(
               std::time::SystemTime::now()
                   .duration_since(std::time::UNIX_EPOCH)
                   .unwrap_or_default()
                   .as_millis() as u64,
           );
           self.persist_credits();
       }

       pub fn consume_credit(&mut self) -> bool {
           if self.credits.credits_available > 0 {
               self.credits.credits_available -= 1;
               self.credits.credits_dispensed_today =
                   self.credits.credits_dispensed_today.saturating_add(1);
               self.persist_credits();
               true
           } else {
               false
           }
       }
   ```
   Directly observed:
   - Both `add_credits` and `consume_credit` require `&mut self` and synchronously call `self.persist_credits()`.
   - In `main()` lines 620-628, `daemon_state` is wrapped in `Arc<RwLock<DaemonSharedState>>`.
   - In `execute_cabinet_command` lines 340-352, `AddCredits` acquires `state_arc.write().await`, calling `st.add_credits(*credits)`.

3. **Existing Test Suite in `astro-daemon`**:
   Running `cargo test -p astro-daemon` executes 10 tests located exclusively in `src/netplay.rs` (`crc32_*`, `drift_controller_*`, `packet_roundtrip_*`, `rollback_*`).
   There are currently **zero** tests for `load_credits`, `persist_credits`, `DaemonSharedState`, or command dispatcher execution in `src/main.rs`.

4. **Dependencies**:
   `astro/daemon/Cargo.toml` has `tokio`, `serde`, `serde_json`, `reqwest`, `tracing`, `tracing-subscriber`, `sysinfo`, `tokio-tungstenite`, `futures-util`, `sha2`, `beercade-shared`, `beercade-hal`. Standard library `std::fs`, `std::io::Write`, and `std::path::PathBuf` are immediately available.

---

## 2. Logic Chain
1. **From Observation 1**: In-place `std::fs::write` opens the target file with truncation (`O_TRUNC`). If power is removed during writing, `credits.json` is left with 0 bytes or half-written JSON. Because `sync_all()` is never called, writes reside only in the volatile OS page cache and are not flushed to physical media.
2. **From Observation 1**: `load_credits` silently defaults to `CreditCounters::default()` when `from_str` fails. Therefore, after any power-loss during `persist_credits`, rebooting the cabinet wipes out all accumulated credits and accounting history, violating Requirement R2.
3. **From Requirement R2**:
   - Persisting to `credits.json.tmp` in the same directory guarantees source and destination reside on the same filesystem.
   - Calling `file.sync_all()?` flushes all data and inode metadata to non-volatile disk blocks.
   - Dropping the file handle closes the file descriptor, avoiding `ERROR_SHARING_VIOLATION` on Windows and releasing locks.
   - Calling `std::fs::rename` guarantees that replacing `credits.json` is an atomic filesystem operation. A reader or crash either sees the old intact file or the new intact file, never a partial file.
   - Updating `credits.json.bak` on successful write maintains a redundant, valid copy on disk.
4. **From Requirement R2 & Observation 1**: In `load_credits`:
   - If `credits.json` is healthy, return it and ensure `credits.json.bak` exists.
   - If `credits.json` is corrupted, truncated, or missing, attempt reading `credits.json.bak`.
   - If `credits.json.bak` is valid, log a warning, self-heal `credits.json` from `credits.json.bak`, and return the recovered counters.
   - If both are corrupted or missing, log an error and safely return `CreditCounters::default()` without panicking.
5. **From Observation 2**: Within the daemon process, all credit mutations occur through `Arc<RwLock<DaemonSharedState>>` using `write().await`. This provides mutual exclusion for memory mutation and persistence. Rapid additions or consumptions are serialized; however, to ensure tests can stress this without deadlocks or race conditions, tests must verify concurrent tasks updating state simultaneously.
6. **From Observation 3**: Expanding tests under `#[cfg(test)] mod tests` in `astro/daemon/src/main.rs` using RAII `TestEnv` (using `std::env::temp_dir()`) satisfies Requirement R6 without introducing external crate dependencies or disk image build steps.

---

## 3. Caveats
- `sync_all()` flushes the file descriptor. On Linux ext4/xfs, completely flushing directory entries (the link to `credits.json`) can additionally be performed by opening the directory file descriptor and calling `fsync()`. However, standard library `File::open` on a directory fails on Windows. Using `file.sync_all()` on the file before `std::fs::rename` is standard and portable across all operating systems.
- Network and command dispatcher integration tests should isolate network endpoints using fake local listeners or mock responders to prevent external network traffic.
- No other caveats.

---

## 4. Conclusion
Requirement R2 requires refactoring `persist_credits` and `load_credits` in `astro/daemon/src/main.rs`:
1. Implement `DaemonSharedState::persist_credits_to(path: &Path, counters: &CreditCounters) -> Result<(), std::io::Error>`:
   - Derives `path.with_added_extension(".tmp")` and `path.with_added_extension(".bak")`.
   - Writes JSON to `.tmp`.
   - Invokes `file.sync_all()?`.
   - Drops file handle.
   - Atomically renames `.tmp` -> `path` via `std::fs::rename`.
   - Copies `path` -> `.bak` to refresh backup.
2. Refactor `load_credits(path: &Path) -> CreditCounters`:
   - Dual-tier recovery: checks primary `credits.json`, falls back to `credits.json.bak` with warning log and self-healing restoration.
   - Catastrophic fallback: returns `CreditCounters::default()` without panic.
3. Add automated test module in `astro/daemon/src/main.rs` covering:
   - Happy-path atomic persistence and backup creation.
   - Recovery from truncated JSON (cut off mid-stream).
   - Recovery from 0-byte primary file.
   - Recovery from corrupted non-JSON garbage bytes.
   - Recovery when primary is deleted but backup exists.
   - Fallback when both primary and backup are corrupted (no panic).
   - Rapid concurrent credit updates across 20+ concurrent tasks.

---

## 5. Verification Method
1. **Compilation Check**:
   ```powershell
   cargo check -p astro-daemon
   ```
   Expected: 0 errors.

2. **Automated Test Execution**:
   ```powershell
   cargo test -p astro-daemon -- --nocapture
   ```
   Expected: All 10 existing netplay tests plus all newly added credit persistence unit tests pass cleanly (100% pass, exit code 0).

3. **Workspace Integrity Check**:
   ```powershell
   cargo test --workspace
   ```
   Expected: All tests pass across `beercade-shared`, `boss-server`, and `astro-daemon`.

4. **Failure Invalidation Conditions**:
   - Any panic occurs when `credits.json` or `credits.json.bak` contains invalid or truncated JSON.
   - `credits.json.tmp` is left behind after successful persistence.
   - Corrupted `credits.json` fails to recover valid credit counts from `credits.json.bak`.
   - Multiple concurrent `add_credits` calls result in mismatched total credits or corrupted JSON.
