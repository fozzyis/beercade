# Handoff Report: Milestone 3 — Requirement R1 (Mandatory SHA-256 OTA Verification & Cleanup)

**Agent**: Explorer M3-1 (`explorer_m3_1`)  
**Date**: 2026-10-27T23:45:00Z  
**Recipient**: Orchestrator (`bd770878-29b7-48ec-a1e1-b5265681a707`)  
**Milestone**: Milestone 3 (Requirements R1 & R6)  
**Artifacts Generated**: `analysis.md`, `handoff.md`  

---

## 1. Observation

1. **`astro/daemon/src/ota.rs` Line 70-82 (Current SHA-256 Handling)**:
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
   - **Observation 1a**: If `expected_sha256` is `None` (header omitted by server or invalid UTF-8), the entire `if let Some` block is bypassed. No verification error is returned, the unverified payload is staged, written to the inactive slot via `dd`, and rebooted.
   - **Observation 1b**: If `expected_sha256` is `Some("")` (empty string), `actual != ""` evaluates to true, yielding `Err("SHA-256 mismatch: expected , got <actual>")`. This obscures the fact that the header was empty.
   - **Observation 1c**: The file handle `dest: tokio::fs::File` (opened at line 58) remains in scope when `tokio::fs::remove_file(STAGING_PATH).await` is called at line 79. On Windows host systems, open handles cause file deletion to fail with `ERROR_SHARING_VIOLATION` (error 32).
   - **Observation 1d**: At lines 63-67, if `response.chunk().await` or `dest.write_all()` returns an `Err`, the function returns early via `?` without invoking `remove_file`, leaving partial files on disk.

2. **`boss/src/ota_sync.rs` Lines 75-80 (Server Header Generation)**:
   ```rust
   let sha256_path = format!("{}.sha256", path_str);
   if let Ok(sha256_hex) = tokio::fs::read_to_string(&sha256_path).await {
       if !sha256_hex.trim().is_empty() {
           resp = resp.header("X-OTA-SHA256", sha256_hex.trim());
       }
   }
   ```
   - If `{path}.sha256` does not exist or is empty on B.O.S.S., the `X-OTA-SHA256` header is omitted from the response.

3. **`astro/daemon/Cargo.toml` Line 18**:
   ```toml
   sha2 = "0.10"
   ```
   - The `sha2` crate is already present in `astro-daemon` dependencies.
   - In `shared/crates/beercade-shared/src/auth.rs`, `constant_time_eq` is already implemented and exported.

4. **Existing Test Coverage in `astro-daemon`**:
   - Command: `cargo test -p astro-daemon`
   - Output: 10 tests run, all in `netplay.rs`. Exactly 0 tests exist in `ota.rs` or `main.rs`.

5. **`TEST_INFRA.md` Specification for R1**:
   - Tier 1 & Tier 2: 5 tests + 5 tests for Mandatory OTA SHA-256.
   - Guideline: Zero external dependencies; use in-memory `tokio::net::TcpListener` (`127.0.0.1:0`) and standard library RAII `FakeRoot` (`std::env::temp_dir()`).

---

## 2. Logic Chain

1. **From Observation 1a to R1 Vulnerability**:
   - Because `expected_sha256` is checked via `if let Some(expected)`, a missing header is silently accepted.
   - From Observation 2, B.O.S.S. omits the header if `{path}.sha256` is missing.
   - Therefore, a node requesting an unhashed or corrupted payload from B.O.S.S. will flash the payload without error, violating Requirement R1.
   - *Conclusion*: Header presence must be made mandatory (`match raw_header { None => return Err(...), ... }`).

2. **From Observation 1b to Explicit Error Semantics**:
   - An empty string header currently triggers a mismatch error with an empty expected value.
   - Requirement R1 explicitly states: *"If the X-OTA-SHA256 header is missing, empty, or fails checksum validation, reject the update, delete staged files, and return an explicit error."*
   - *Conclusion*: Empty headers must be explicitly identified and rejected with an error such as `"Empty X-OTA-SHA256 header in OTA response"`.

3. **From Observation 1c & 1d to Guaranteed Cleanup**:
   - Incomplete downloads or checksum rejections must delete staged files.
   - Calling `remove_file` while `dest` is open fails on Windows due to file sharing locks.
   - *Conclusion*: The file handle `dest` must be explicitly dropped (`drop(dest)`) prior to calling `remove_file`. Furthermore, all network streaming errors and pre-flight rejections must trigger cleanup.

4. **From Observations 4 & 5 to Testability Architecture**:
   - Direct invocation of `execute_ota_update` in tests attempts to run `df`, `dd`, `bootctl`, and `reboot`, which cannot run safely or successfully in a unit test environment.
   - By extracting payload downloading, verification, and staging into `pub async fn stage_ota_payload(...) -> Result<u64, String>`, tests can directly exercise staging, header validation, and file deletion against an in-memory `tokio::net::TcpListener` mock server, while `execute_ota_update` delegates to `stage_ota_payload` without altering its signature.

---

## 3. Caveats

1. **Hardware Bootloader & Flashing in Real HW**:
   - `flash_inactive_slot` (`dd`) and `platform.commit_boot_slot` (`bootctl` / extlinux) are real hardware operations and cannot be exercised in unit tests without disk devices. Our unit tests will strictly verify `stage_ota_payload` and cleanup logic.
2. **`check_free_space` Platform Dependency**:
   - `check_free_space` relies on `df --output=avail -B1`. On Windows or test containers where `df` is unavailable, `check_free_space` must gracefully tolerate `df` being missing so that unit tests can execute cross-platform.
3. **Scope Constraint**:
   - In accordance with explorer rules and project constraints, no source files were modified, and no image builds (`make image-x86`, `mksquashfs`) were run.

---

## 4. Conclusion & Concrete Proposal

We recommend that Worker M3 implement the following changes in `astro/daemon/src/ota.rs`:

### 4.1 Implementation Code Proposal for `astro/daemon/src/ota.rs`

```rust
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use reqwest::Client;
use sha2::{Sha256, Digest};
use std::path::Path;

use beercade_hal::Platform;

const STAGING_PATH: &'static str = "/data/beercade/sync/staging.sqsh";
const STAGING_DIR: &'static str = "/data/beercade/sync";
const SPARE_MARGIN_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

/// Staging implementation: downloads, validates mandatory SHA-256 header, streams payload,
/// checks hash integrity, and guarantees immediate deletion upon any failure.
pub async fn stage_ota_payload(
    boss_url: &str,
    filename: &str,
    boss_token: &str,
    staging_path: &Path,
) -> Result<u64, String> {
    let url = beercade_shared::ipc::format_ota_download_url(boss_url, filename);
    let client = Client::new();
    let mut response = client
        .get(&url)
        .bearer_auth(boss_token)
        .send()
        .await
        .map_err(|e| format!("failed to reach BOSS sync endpoint: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Failed to download OTA payload: {}", response.status()));
    }

    // MANDATORY HEADER CHECK (R1): Header must be present, valid UTF-8, and non-empty.
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

    // Budget free space if content-length is present
    let expected_bytes = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(0);

    let staging_dir = staging_path.parent().unwrap_or(Path::new(STAGING_DIR));
    check_free_space(staging_dir, expected_bytes)?;

    // Ensure parent directory exists
    if let Some(parent) = staging_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    // Stream into destination file
    let mut dest = File::create(staging_path)
        .await
        .map_err(|e| format!("failed to create staging file: {e}"))?;

    let mut total_bytes = 0u64;
    let mut hasher = Sha256::new();

    while let Some(chunk_res) = response.chunk().await.transpose() {
        match chunk_res {
            Ok(chunk) => {
                if let Err(e) = dest.write_all(&chunk).await {
                    drop(dest);
                    let _ = tokio::fs::remove_file(staging_path).await;
                    return Err(format!("failed to write chunk to staging file: {e}"));
                }
                hasher.update(&chunk);
                total_bytes += chunk.len() as u64;
            }
            Err(e) => {
                drop(dest);
                let _ = tokio::fs::remove_file(staging_path).await;
                return Err(format!("failed to stream OTA payload: {e}"));
            }
        }
    }

    if let Err(e) = dest.sync_all().await {
        drop(dest);
        let _ = tokio::fs::remove_file(staging_path).await;
        return Err(format!("failed to sync staging file: {e}"));
    }

    // Drop open file handle before verifying or attempting removal
    drop(dest);

    // Validate payload size
    if total_bytes == 0 {
        let _ = tokio::fs::remove_file(staging_path).await;
        return Err("payload is empty; refusing to flash".to_string());
    }
    if expected_bytes > 0 && total_bytes != expected_bytes {
        let _ = tokio::fs::remove_file(staging_path).await;
        return Err(format!(
            "payload size mismatch: expected {} bytes, got {}",
            expected_bytes, total_bytes
        ));
    }

    // Validate SHA-256 Checksum
    let actual = format!("{:x}", hasher.finalize()).to_ascii_lowercase();
    if !beercade_shared::auth::constant_time_eq(actual.as_bytes(), expected_sha256.as_bytes()) {
        let _ = tokio::fs::remove_file(staging_path).await;
        return Err(format!(
            "SHA-256 mismatch: expected {}, got {}",
            expected_sha256, actual
        ));
    }

    Ok(total_bytes)
}

pub async fn execute_ota_update(boss_url: &str, filename: &str, boss_token: &str) -> Result<(), String> {
    let platform = Platform::detect();
    let target = platform.active_slot().other();
    let inactive_slot = platform.slot_device(target);
    println!("[i] OTA Update Triggered: {} -> {}", filename, inactive_slot);

    let staging_path = Path::new(STAGING_PATH);
    let total_bytes = stage_ota_payload(boss_url, filename, boss_token, staging_path).await?;
    println!("[i] Staged {} bytes to {}", total_bytes, staging_path.display());

    flash_inactive_slot(STAGING_PATH, inactive_slot)?;
    platform.commit_boot_slot(target).map_err(|e| e.to_string())?;
    println!("[i] OTA applied to {}; rebooting onto the new image...", inactive_slot);
    let _ = std::process::Command::new("reboot").spawn();

    Ok(())
}

fn check_free_space(staging_dir: &Path, required: u64) -> Result<(), String> {
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
            // df is unavailable on non-Linux dev hosts; allow execution
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

### 4.2 Proposed Unit Test Suite Structure

In `astro/daemon/src/ota.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct FakeRoot(std::path::PathBuf);

    impl FakeRoot {
        fn new(prefix: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let id = N.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), id));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            FakeRoot(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FakeRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn spawn_mock_ota_server(
        status: u16,
        sha256_header: Option<&str>,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{}", addr);

        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;

                let header_str = match sha256_header {
                    Some(sha) => format!("X-OTA-SHA256: {}\r\n", sha),
                    None => String::new(),
                };

                let response = format!(
                    "HTTP/1.1 {} OK\r\n\
                     Content-Length: {}\r\n\
                     {}\
                     \r\n",
                    status,
                    body.len(),
                    header_str
                );

                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            }
        });

        (server_url, handle)
    }

    #[tokio::test]
    async fn test_ota_staging_success_when_hash_matches() {
        let payload = b"SQUASHFS_VALID_PAYLOAD_IMAGE";
        let hash = beercade_shared::crypto::compute_sha256(payload);
        let (server_url, _) = spawn_mock_ota_server(200, Some(&hash), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-success");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), payload.len() as u64);
        assert!(staging_file.exists());
        assert_eq!(std::fs::read(&staging_file).unwrap(), payload);
    }

    #[tokio::test]
    async fn test_ota_staging_rejected_when_sha256_header_missing() {
        let payload = b"SQUASHFS_PAYLOAD_WITHOUT_CHECKSUM";
        let (server_url, _) = spawn_mock_ota_server(200, None, payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-missing");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("Missing mandatory X-OTA-SHA256 header"));
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_rejected_when_sha256_header_empty() {
        let payload = b"SQUASHFS_PAYLOAD_EMPTY_CHECKSUM";
        let (server_url, _) = spawn_mock_ota_server(200, Some("   "), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-empty");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("Empty X-OTA-SHA256 header"));
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_rejected_when_sha256_mismatched() {
        let payload = b"SQUASHFS_CORRUPTED_PAYLOAD";
        let bad_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let (server_url, _) = spawn_mock_ota_server(200, Some(bad_hash), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-mismatch");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("SHA-256 mismatch"));
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_cleanly_deletes_preexisting_file_on_rejection() {
        let payload = b"SQUASHFS_PAYLOAD_CORRUPT";
        let bad_hash = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let (server_url, _) = spawn_mock_ota_server(200, Some(bad_hash), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-cleanup");
        let staging_file = root.path().join("staging.sqsh");
        // Pre-create file to simulate stale or leftover data
        std::fs::write(&staging_file, b"OLD_STALE_CORRUPTED_DATA").unwrap();
        assert!(staging_file.exists());

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        // Staged file MUST be deleted from disk upon rejection
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_accepts_uppercase_sha256_header() {
        let payload = b"SQUASHFS_VALID_PAYLOAD_IMAGE";
        let hash_lower = beercade_shared::crypto::compute_sha256(payload);
        let hash_upper = hash_lower.to_ascii_uppercase();
        let (server_url, _) = spawn_mock_ota_server(200, Some(&hash_upper), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-uppercase");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_ok());
        assert!(staging_file.exists());
    }
}
```

---

## 5. Verification Method

1. **Independent Verification Commands**:
   - Run the dedicated OTA unit tests:
     ```powershell
     cargo test -p astro-daemon --lib ota::tests
     ```
   - Verify all tests pass with exit code 0:
     ```powershell
     cargo test --workspace
     ```
   - Verify clean build without warnings:
     ```powershell
     cargo check --workspace --tests
     ```

2. **Files to Inspect**:
   - `astro/daemon/src/ota.rs`: Inspect header validation, `drop(dest)` prior to `remove_file`, error messages, and tests.
   - `astro/daemon/src/main.rs`: Ensure caller site (`ota::execute_ota_update(...)`) remains compatible.

3. **Invalidation Conditions**:
   - If an OTA download with omitted `X-OTA-SHA256` header produces `Ok(())` or leaves any file on disk.
   - If an empty header returns `SHA-256 mismatch: expected , got ...` instead of explicit empty header rejection.
   - If `cargo test` triggers image build tools or requires external rootfs image creation.
