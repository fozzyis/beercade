# Analysis: Requirement R1 — Mandatory SHA-256 OTA Verification & Cleanup

**Agent**: Explorer M3-1 (`explorer_m3_1`)  
**Target Component**: `astro/daemon/src/ota.rs`  
**Milestone**: Milestone 3  
**Requirements Addressed**: R1 (Enforce OTA Hash Verification) & R6 (OTA Staging Tests)

---

## 1. Executive Summary

Requirement R1 mandates that SHA-256 header verification during Over-The-Air (OTA) update staging in `astro/daemon/src/ota.rs` must be **strictly mandatory**. If the `X-OTA-SHA256` HTTP header is missing, empty, or fails checksum validation, ASTRO daemon must immediately reject the update, delete any staged files (including `.tmp` or destination files), and return an explicit error.

Our investigation of `astro/daemon/src/ota.rs`, `boss/src/ota_sync.rs`, and the shared cryptographic primitives in `beercade-shared` revealed:
1. **Critical Defect — Optional Hash**: The existing `execute_ota_update` function treats `X-OTA-SHA256` as entirely optional (`if let Some(expected) = expected_sha256 { ... }`). If the header is missing, no verification occurs, and an unverified SquashFS payload is flashed directly to the boot partition.
2. **Ambiguous Error for Empty Headers**: An empty header evaluates to `""`, causing a confusing mismatch error (`expected , got <hash>`) rather than an explicit empty-header rejection.
3. **Cleanup Leaks & File Locking**: If network chunk streaming fails or the checksum mismatches, the file handle `dest` is not dropped before calling `tokio::fs::remove_file`, causing Windows file sharing violations in development/test environments. Furthermore, if chunk streaming errors midway, the staging file is not removed at all.
4. **Zero Existing Tests**: There is currently zero unit or integration test coverage for OTA staging or verification in `astro-daemon`.
5. **Tightly Coupled Execution**: `execute_ota_update` combines HTTP streaming, disk staging, flashing with `dd`, bootloader commit, and `reboot`, making direct unit testing of staging impossible without refactoring.

A clean, backward-compatible decoupling into `pub async fn stage_ota_payload` and `pub async fn execute_ota_update`, combined with an in-memory `tokio::net::TcpListener` mock test harness and RAII `FakeRoot`, fully addresses all requirements with zero new external dependencies.

---

## 2. Current Implementation Analysis (`astro/daemon/src/ota.rs`)

### 2.1 File Structure & Flow
`astro/daemon/src/ota.rs` (171 lines) contains:
- Constants:
  - `STAGING_PATH: &'static str = "/data/beercade/sync/staging.sqsh"` (lines 15)
  - `STAGING_DIR: &'static str = "/data/beercade/sync"` (line 16)
  - `SPARE_MARGIN_BYTES: u64 = 64 * 1024 * 1024` (line 19)
- Primary entry point: `pub async fn execute_ota_update(boss_url: &str, filename: &str, boss_token: &str) -> Result<(), String>` (line 26)
- Helper functions:
  - `fn check_free_space(staging_dir: &str, required: u64) -> Result<(), String>` (lines 112-143)
  - `fn flash_inactive_slot(staging_path: &str, inactive_slot: &str) -> Result<(), String>` (lines 151-170)

### 2.2 Detailed Execution Trace of `execute_ota_update`
1. **Slot Detection** (lines 28-30):
   ```rust
   let platform = Platform::detect();
   let target = platform.active_slot().other();
   let inactive_slot = platform.slot_device(target);
   ```
2. **HTTP GET** (lines 33-40):
   Formats URL via `beercade_shared::ipc::format_ota_download_url(boss_url, filename)`.
   Executes `client.get(&url).bearer_auth(boss_token).send().await`.
3. **Pre-flight Free Space** (lines 47-55):
   Reads `content-length` header. Executes `check_free_space(STAGING_DIR, expected_bytes)`.
   *Problem*: `check_free_space` invokes `std::process::Command::new("df")`. On Windows or systems without `df`, this fails with an OS process spawn error.
4. **Staging Stream** (lines 58-68):
   Creates `dest = File::create(STAGING_PATH).await`.
   Streams chunks:
   ```rust
   let mut dest = File::create(STAGING_PATH)
       .await
       .map_err(|e| format!("failed to create staging file: {e}"))?;
   let mut total_bytes = 0u64;
   let mut hasher = Sha256::new();
   while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
       dest.write_all(&chunk).await.map_err(|e| e.to_string())?;
       hasher.update(&chunk);
       total_bytes += chunk.len() as u64;
   }
   dest.sync_all().await.map_err(|e| e.to_string())?;
   ```
   *Flaws*:
   - If `response.chunk().await` errors, the function returns early via `?` without deleting `STAGING_PATH`, leaving orphaned partial bytes on disk.
   - If `dest.write_all()` fails, partial file remains.
5. **Checksum Verification** (lines 70-82):
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
   *Flaws*:
   - If `expected_sha256` is `None` (header omitted, or non-UTF-8), the `if let Some` block is skipped entirely. No error is raised; no checksum is validated!
   - If `expected_sha256` is `Some("")` (empty string), it raises a mismatch error: `"SHA-256 mismatch: expected , got ..."`. It does not explicitly state that the header was empty.
   - `dest` is still in scope when `tokio::fs::remove_file(STAGING_PATH).await` is called. On Windows systems, open file handles cause `ERROR_SHARING_VIOLATION` (access denied), causing `remove_file` to silently fail (`let _ = ...`).
6. **Slot Flashing & Reboot** (lines 100-105):
   Executes `dd if=staging.sqsh of=<inactive_slot>` and `Command::new("reboot").spawn()`.

---

## 3. Server-Side Contract Analysis (`boss/src/ota_sync.rs`)

In B.O.S.S., OTA payload management is implemented in `boss/src/ota_sync.rs`:
1. **Upload (`payload_upload_handler`, lines 98-232)**:
   - When an operator uploads a SquashFS image via `POST /api/v1/sync/upload`, B.O.S.S. streams the payload through `Sha256::new()`.
   - It writes the computed hash to `{path}.sha256`:
     ```rust
     let hash_hex = format!("{:x}", hasher.finalize());
     let sha256_path_str = format!("{}.sha256", path_str);
     tokio::fs::write(&sha256_path_str, hash_hex).await;
     ```
2. **Download (`payload_download_handler`, lines 46-91)**:
   - When a node downloads via `GET /api/v1/sync/:filename`:
     ```rust
     let sha256_path = format!("{}.sha256", path_str);
     if let Ok(sha256_hex) = tokio::fs::read_to_string(&sha256_path).await {
         if !sha256_hex.trim().is_empty() {
             resp = resp.header("X-OTA-SHA256", sha256_hex.trim());
         }
     }
     ```
   - **Key Finding**: If `{path}.sha256` is missing or empty on the B.O.S.S. server, B.O.S.S. serves the payload **without** the `X-OTA-SHA256` header.
   - Under current `astro-daemon` code, this allows unverified images to be flashed. Under Requirement R1, `astro-daemon` will strictly reject this download.

---

## 4. Cryptographic Primitives in Workspace

1. **`astro/daemon/Cargo.toml`**:
   Line 18 includes:
   ```toml
   sha2 = "0.10"
   ```
2. **`beercade-shared/src/crypto.rs`**:
   Provides tested SHA-256 helpers:
   - `compute_sha256(data: &[u8]) -> String`
   - `verify_sha256(data: &[u8], expected_hex: &str) -> bool`
3. **`beercade-shared/src/auth.rs`**:
   Provides constant-time string comparison:
   - `constant_time_eq(a: &[u8], b: &[u8]) -> bool`
4. **Verification Strategy**:
   - For streaming multi-gigabyte SquashFS payloads, `sha2::{Sha256, Digest}` must be used incrementally during chunk streaming (`hasher.update(&chunk)`) to avoid loading gigabytes into RAM.
   - For comparing the finalized hex digest with the header value, `beercade_shared::auth::constant_time_eq` should be used after lowercasing and trimming both strings.

---

## 5. Rejection & Cleanup Specification

Requirement R1 mandates:
> "If the X-OTA-SHA256 header is missing, empty, or fails checksum validation, reject the update, delete staged files, and return an explicit error."

### 5.1 Rejection Criteria & Error Messages
| Condition | Trigger | Error Message | Cleanup Action |
|-----------|---------|---------------|----------------|
| **Missing Header** | `response.headers().get("x-ota-sha256").is_none()` | `"Missing mandatory X-OTA-SHA256 header in OTA response"` | Remove staged file |
| **Empty Header** | Header present but `.trim().is_empty()` | `"Empty X-OTA-SHA256 header in OTA response"` | Remove staged file |
| **Invalid Format** | Header present but not valid UTF-8 | `"Invalid UTF-8 in X-OTA-SHA256 header"` | Remove staged file |
| **Malformed Hash** | Length != 64 or non-hexdigits | `"Invalid SHA-256 hex format in X-OTA-SHA256 header"` | Remove staged file |
| **Checksum Mismatch** | Computed hash != header hash | `"SHA-256 mismatch: expected <exp>, got <act>"` | Remove staged file |
| **Empty Payload** | `total_bytes == 0` | `"payload is empty; refusing to flash"` | Remove staged file |
| **Size Mismatch** | `expected_bytes > 0 && total_bytes != expected_bytes` | `"payload size mismatch: expected <exp> bytes, got <act>"` | Remove staged file |
| **Stream / Write Error** | `response.chunk()` or `write_all()` fails | `"failed streaming/writing OTA payload: <err>"` | Remove staged file |

### 5.2 Cleanup Guarantees
1. **Drop File Handle First**:
   Before calling `tokio::fs::remove_file(staging_path).await`, any open `tokio::fs::File` (`dest`) must be explicitly dropped (`drop(dest)`). This releases OS file locks, preventing Windows `ERROR_SHARING_VIOLATION` (error 32) and ensuring cross-platform cleanup reliability.
2. **Pre-existing File Cleanup**:
   If an earlier interrupted run or malicious actor left a file at `staging_path`, and the incoming update fails header checks, `tokio::fs::remove_file(staging_path).await` must be called to ensure no stale file remains.
3. **All Error Paths Clean Up**:
   Every `Err` exit path after `staging_path` is touched must guarantee removal.

---

## 6. Proposed Architectural Refactoring

To make `ota.rs` testable and maintain clean separation of concerns, we divide the functionality into two layers:

### 6.1 `pub async fn stage_ota_payload`
Performs the download, free space check, streaming, SHA-256 calculation, header validation, and guaranteed cleanup. It takes the target path as a parameter:
```rust
pub async fn stage_ota_payload(
    boss_url: &str,
    filename: &str,
    boss_token: &str,
    staging_path: &std::path::Path,
) -> Result<u64, String>
```
Returns `Ok(total_bytes)` on success, or `Err(String)` on failure (with guaranteed deletion of `staging_path`).

### 6.2 `pub async fn execute_ota_update`
Calls `stage_ota_payload`, then executes the hardware slot flashing (`dd`), bootloader target commit, and reboot:
```rust
pub async fn execute_ota_update(
    boss_url: &str,
    filename: &str,
    boss_token: &str,
) -> Result<(), String> {
    let platform = Platform::detect();
    let target = platform.active_slot().other();
    let inactive_slot = platform.slot_device(target);
    println!("[i] OTA Update Triggered: {} -> {}", filename, inactive_slot);

    let staging = staging_path();
    let total_bytes = stage_ota_payload(boss_url, filename, boss_token, &staging).await?;
    println!("[i] Staged {} bytes to {}", total_bytes, staging.display());

    flash_inactive_slot(staging.to_str().unwrap_or(STAGING_PATH), inactive_slot)?;
    platform.commit_boot_slot(target).map_err(|e| e.to_string())?;
    println!("[i] OTA applied to {}; rebooting onto the new image...", inactive_slot);
    let _ = std::process::Command::new("reboot").spawn();

    Ok(())
}
```
This preserves the exact signature and external contract expected by `astro/daemon/src/main.rs:709`.

### 6.3 Portable Free Space Check
Modify `check_free_space` so that if `Command::new("df")` fails because `df` is not installed on the host system (such as during Windows development or in non-Linux CI runners), it does not fail the test:
```rust
fn check_free_space(staging_dir: &std::path::Path, required: u64) -> Result<(), String> {
    if required == 0 {
        return Ok(());
    }
    let needed = required + SPARE_MARGIN_BYTES;
    let output = match std::process::Command::new("df")
        .arg("--output=avail")
        .arg("-B1")
        .arg(staging_dir)
        .output()
    {
        Ok(out) => out,
        Err(_) => {
            // Non-Linux host (e.g. Windows unit test environment) lacks df; allow execution
            return Ok(());
        }
    };

    if !output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8(output.stdout).unwrap_or_default();
    let avail = stdout
        .split_whitespace()
        .last()
        .and_then(|tok| tok.parse::<u64>().ok())
        .unwrap_or(0);

    if avail > 0 && avail < needed {
        return Err(format!(
            "insufficient free space on {}: need {} bytes (payload {} + margin) but only {} available",
            staging_dir.display(),
            needed,
            required,
            avail
        ));
    }
    Ok(())
}
```

---

## 7. Comprehensive Unit Test Plan

### 7.1 In-Memory Mock Server Architecture
Using `tokio::net::TcpListener::bind("127.0.0.1:0")`, we spin up lightweight, ephemeral HTTP mock servers for each test without external dependencies.

```rust
async fn spawn_mock_ota_server(
    status: u16,
    sha256_header: Option<&str>,
    body: Vec<u8>,
) -> (String, tokio::task::JoinHandle<()>)
```

### 7.2 RAII `FakeRoot` for Isolated Temporary Directories
```rust
struct FakeRoot(std::path::PathBuf);

impl FakeRoot {
    fn new(prefix: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        FakeRoot(dir)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for FakeRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
```

### 7.3 Test Matrix
We define 6 tests in `astro/daemon/src/ota.rs`:

1. **`test_ota_staging_success_when_hash_matches`**:
   - Given: Valid payload bytes (e.g. `b"BEERCADE_TEST_SQUASHFS_V1"`).
   - Server sends: `200 OK`, `X-OTA-SHA256: <valid_sha256>`, matching `Content-Length`.
   - Assertions:
     - `stage_ota_payload` returns `Ok(25)`.
     - `staging_file.exists()` is `true`.
     - Contents of `staging_file` exactly match the payload.

2. **`test_ota_staging_rejected_when_sha256_header_missing`**:
   - Server sends: `200 OK`, `Content-Length: 25`, **no** `X-OTA-SHA256` header.
   - Assertions:
     - `stage_ota_payload` returns `Err(msg)`.
     - `msg` explicitly contains `"Missing mandatory X-OTA-SHA256 header"`.
     - `staging_file.exists()` is `false`.

3. **`test_ota_staging_rejected_when_sha256_header_empty`**:
   - Server sends: `200 OK`, `X-OTA-SHA256: ""`.
   - Assertions:
     - `stage_ota_payload` returns `Err(msg)`.
     - `msg` explicitly contains `"Empty X-OTA-SHA256 header"`.
     - `staging_file.exists()` is `false`.

4. **`test_ota_staging_rejected_when_sha256_mismatched`**:
   - Server sends: `200 OK`, `X-OTA-SHA256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef` (tampered/mismatched).
   - Assertions:
     - `stage_ota_payload` returns `Err(msg)`.
     - `msg` explicitly contains `"SHA-256 mismatch"`.
     - `staging_file.exists()` is `false`.

5. **`test_ota_staging_cleanly_deletes_staged_file_on_rejection`**:
   - Setup: Pre-populate `staging_file` with stale/corrupt bytes on disk (`b"STALE_ORPHANED_DATA"`).
   - Server sends: `200 OK` with invalid checksum.
   - Assertions:
     - `stage_ota_payload` returns `Err(_)`.
     - `staging_file.exists()` is `false` (the pre-existing file was cleanly purged).

6. **`test_ota_staging_accepts_uppercase_sha256_header`**:
   - Server sends valid SHA-256 header formatted in uppercase (`E3B0...`).
   - Assertions:
     - Case-insensitive comparison succeeds.
     - `stage_ota_payload` returns `Ok(_)`.
     - `staging_file.exists()` is `true`.

---

## 8. Summary of Actionable Items for Implementer (Worker M3)

1. In `astro/daemon/src/ota.rs`:
   - Refactor `execute_ota_update` to separate `stage_ota_payload` from flashing/reboot.
   - Implement mandatory `X-OTA-SHA256` validation:
     - Missing header -> return explicit `Err` & delete staged file.
     - Empty header -> return explicit `Err` & delete staged file.
     - Checksum mismatch -> return explicit `Err` & delete staged file.
   - Ensure `drop(dest)` occurs before `tokio::fs::remove_file`.
   - Ensure all network/chunk error paths delete partial files.
   - Update `check_free_space` to handle non-Linux/Windows hosts without failing tests.
   - Add `#[cfg(test)] mod tests` containing the 6 tests using `tokio::net::TcpListener` and `FakeRoot`.
2. Run verification:
   `cargo test -p astro-daemon --lib ota::tests`
   `cargo test --workspace`
