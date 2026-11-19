use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use reqwest::Client;
use sha2::{Sha256, Digest};

use beercade_hal::Platform;

// The system boots from one of two SquashFS slots; we OTA to the *other* slot
// and then flip the bootloader target so the active runtime is never touched
// mid-flash. Slot devices and the bootloader mechanism come from beercade-hal.

/// The payload is staged here first (on the same physical disk as the live
/// root so we can verify free space up front) and only after a successful
/// download + verification is it written to the inactive slot.
pub const STAGING_PATH: &'static str = "/data/beercade/sync/staging.sqsh";
pub const STAGING_DIR: &'static str = "/data/beercade/sync";

/// Always keep this much spare space above the payload size before flashing.
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
        let _ = tokio::fs::remove_file(staging_path).await;
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

    // Expect the server to advertise the size so we can budget disk up front.
    let expected_bytes = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(0);

    let staging_dir = staging_path.parent().unwrap_or(Path::new(STAGING_DIR));
    if let Err(e) = check_free_space(staging_dir, expected_bytes) {
        let _ = tokio::fs::remove_file(staging_path).await;
        return Err(e);
    }

    // Ensure staging directory exists
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

    // Drop open file handle before verifying or attempting removal to prevent Windows sharing violations
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

/// Downloads an OTA payload from BOSS, stages it with a free-space pre-flight
/// check, then writes it to the inactive boot slot and reboots onto it.
///
/// `boss_token` is forwarded as `Authorization: Bearer <token>` so that the
/// BOSS `/api/v1/sync/<filename>` endpoint accepts the request.
pub async fn execute_ota_update(boss_url: &str, filename: &str, boss_token: &str) -> Result<(), String> {
    // Safely detect A/B slots from the running environment (no mmcblk hardcodes).
    let platform = Platform::detect();
    let target = platform.active_slot().other();
    let inactive_slot = platform.slot_device(target);
    println!("[i] OTA Update Triggered: {} -> {}", filename, inactive_slot);

    let staging_path = Path::new(STAGING_PATH);
    let total_bytes = stage_ota_payload(boss_url, filename, boss_token, staging_path).await?;
    println!("[i] Staged {} bytes to {}", total_bytes, staging_path.display());

    // 3) Write the verified staging image to the inactive boot slot.
    flash_inactive_slot(STAGING_PATH, inactive_slot)?;

    // 4) Make the bootloader boot the freshly flashed slot, then reboot.
    platform.commit_boot_slot(target).map_err(|e| e.to_string())?;
    println!("[i] OTA applied to {}; rebooting onto the new image...", inactive_slot);
    let _ = std::process::Command::new("reboot").spawn();

    Ok(())
}

/// Confirm the staging directory has room for `required` bytes (plus a safety
/// margin) BEFORE we start pulling the payload off the network.
fn check_free_space(staging_dir: &Path, required: u64) -> Result<(), String> {
    if required == 0 {
        // Server did not advertise a size; skip the pre-flight (the flash step
        // still validates the write rather than trusting a size we never had).
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
            // df is unavailable on non-Linux dev hosts (e.g. Windows); allow execution
            return Ok(());
        }
    };

    if !output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8(output.stdout).unwrap_or_default();
    // `df --output=avail -B1 <dir>` prints a header line then a bare byte count.
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

/// Copy the verified staging image onto the inactive (non-mounted) boot slot.
///
/// Because the inactive slot is not the running root, a partial write cannot
/// corrupt the active runtime — and we only flip the boot target after `dd`
/// reports success, making the swap effectively atomic from the bootloader's
/// point of view.
fn flash_inactive_slot(staging_path: &str, inactive_slot: &str) -> Result<(), String> {
    let output = std::process::Command::new("dd")
        .arg(format!("if={}", staging_path))
        .arg(format!("of={}", inactive_slot))
        .arg("bs=1M")
        .arg("iflag=fullblock")
        .arg("conv=fsync")
        .arg("status=none")
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("failed to run dd: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "dd failed ({}); aborting OTA — active runtime untouched",
            output.status
        ));
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    pub struct FakeRoot(std::path::PathBuf);

    impl FakeRoot {
        pub fn new(prefix: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let id = N.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("beercade-test-{}-{}-{}", prefix, std::process::id(), id));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            FakeRoot(dir)
        }

        pub fn path(&self) -> &Path {
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
        let sha256_header = sha256_header.map(|s| s.to_string());

        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;

                let header_str = match sha256_header {
                    Some(sha) => format!("X-OTA-SHA256: {}\r\n", sha),
                    None => String::new(),
                };

                let status_line = match status {
                    200 => "HTTP/1.1 200 OK",
                    404 => "HTTP/1.1 404 Not Found",
                    500 => "HTTP/1.1 500 Internal Server Error",
                    _ => "HTTP/1.1 200 OK",
                };

                let response = format!(
                    "{}\r\n\
                     Connection: close\r\n\
                     Content-Length: {}\r\n\
                     {}\
                     \r\n",
                    status_line,
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

    async fn spawn_mock_ota_server_raw_header(
        status: u16,
        header_line_bytes: &[u8],
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{}", addr);
        let header_line = header_line_bytes.to_vec();

        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;

                let mut response = format!(
                    "HTTP/1.1 {} OK\r\n\
                     Connection: close\r\n\
                     Content-Length: {}\r\n",
                    status,
                    body.len()
                )
                .into_bytes();

                response.extend_from_slice(&header_line);
                response.extend_from_slice(b"\r\n");

                let _ = socket.write_all(&response).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            }
        });

        (server_url, handle)
    }

    #[tokio::test]
    async fn test_ota_staging_success_when_hash_matches() {
        let payload = b"SQUASHFS_VALID_PAYLOAD_IMAGE";
        let hash = beercade_shared::compute_sha256(payload);
        let (server_url, _) = spawn_mock_ota_server(200, Some(&hash), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-success");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_ok(), "Expected Ok, got {:?}", res);
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
        assert!(err.contains("Missing mandatory X-OTA-SHA256 header"), "Unexpected error: {}", err);
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
        assert!(err.contains("Empty X-OTA-SHA256 header"), "Unexpected error: {}", err);
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
        assert!(err.contains("SHA-256 mismatch"), "Unexpected error: {}", err);
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
    async fn test_ota_staging_cleanly_deletes_preexisting_file_on_missing_header() {
        let payload = b"SQUASHFS_PAYLOAD_WITHOUT_CHECKSUM";
        let (server_url, _) = spawn_mock_ota_server(200, None, payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-preexist-missing");
        let staging_file = root.path().join("staging.sqsh");
        std::fs::write(&staging_file, b"OLD_STALE_DATA").unwrap();
        assert!(staging_file.exists());

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_accepts_uppercase_sha256_header() {
        let payload = b"SQUASHFS_VALID_PAYLOAD_IMAGE";
        let hash_lower = beercade_shared::compute_sha256(payload);
        let hash_upper = hash_lower.to_ascii_uppercase();
        let (server_url, _) = spawn_mock_ota_server(200, Some(&hash_upper), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-uppercase");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_ok());
        assert!(staging_file.exists());
        assert_eq!(std::fs::read(&staging_file).unwrap(), payload);
    }

    #[tokio::test]
    async fn test_ota_staging_rejected_when_payload_is_empty() {
        let payload = b"";
        let hash = beercade_shared::compute_sha256(payload);
        let (server_url, _) = spawn_mock_ota_server(200, Some(&hash), payload.to_vec()).await;

        let root = FakeRoot::new("ota-test-empty-payload");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("payload is empty"), "Unexpected error: {}", err);
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_rejected_on_size_mismatch() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{}", addr);

        let body = b"SHORT_PAYLOAD".to_vec();
        let hash = beercade_shared::compute_sha256(&body);

        let _handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;

                // Advertise 500 bytes but only send 13 bytes
                let response = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Connection: close\r\n\
                     Content-Length: 500\r\n\
                     X-OTA-SHA256: {}\r\n\
                     \r\n",
                    hash
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            }
        });

        let root = FakeRoot::new("ota-test-size-mismatch");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("payload size mismatch") || err.contains("failed to stream OTA payload"),
            "Unexpected error: {}",
            err
        );
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_rejected_on_server_error_status() {
        let (server_url, _) = spawn_mock_ota_server(404, None, b"Not found".to_vec()).await;

        let root = FakeRoot::new("ota-test-server-err");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("Failed to download OTA payload"), "Unexpected error: {}", err);
        assert!(!staging_file.exists());
    }

    #[tokio::test]
    async fn test_ota_staging_rejected_on_invalid_utf8_header() {
        let raw_hdr = b"X-OTA-SHA256: \xFF\xFE\xFD\r\n";
        let (server_url, _) = spawn_mock_ota_server_raw_header(200, raw_hdr, b"PAYLOAD".to_vec()).await;

        let root = FakeRoot::new("ota-test-invalid-utf8");
        let staging_file = root.path().join("staging.sqsh");

        let res = stage_ota_payload(&server_url, "update.sqsh", "token", &staging_file).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("Invalid UTF-8 in X-OTA-SHA256 header"), "Unexpected error: {}", err);
        assert!(!staging_file.exists());
    }
}
