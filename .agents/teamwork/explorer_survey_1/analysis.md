# Comprehensive Analysis: R1, R2, and R3 in B.E.E.R.C.A.D.E.

**Agent**: `explorer_survey_1` (A.S.T.R.O. Daemon & Network Explorer)  
**Date**: 2026-10-27  
**Scope**: Requirements R1 (OTA Hash Verification), R2 (Atomic Credit Persistence & Recovery), R3 (Remove Hardcoded Static IP Conflicts)  
**Integrity Mode**: Development / Read-Only Investigation  

---

## 1. Executive Summary

| Requirement | Target Files | Primary Vulnerability / Defect | Recommended Resolution |
|---|---|---|---|
| **R1: Enforce OTA Hash Verification** | `astro/daemon/src/ota.rs`<br>`boss/src/ota_sync.rs` | Missing `X-OTA-SHA256` header is silently accepted without verification; hashing check is wrapped in `if let Some(expected)`; failed chunk reads/syncs leave partial files on disk. | Make `X-OTA-SHA256` mandatory (validate presence and non-empty up-front and post-stream); clean up `STAGING_PATH` on all error branches; return explicit `Err`. |
| **R2: Atomic & Crash-Safe Credit Persistence** | `astro/daemon/src/main.rs`<br>`beercade-shared/src/models.rs` | `persist_credits` directly writes to `credits.json` with no temp file and no `sync_all()`; `load_credits` silently zeroes counters on corrupted or truncated files, causing permanent loss of lifetime revenue metrics (`credits_inserted_total`). | Write to `credits.json.tmp` on the same directory, call `sync_all()`, maintain `credits.json.bak`, atomically rename; implement tiered fallback in `load_credits` (primary -> `.bak` -> `.tmp` -> quarantine `.corrupt.<ts>`). |
| **R3: Remove Hardcoded Static IP Conflicts** | `astro/rootfs/etc/systemd/network/10-wired.network`<br>`astro/rootfs/usr/local/bin/boss_static_net.sh`<br>`astro/rootfs/etc/systemd/system/boss-static-net.service` | `192.168.100.2/24` is hardcoded in `10-wired.network` (overriding DHCP) and `boss_static_net.sh` (Netlink static assignment), causing guaranteed LAN IP collisions when multiple x86 cabinets are connected to the same switch. | Remove `Address=192.168.100.2/24` and gateway from `10-wired.network`, aligning with `shared/arch/systemd/10-wired.network` (`DHCP=yes`, `LinkLocalAddressing=fallback`); remove `ip addr replace 192.168.100.2/24` from `boss_static_net.sh`. |

---

## 2. Requirement R1: OTA Hash Verification

### 2.1 Current Implementation in `astro/daemon/src/ota.rs`
The entry point for client-side OTA updates is `execute_ota_update`:
- **Function signature** (`astro/daemon/src/ota.rs:26`):
  ```rust
  pub async fn execute_ota_update(boss_url: &str, filename: &str, boss_token: &str) -> Result<(), String>
  ```
- **Invocation** (`astro/daemon/src/main.rs:715`):
  Triggered when the node receives an SSE or WebSocket message `{"kind": "payload_available", "filename": ...}` from BOSS.

### 2.2 Payload Staging Workflow
1. **Target Slot Determination** (`ota.rs:28-30`):
   Uses `beercade_hal::Platform::detect()` to inspect the running root device, determines the inactive slot (`platform.active_slot().other()`), and retrieves the target block device (`platform.slot_device(target)`).
2. **Download URL Construction** (`ota.rs:33`):
   Calls `beercade_shared::ipc::format_ota_download_url(boss_url, filename)`, resolving to `{boss_url}/api/v1/sync/{filename}`.
3. **HTTP GET & Bearer Auth** (`ota.rs:35-44`):
   Executes `reqwest::Client::new().get(&url).bearer_auth(boss_token).send().await`. Rejects non-2xx statuses with `Err("Failed to download OTA payload: {status}")`.
4. **Content-Length Pre-flight Check** (`ota.rs:47-56`):
   Parses `content-length` header. Calls `check_free_space(STAGING_DIR, expected_bytes)` which runs `df --output=avail -B1 /data/beercade/sync` and verifies available bytes exceed `expected_bytes + 64 MiB` (`SPARE_MARGIN_BYTES`).
5. **Streaming to Disk & On-The-Fly Hashing** (`ota.rs:58-68`):
   Opens `STAGING_PATH = "/data/beercade/sync/staging.sqsh"` via `tokio::fs::File::create`. Reads stream via `while let Some(chunk) = response.chunk().await?`, writes to file via `dest.write_all(&chunk).await?`, and updates `hasher.update(&chunk)`. Calls `dest.sync_all().await?`.

### 2.3 Vulnerability: Skipped SHA-256 Check
Lines 70–82 in `astro/daemon/src/ota.rs`:
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
**Defects Identified**:
1. **Header Absence**: If the `X-OTA-SHA256` header is omitted by BOSS (for instance, if `<payload>.sha256` was not generated or read failed in `boss/src/ota_sync.rs:76`), `expected_sha256` resolves to `None`. The `if let Some(expected)` condition evaluates to `false`, silently skipping verification entirely.
2. **Empty Header**: If `X-OTA-SHA256` is empty or whitespace, `trim()` produces `""`. While `actual != ""` triggers an error, the error message reads `SHA-256 mismatch: expected , got <hash>` rather than reporting a missing/invalid header.
3. **Non-ASCII Characters**: If the header contains invalid UTF-8 bytes, `to_str().ok()` yields `None`, which also causes verification to be skipped.
4. **Deferred Check Waste**: Even though HTTP headers are available immediately upon receiving the response headers (before reading response body chunks), the check is deferred until after the entire multi-gigabyte SquashFS file is streamed and written to disk.

### 2.4 Staging File Cleanup Gaps
`tokio::fs::remove_file(STAGING_PATH).await` is only called if:
- Hash mismatch occurs (line 79)
- Size mismatch occurs (line 86)
- Total bytes is 0 (line 94)

`STAGING_PATH` is **NOT** cleaned up if:
- Network error occurs during chunk reading (`response.chunk().await?` at line 63)
- Write error occurs (`dest.write_all(&chunk).await?` at line 64)
- Flush error occurs (`dest.sync_all().await?` at line 68)
- Flashing the inactive slot fails (`flash_inactive_slot` at line 100)
- Committing boot slot fails (`platform.commit_boot_slot` at line 103)

### 2.5 Server-Side Behavior (`boss/src/ota_sync.rs`)
- On upload (`payload_upload_handler`, line 98): BOSS computes SHA-256 on the incoming stream and saves it to `/data/beercade/sync/<filename>.sha256` (lines 209–213).
- On download (`payload_download_handler`, line 46): BOSS reads `/data/beercade/sync/<filename>.sha256`. If present and non-empty, it adds header `"X-OTA-SHA256"` (lines 75–80).
- If the `.sha256` file does not exist, BOSS does NOT send `X-OTA-SHA256`.

### 2.6 Implementation Proposal for R1
1. **Mandatory Header Validation Up-Front**:
   Before creating `STAGING_PATH` or streaming chunks, inspect `response.headers().get("x-ota-sha256")`:
   ```rust
   let expected_sha256 = response
       .headers()
       .get("x-ota-sha256")
       .and_then(|v| v.to_str().ok())
       .map(|s| s.trim().to_lowercase())
       .filter(|s| !s.is_empty())
       .ok_or_else(|| {
           "missing or empty X-OTA-SHA256 header; refusing to stage unverified OTA payload".to_string()
       })?;
   ```
2. **Guaranteed Cleanup**:
   Ensure `let _ = tokio::fs::remove_file(STAGING_PATH).await;` is called whenever any error occurs after `STAGING_PATH` is created.
3. **Modular Function for Unit Testing**:
   Extract payload staging and verification logic into a testable function:
   ```rust
   pub async fn stage_and_verify_stream<S, E>(
       mut stream: S,
       expected_len: u64,
       expected_sha256: &str,
       dest_path: &Path,
   ) -> Result<u64, String>
   ```
   This allows unit tests to feed byte streams (with correct, corrupt, or missing hashes) into a temporary staging path and verify rejection and cleanup without calling hardware `dd` or `reboot`.

---

## 3. Requirement R2: Atomic & Crash-Safe Credit Persistence

### 3.1 Current Implementation in `astro/daemon/src/main.rs`
Credit data is managed by `DaemonSharedState`:
- **State Definition** (`main.rs:29-37`):
  ```rust
  pub struct DaemonSharedState {
      ...
      pub credits: CreditCounters,
      pub credits_path: PathBuf,
  }
  ```
- **Configuration** (`main.rs:620-624`):
  ```rust
  let credits_dir = std::env::var("ASTRO_DATA_DIR")
      .map(PathBuf::from)
      .unwrap_or_else(|_| PathBuf::from("/data/beercade"));
  let credits_path = credits_dir.join("credits.json");
  let initial_credits = DaemonSharedState::load_credits(&credits_path);
  ```
- **Persistence Function** (`main.rs:49-56`):
  ```rust
  pub fn persist_credits(&self) {
      if let Some(parent) = self.credits_path.parent() {
          let _ = std::fs::create_dir_all(parent);
      }
      if let Ok(json) = serde_json::to_string_pretty(&self.credits) {
          let _ = std::fs::write(&self.credits_path, json);
      }
  }
  ```
- **Loading Function** (`main.rs:40-47`):
  ```rust
  pub fn load_credits(path: &Path) -> CreditCounters {
      if let Ok(data) = std::fs::read_to_string(path) {
          if let Ok(c) = serde_json::from_str::<CreditCounters>(&data) {
              return c;
          }
      }
      CreditCounters::default()
  }
  ```

### 3.2 Analysis of Failure Modes
1. **Direct Overwrite via `std::fs::write`**:
   `std::fs::write` truncates the file immediately upon opening (`O_TRUNC`). If power is lost or the daemon crashes while writing, `credits.json` is left partially written or 0 bytes.
2. **Absence of `sync_all`**:
   `std::fs::write` closes the file without calling `fsync` (`sync_all`). On sudden power removal, uncommitted disk caches are lost.
3. **Silent Destruction of Accounting Data on Corruption**:
   When `credits.json` is truncated or corrupted, `serde_json::from_str::<CreditCounters>` fails. `load_credits` immediately falls back to `CreditCounters::default()` (`credits_available = 0`, `credits_inserted_total = 0`). The next time `persist_credits` is called (e.g. coin drop or API command), it overwrites `credits.json` with zeros, permanently destroying historical coin drop accounting.
4. **No Backup / Recovery Mechanism**:
   No backup file (e.g. `credits.json.bak`) is created, and no recovery logic exists.

### 3.3 Target File Paths
- Primary: `/data/beercade/credits.json` (or `$ASTRO_DATA_DIR/credits.json`)
- Temporary: `/data/beercade/credits.json.tmp` (MUST be in the same directory as primary to ensure `rename` is an atomic single-filesystem inode update, avoiding `EXDEV`)
- Backup: `/data/beercade/credits.json.bak`
- Quarantine for unrecoverable corrupted data: `/data/beercade/credits.json.corrupt.<timestamp>`

### 3.4 Implementation Proposal for R2
1. **Crash-Safe `persist_credits` Algorithm**:
   ```rust
   pub fn persist_credits(&self) {
       let parent = match self.credits_path.parent() {
           Some(p) => p,
           None => return,
       };
       let _ = std::fs::create_dir_all(parent);

       let json = match serde_json::to_string_pretty(&self.credits) {
           Ok(j) => j,
           Err(e) => {
               tracing::error!("Failed to serialize credits: {}", e);
               return;
           }
       };

       let tmp_path = self.credits_path.with_file_name("credits.json.tmp");
       let bak_path = self.credits_path.with_file_name("credits.json.bak");

       // 1. Write to temporary file
       let write_res = (|| -> std::io::Result<()> {
           let mut file = std::fs::File::create(&tmp_path)?;
           std::io::Write::write_all(&mut file, json.as_bytes())?;
           file.sync_all()?;
           Ok(())
       })();

       if let Err(e) = write_res {
           tracing::error!("Failed to write and sync temporary credits: {}", e);
           let _ = std::fs::remove_file(&tmp_path);
           return;
       }

       // 2. Maintain backup if primary exists and is valid
       if self.credits_path.exists() {
           let _ = std::fs::copy(&self.credits_path, &bak_path);
       }

       // 3. Atomically rename tmp into primary
       if let Err(e) = std::fs::rename(&tmp_path, &self.credits_path) {
           tracing::error!("Failed to atomically rename credits file: {}", e);
           return;
       }

       // 4. On Unix, fsync parent directory to flush directory entry
       #[cfg(unix)]
       if let Ok(dir) = std::fs::File::open(parent) {
           let _ = dir.sync_all();
       }
   }
   ```
2. **Tiered Recovery in `load_credits`**:
   - **Tier 1 (Primary)**: If `credits.json` exists and parses cleanly, return it.
   - **Tier 2 (Backup)**: If primary is missing, truncated (0 bytes), or invalid JSON, check `credits.json.bak`. If `.bak` is valid, log a warning, copy `.bak` -> `credits.json`, and return it.
   - **Tier 3 (Interrupted Tmp)**: If primary and backup fail, check `credits.json.tmp`. If valid, log a warning, rename `.tmp` -> `credits.json`, and return it.
   - **Tier 4 (Quarantine & Default)**: If primary exists but is irreparably corrupt, rename to `credits.json.corrupt.<epoch_ms>` for forensic recovery, and return `CreditCounters::default()`.

---

## 4. Requirement R3: Remove Hardcoded Static IP Conflicts

### 4.1 Root Cause & File Inspection

#### 4.1.1 `astro/rootfs/etc/systemd/network/10-wired.network`
```ini
[Match]
Name=eth* en* end* lan*
Type=ether

[Network]
Address=192.168.100.2/24
Gateway=192.168.100.1
DNS=192.168.100.1 1.1.1.1 8.8.8.8
DNSSEC=no
MulticastDNS=yes
DHCP=yes
LinkLocalAddressing=fallback
LLDP=no
EmitLLDP=no
IPv6AcceptRA=no
ConfigureWithoutCarrier=yes

[DHCPv4]
RouteMetric=100
UseDNS=yes
```
- **Conflict**: Line 6 sets `Address=192.168.100.2/24` and Line 7 sets `Gateway=192.168.100.1`.
- In `systemd-networkd`, specifying `Address=` creates an unconditional static IPv4 assignment. Even though `DHCP=yes` is set, `systemd-networkd` retains the static address on the interface.
- If two cabinets (e.g. Cabinet A and Cabinet B) are connected to the same switch, both assign `192.168.100.2/24` to their Ethernet interface, causing ARP collisions and breaking network communication.

#### 4.1.2 `astro/rootfs/usr/local/bin/boss_static_net.sh`
```sh
#!/bin/sh
# Fallback static network setup for end0 via Netlink
# Runs independently of systemd-networkd
IFACE="end0"
if [ ! -d "/sys/class/net/$IFACE" ]; then
    IFACE=$(ls -1 /sys/class/net | grep -E '^(en|eth|end)' | head -n 1 || true)
fi

if [ -n "$IFACE" ] && [ -d "/sys/class/net/$IFACE" ]; then
    echo "[*] Fallback: Configuring static IP on $IFACE..."
    ip link set "$IFACE" up || true
    ip addr replace 192.168.100.2/24 dev "$IFACE" || true
    ip route replace default via 192.168.100.1 dev "$IFACE" || true
fi
exit 0
```
- **Conflict**: Line 12 (`ip addr replace 192.168.100.2/24 dev "$IFACE"`) and Line 13 (`ip route replace default via 192.168.100.1 dev "$IFACE"`).
- Called by `astro/rootfs/etc/systemd/system/boss-static-net.service`.
- Even if `systemd-networkd` was running dynamic DHCP, this script explicitly replaces any assigned IP with `192.168.100.2`.

### 4.2 Canonical Reference Configuration
The repository already contains a clean, conflict-free configuration in `shared/arch/systemd/10-wired.network`:
```ini
[Match]
Name=eth* en* end* lan*
Type=ether

[Network]
DHCP=yes
DNSSEC=no
LinkLocalAddressing=fallback
MulticastDNS=yes
LLMNR=yes

[DHCPv4]
MaxAttempts=infinity
UseDNS=yes
RouteMetric=100

[DHCPv6]
UseDNS=yes
RouteMetric=100
```

### 4.3 Implementation Proposal for R3
1. **Update `astro/rootfs/etc/systemd/network/10-wired.network`**:
   - Delete `Address=192.168.100.2/24`
   - Delete `Gateway=192.168.100.1`
   - Delete `DNS=192.168.100.1 ...` (or retain public fallback DNS `DNS=1.1.1.1 8.8.8.8` without the unreachable `192.168.100.1`)
   - Ensure `DHCP=yes` and `LinkLocalAddressing=fallback` are active.
   - Set `MaxAttempts=infinity` under `[DHCPv4]` so `systemd-networkd` continuously attempts DHCP if a router is plugged in later.
2. **Update `astro/rootfs/usr/local/bin/boss_static_net.sh`**:
   - Remove `ip addr replace 192.168.100.2/24 dev "$IFACE"` and default route replacement.
   - Modify the script so it only brings the link up (`ip link set "$IFACE" up || true`), allowing `systemd-networkd` to manage dynamic assignment and link-local fallback.
3. **Verify Link-Local (IPv4LL RFC 3927) Dynamics**:
   - When a DHCP server is present: station receives a dynamic DHCP address.
   - When no DHCP server is present: `systemd-networkd` automatically claims an address in `169.254.0.0/16` via ARP probing.
   - Stations publish their hostname on the local subnet via `MulticastDNS=yes` (e.g. `astro-01.local`), allowing BOSS and peer nodes to discover each other dynamically without IP collisions.

---

## 5. Test Plan for Requirements R1, R2, R3 (Covering R6)

1. **R1 Tests (`astro/daemon/src/ota.rs`)**:
   - `test_ota_rejects_missing_sha256_header`: Create mock HTTP response without `X-OTA-SHA256`; verify payload download is aborted, `STAGING_PATH` is deleted/not created, and `Err` contains explicit message.
   - `test_ota_rejects_empty_sha256_header`: Create mock HTTP response with empty or whitespace `X-OTA-SHA256`; verify update is rejected and cleaned up.
   - `test_ota_rejects_checksum_mismatch`: Create mock HTTP response with mismatched SHA-256; verify staging file is deleted and error returned.
   - `test_ota_accepts_valid_checksum`: Provide valid payload matching `X-OTA-SHA256`; verify payload verifies successfully.
2. **R2 Tests (`astro/daemon/src/main.rs`)**:
   - `test_credit_persistence_atomic_write`: Persist credits to a temporary directory; verify `credits.json` is created, valid JSON, and matches in-memory counters.
   - `test_credit_recovery_from_truncated_file`: Write a 0-byte or incomplete `credits.json`, with a valid `credits.json.bak`; call `load_credits`; verify data is restored from backup.
   - `test_credit_recovery_from_corrupt_file`: Write garbage text into `credits.json`, with a valid `credits.json.bak`; verify clean recovery without panic.
   - `test_credit_rapid_counter_updates`: Concurrently or rapidly call `add_credits` and `consume_credit` 100 times; verify file integrity and that final counter matches exactly.
3. **R3 Verification**:
   - Inspect `10-wired.network` and `boss_static_net.sh` to ensure no instances of `192.168.100.2` remain.
   - Verify `systemd-networkd` configuration syntax.
