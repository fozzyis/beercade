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
const STAGING_PATH: &'static str = "/data/beercade/sync/staging.sqsh";
const STAGING_DIR: &'static str = "/data/beercade/sync";

/// Always keep this much spare space above the payload size before flashing.
const SPARE_MARGIN_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

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

    // Expect the server to advertise the size so we can budget disk up front.
    let expected_bytes = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(0);

    // 1) Free-space gate BEFORE pulling any bytes.
    check_free_space(STAGING_DIR, expected_bytes)?;

    // 2) Stream into the staging area — never write to the live root up front.
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

    // Verify we got the whole image before we risk a flash.
    if expected_bytes > 0 && total_bytes != expected_bytes {
        let _ = tokio::fs::remove_file(STAGING_PATH).await;
        return Err(format!(
            "payload size mismatch: expected {} bytes, got {}",
            expected_bytes,
            total_bytes
        ));
    }
    if total_bytes == 0 {
        let _ = tokio::fs::remove_file(STAGING_PATH).await;
        return Err("payload is empty; refusing to flash".to_string());
    }
    println!("[i] Staged {} bytes to {}", total_bytes, STAGING_PATH);

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
fn check_free_space(staging_dir: &str, required: u64) -> Result<(), String> {
    if required == 0 {
        // Server did not advertise a size; skip the pre-flight (the flash step
        // still validates the write rather than trusting a size we never had).
        return Ok(());
    }
    let needed = required + SPARE_MARGIN_BYTES;
    let output = std::process::Command::new("df")
        .arg("--output=avail")
        .arg("-B1")
        .arg(staging_dir)
        .output()
        .map_err(|e| format!("failed to probe free space via df: {e}"))?;
    let stdout = String::from_utf8(output.stdout.clone()).unwrap_or_default();
    // `df --output=avail -B1 <dir>` prints a header line then a bare byte count.
    let avail = stdout
        .split_whitespace()
        .last()
        .and_then(|tok| tok.parse::<u64>().ok())
        .unwrap_or(0);

    if avail < needed {
        return Err(format!(
            "insufficient free space on {}: need {} bytes (payload {} + margin) but only {} available",
            staging_dir,
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
