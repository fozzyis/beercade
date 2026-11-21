# Requirement R2 Analysis: Atomic & Crash-Safe Credit Persistence & Backup Recovery

## Executive Summary
This document provides a comprehensive technical investigation into Requirement **R2** (Atomic & Crash-Safe Credit Persistence & Backup Recovery) and related Requirement **R6** (Automated Test Suite Expansion) for the A.S.T.R.O. Daemon in `astro/daemon/src/main.rs`.

Currently, credit accounting is vulnerable to corruption and data loss on sudden power-off or kernel panics because `persist_credits` directly overwrites `credits.json` with `std::fs::write`, never calls `sync_all()`, and `load_credits` has no backup fallback when `credits.json` is corrupted or truncated.

This report establishes:
1. Complete forensic analysis of existing vulnerabilities in `astro/daemon/src/main.rs`.
2. Exact atomic write architecture using `credits.json.tmp`, `sync_all()`, and atomic `rename`.
3. Self-healing dual-tier backup and recovery strategy via `credits.json.bak`.
4. Concurrency characteristics within `Arc<RwLock<DaemonSharedState>>`.
5. Comprehensive unit test suite design covering happy-path persistence, truncated/corrupted recovery, dual corruption fallback, and multi-task rapid counter updates.

---

## 1. Current Implementation Forensic Analysis

### 1.1 Existing Code (`astro/daemon/src/main.rs:39-82`)
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
}
```

### 1.2 Identified Failure Modes
1. **Direct In-Place Truncation (`std::fs::write`)**:
   `std::fs::write` calls `File::create(path)` under the hood, opening the file with `O_TRUNC | O_CREAT | O_WRONLY`. If power cuts or the station crashes mid-write, `credits.json` is left with 0 bytes or incomplete JSON (e.g., `{"credits_available": 10, "cr`).
2. **Missing `sync_all()` (OS Dirty Page Cache Vulnerability)**:
   Modern storage layers (eMMC/NVMe/ext4/NTFS) defer flushing dirty pages to non-volatile physical storage for up to 30 seconds. Without `sync_all()`, a sudden power loss (standard occurrence in arcade venues at closing time) can revert the file to zero length or metadata-only stubs.
3. **No Backup File (`credits.json.bak`)**:
   No backup file is maintained or updated. If the primary file is corrupted, recovery is impossible.
4. **Silent Reset in `load_credits`**:
   If `read_to_string` or `from_str` fails (due to corruption, partial writes, or syntax errors), `load_credits` silently returns `CreditCounters::default()` (0 credits, 0 lifetime total). The operator and player lose all accounting data and inserted coins without any warning emitted to logs or supervisory channels.
5. **Silent I/O Error Swallowing**:
   `if let Ok(json) = ... { let _ = std::fs::write(...) }` completely suppresses I/O errors. If the mount is read-only, disk is full, or permissions fail, `persist_credits` fails silently while in-memory counters keep incrementing.

---

## 2. Atomic Write Strategy Architecture

### 2.1 File Naming & Path Derivation
For any target `credits_path: &Path` (e.g. `/data/beercade/credits.json`):
- Temporary file path: Derived by appending `.tmp` to the target path:
  ```rust
  fn get_tmp_path(path: &Path) -> PathBuf {
      let mut os = path.as_os_str().to_os_string();
      os.push(".tmp");
      PathBuf::from(os)
  }
  ```
  Result: `/data/beercade/credits.json.tmp`.
- Backup file path: Derived by appending `.bak` to the target path:
  ```rust
  fn get_bak_path(path: &Path) -> PathBuf {
      let mut os = path.as_os_str().to_os_string();
      os.push(".bak");
      PathBuf::from(os)
  }
  ```
  Result: `/data/beercade/credits.json.bak`.

**Filesystem Locality Principle**: Because `credits.json.tmp`, `credits.json`, and `credits.json.bak` reside in the exact same directory, they are guaranteed to exist on the same physical filesystem partition/mount point. This guarantees that `std::fs::rename` is an atomic inode/metadata pointer swap rather than a cross-device file copy.

### 2.2 Sequence of Operations for Atomic Persistence
1. **Directory Preparation**:
   Check `path.parent()`. If non-empty, call `std::fs::create_dir_all(parent)`.
2. **Serialization**:
   Serialize `counters` with `serde_json::to_string_pretty`.
3. **Staging & Physical Flush (`sync_all`)**:
   Open/create `tmp_path`. Write all bytes.
   Execute `file.sync_all()?`.
   `sync_all()` forces OS file tables, inodes, and data blocks to physical media (via `fsync` on POSIX / `FlushFileBuffers` on Windows).
4. **Explicit Handle Close (`drop(file)`)**:
   Explicitly close the file handle before invoking `std::fs::rename`.
   *Critical Cross-Platform Detail*: On Windows, moving or replacing a file with an open handle can result in `ERROR_SHARING_VIOLATION` (os error 32). Calling `drop(file)` guarantees full cross-platform compatibility across Windows development environments and Linux RK3588S / x86_64 target platforms.
5. **Atomic Rename (`std::fs::rename`)**:
   Rename `tmp_path` to `path`.
   - On Linux/POSIX: Invokes `rename(2)` with atomic replace semantics.
   - On Windows: Invokes `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`.
   If `rename` fails, remove `tmp_path` and bubble/log the error.
6. **Backup Refresh**:
   Upon successful atomic placement of `credits.json`, copy `path` to `bak_path`:
   ```rust
   let _ = std::fs::copy(path, &bak_path);
   ```
   This ensures `credits.json.bak` is refreshed with known-good valid JSON.

---

## 3. Backup & Recovery Strategy Architecture

### 3.1 Multi-Tier Recovery Flow in `load_credits`
The boot loading sequence follows a resilient 3-tier cascade:

```
                  +-----------------------------------+
                  |   Call load_credits(credits_path) |
                  +-----------------+-----------------+
                                    |
                    +---------------v---------------+
                    | Attempt 1: Read credits.json   |
                    +---------------+---------------+
                                    |
             +----------------------+----------------------+
             | Parse OK                                    | Parse Error / Corrupt / Missing
             v                                             v
  +----------------------+                     +---------------------------+
  | Check credits.json.bak|                     | Log WARN: Primary corrupt |
  | (create if missing)  |                     | Attempt 2: Read .bak      |
  +----------+-----------+                     +-------------+-------------+
             |                                               |
             v                                +--------------+--------------+
  +----------------------+                    | Parse OK                    | Parse Error / Missing
  | Return Primary State |                    v                             v
  +----------------------+         +---------------------+        +-------------------------+
                                   | Log INFO: Recovered |        | Log ERROR: Dual Corrupt |
                                   | Self-Heal: Rewrite  |        | Attempt 3: Safe Fallback|
                                   | primary from backup |        +------------+------------+
                                   +----------+----------+                     |
                                              |                                v
                                              v                    +-------------------------+
                                   +---------------------+         | Return Default Counters |
                                   | Return Backup State |         | (0 credits, NO PANIC)   |
                                   +---------------------+         +-------------------------+
```

### 3.2 Tier 1: Primary File (`credits.json`)
- Read string from `credits.json`.
- Deserialize into `CreditCounters`.
- If successful:
  - If `credits.json.bak` is absent (e.g. first clean boot), copy `credits.json` -> `credits.json.bak` to prime the backup.
  - Return counters.

### 3.3 Tier 2: Backup Recovery (`credits.json.bak`)
- Triggered if:
  - `credits.json` does not exist (e.g. deleted or uninitialized), OR
  - `credits.json` is truncated (empty 0 bytes or trailing cut off), OR
  - `credits.json` is corrupted (syntax errors, invalid types, invalid UTF-8).
- Log a prominent warning:
  `tracing::warn!("Corrupted or missing credits file at {:?}: {err}. Attempting recovery from backup {:?}", path, bak_path);`
- Check if `credits.json.bak` exists.
- Read string and deserialize into `CreditCounters`.
- If backup deserialization succeeds:
  - Log recovery details:
    `tracing::info!("Successfully recovered credit counters from backup {:?}: available={}, inserted_total={}", bak_path, recovered.credits_available, recovered.credits_inserted_total);`
  - **Self-Healing Restoration**: Immediately write the recovered `CreditCounters` back to `credits.json` using the atomic persistence helper. This eliminates subsequent boots running in degraded recovery mode.
  - Return `recovered`.

### 3.4 Tier 3: Graceful Fallback (Dual Corruption)
- Triggered if:
  - Both `credits.json` and `credits.json.bak` are corrupted, unreadable, or missing.
- Log error:
  `tracing::error!("Both primary credits file {:?} and backup {:?} are corrupted or unreadable. Initializing default counters safely.", path, bak_path);`
- Return `CreditCounters::default()` (`credits_available = 0`, `credits_inserted_total = 0`, `credits_dispensed_today = 0`, `last_coin_epoch_ms = None`).
- **Zero Panic Guarantee**: Under no circumstances does `load_credits` crash, panic, or prevent the station daemon from starting up.

---

## 4. Concurrency & Credit Accounting in `DaemonSharedState`

### 4.1 In-Memory Concurrency Model
In `astro/daemon/src/main.rs`:
```rust
pub struct DaemonSharedState {
    pub node_id: String,
    pub boss_url: String,
    pub boss_token: String,
    pub mode: StationMode,
    pub credits: CreditCounters,
    pub active_game: Option<ActiveGameInfo>,
    pub credits_path: PathBuf,
}
```
State is wrapped in `Arc<tokio::sync::RwLock<DaemonSharedState>>`.

- **Reader Paths**:
  - Telemetry sampling loop (`sample_telemetry(&state)`) acquires a read lock (`state_arc.read().await`).
  - Command poll loop reads `boss_url` and `node_id`.
  - Non-blocking: Multiple readers inspect telemetry and credit counters simultaneously.
- **Writer Paths**:
  - Command execution (`CabinetCommand::AddCredits`) acquires a write lock (`state_arc.write().await`) and calls `st.add_credits(amount)`.
  - Game launch / coin insert loops acquire `write().await` and call `st.consume_credit()`.
  - Because `add_credits` and `consume_credit` require `&mut self`, Rust's ownership system combined with `RwLock` guarantees mutual exclusion.
- **Arithmetic Safety**:
  - `credits_available.saturating_add(amount)`
  - `credits_inserted_total.saturating_add(amount as u64)`
  - `credits_dispensed_today.saturating_add(1)`
  - Overflow or underflow wrapping is strictly prevented by saturating operations.

### 4.2 Rapid Counter Updates & I/O
When rapid coin insertions occur (e.g. 50 coin pulses in 1 second, or multiple concurrent remote `AddCredits` commands):
1. Each operation acquires the exclusive write lock in sequence.
2. The state is mutated in memory.
3. The atomic write, sync, and rename execute synchronously.
4. If multiple tasks call `add_credits` concurrently via `Arc<RwLock<DaemonSharedState>>`, the write lock serializes the filesystem updates.
5. Because every write atomically replaces `credits.json`, at no point does any reader or subsequent read see a partial or torn JSON document.

---

## 5. Implementation Proposal

### 5.1 Proposed Code in `astro/daemon/src/main.rs`
```rust
impl DaemonSharedState {
    /// Atomic persistence helper: writes to tmp, syncs to disk, atomically renames, and updates backup.
    pub fn persist_credits_to(path: &Path, counters: &CreditCounters) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let json = serde_json::to_string_pretty(counters)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut tmp_os = path.as_os_str().to_os_string();
        tmp_os.push(".tmp");
        let tmp_path = PathBuf::from(tmp_os);

        let mut bak_os = path.as_os_str().to_os_string();
        bak_os.push(".bak");
        let bak_path = PathBuf::from(bak_os);

        // 1. Write serialized JSON to temporary file
        {
            let mut file = std::fs::File::create(&tmp_path)?;
            file.write_all(json.as_bytes())?;
            // 2. Flush OS write caches to physical disk
            file.sync_all()?;
            // File descriptor is closed when dropped
        }

        // 3. Atomically rename credits.json.tmp to credits.json
        if let Err(e) = std::fs::rename(&tmp_path, path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }

        // 4. Update backup file with known-good state
        let _ = std::fs::copy(path, &bak_path);

        Ok(())
    }

    /// Load credit counters with crash recovery and fallback.
    pub fn load_credits(path: &Path) -> CreditCounters {
        let mut bak_os = path.as_os_str().to_os_string();
        bak_os.push(".bak");
        let bak_path = PathBuf::from(bak_os);

        // Tier 1: Try reading primary credits file
        match std::fs::read_to_string(path) {
            Ok(data) if !data.trim().is_empty() => {
                match serde_json::from_str::<CreditCounters>(&data) {
                    Ok(counters) => {
                        // Ensure backup exists
                        if !bak_path.exists() {
                            let _ = std::fs::copy(path, &bak_path);
                        }
                        return counters;
                    }
                    Err(err) => {
                        tracing::warn!(
                            "Corrupted primary credits file at {:?}: {}. Attempting recovery from backup {:?}",
                            path,
                            err,
                            bak_path
                        );
                    }
                }
            }
            Ok(_) => {
                tracing::warn!(
                    "Primary credits file at {:?} is empty (0 bytes). Attempting recovery from backup {:?}",
                    path,
                    bak_path
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    "Primary credits file {:?} not found, checking backup {:?}",
                    path,
                    bak_path
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Failed reading primary credits file at {:?}: {}. Attempting recovery from backup {:?}",
                    path,
                    e,
                    bak_path
                );
            }
        }

        // Tier 2: Try recovering from backup file
        if bak_path.exists() {
            match std::fs::read_to_string(&bak_path) {
                Ok(bak_data) if !bak_data.trim().is_empty() => {
                    match serde_json::from_str::<CreditCounters>(&bak_data) {
                        Ok(recovered) => {
                            tracing::info!(
                                "Successfully recovered credit counters from backup {:?}: available={}, inserted_total={}",
                                bak_path,
                                recovered.credits_available,
                                recovered.credits_inserted_total
                            );
                            // Self-heal: restore primary file with recovered counters
                            let _ = Self::persist_credits_to(path, &recovered);
                            return recovered;
                        }
                        Err(err) => {
                            tracing::error!(
                                "Backup credits file at {:?} is also corrupted: {}",
                                bak_path,
                                err
                            );
                        }
                    }
                }
                Ok(_) => {
                    tracing::error!("Backup credits file at {:?} is empty (0 bytes)", bak_path);
                }
                Err(e) => {
                    tracing::error!("Failed reading backup credits file at {:?}: {}", bak_path, e);
                }
            }
        }

        // Tier 3: Safe fallback when both are invalid/missing
        tracing::warn!("Initializing default credit counters safely (0 credits)");
        CreditCounters::default()
    }

    pub fn persist_credits(&self) {
        if let Err(e) = Self::persist_credits_to(&self.credits_path, &self.credits) {
            tracing::error!("Failed to persist credits to {:?}: {}", self.credits_path, e);
        }
    }
```

---

## 6. Unit & Integration Test Architecture (Requirement R6)

### 6.1 Test Environment Harness (`TestEnv`)
To satisfy the zero-external-dependency requirement in `TEST_INFRA.md`, tests use an RAII helper backed by `std::env::temp_dir()`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct TestEnv {
        dir: PathBuf,
    }

    impl TestEnv {
        fn new(prefix: &str) -> Self {
            let unique = format!(
                "beercade_test_{}_{}_{}",
                prefix,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let dir = std::env::temp_dir().join(unique);
            fs::create_dir_all(&dir).expect("failed to create test temp dir");
            Self { dir }
        }

        fn credits_path(&self) -> PathBuf {
            self.dir.join("credits.json")
        }

        fn bak_path(&self) -> PathBuf {
            self.dir.join("credits.json.bak")
        }

        fn tmp_path(&self) -> PathBuf {
            self.dir.join("credits.json.tmp")
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
```

### 6.2 Test Matrix for Credit Persistence
| Test Name | Focus | Scenario | Assertions |
|---|---|---|---|
| `test_atomic_persistence_happy_path` | Happy path persistence | Create state, call `add_credits(5)`, `consume_credit()` | `credits.json` exists & valid, `credits.json.bak` exists & valid, `credits.json.tmp` is deleted, `load_credits` returns 4 available & 5 total. |
| `test_recovery_from_truncated_credits_json` | Crash recovery | Valid `credits.json.bak`, truncated `credits.json` (e.g. `{"credits_av`) | `load_credits` restores data from `.bak`, self-heals `credits.json` to valid JSON, counters match backup. |
| `test_recovery_from_empty_credits_json` | 0-byte file recovery | Valid `credits.json.bak`, 0-byte `credits.json` | `load_credits` successfully loads from `.bak` and repairs primary. |
| `test_recovery_from_corrupted_garbage_json` | Garbage syntax recovery | Valid `credits.json.bak`, binary/corrupted bytes in `credits.json` | `load_credits` detects deserialization error, loads from `.bak`, self-heals primary. |
| `test_recovery_when_primary_missing_backup_present` | Missing primary recovery | No `credits.json`, valid `credits.json.bak` | `load_credits` restores from `.bak`, recreates `credits.json`. |
| `test_graceful_fallback_when_both_corrupted` | Dual corruption | Both `credits.json` and `credits.json.bak` contain garbage | `load_credits` returns `CreditCounters::default()` without panicking. |
| `test_graceful_fallback_when_neither_exists` | Fresh installation | Neither file exists | `load_credits` returns default counters without panic. |
| `test_rapid_concurrent_counter_updates` | Concurrency & stress | 20 tasks concurrently adding 5 credits each via `Arc<RwLock<DaemonSharedState>>` | Total credits = 100, `credits.json` remains perfectly valid JSON, no torn writes or leftover `.tmp`. |

---

## 7. Downstream Recommendations for Implementer
1. Add `use std::io::Write;` to `astro/daemon/src/main.rs`.
2. Implement `persist_credits_to`, updated `persist_credits`, and robust `load_credits` in `DaemonSharedState`.
3. Add the unit test module `#[cfg(test)] mod tests` in `astro/daemon/src/main.rs`.
4. Validate with `cargo test -p astro-daemon`.
