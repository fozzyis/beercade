# Handoff Report — explorer_survey_1 (Astro & Network Explorer)

## 1. Observation
1. **R1 (OTA Hash Verification)**:
   - File `d:\data\dev\beercade\astro\daemon\src\ota.rs:70-82`:
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
   - In `boss\src\ota_sync.rs:75-80`, BOSS only sets header `"X-OTA-SHA256"` if `<filename>.sha256` exists on disk:
     ```rust
     let sha256_path = format!("{}.sha256", path_str);
     if let Ok(sha256_hex) = tokio::fs::read_to_string(&sha256_path).await {
         if !sha256_hex.trim().is_empty() {
             resp = resp.header("X-OTA-SHA256", sha256_hex.trim());
         }
     }
     ```
   - If the header is missing from the response, `expected_sha256` is `None` in `ota.rs`. The `if let Some(expected)` condition evaluates to `false`, silently skipping verification.
   - In `astro\daemon\src\ota.rs:63-68`, if streaming fails (`response.chunk().await?`, `dest.write_all()?`, `dest.sync_all()?`), the function returns with `?` without calling `tokio::fs::remove_file(STAGING_PATH).await`, leaving orphaned partial files in `/data/beercade/sync/staging.sqsh`.

2. **R2 (Credit Persistence & Recovery)**:
   - File `d:\data\dev\beercade\astro\daemon\src\main.rs:49-56`:
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
   - File `d:\data\dev\beercade\astro\daemon\src\main.rs:40-47`:
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
   - `std::fs::write(&self.credits_path, json)` directly overwrites the target file without a temporary file and without calling `sync_all()`.
   - On power failure during write, `credits.json` is left truncated or corrupt.
   - On daemon startup, `load_credits` silently falls back to `CreditCounters::default()` (`credits_available = 0, credits_inserted_total = 0`). The next coin insert or credit change calls `persist_credits()`, overwriting `credits.json` and permanently erasing the cabinet's historical coin counts.

3. **R3 (Hardcoded Static IP Conflicts)**:
   - File `d:\data\dev\beercade\astro\rootfs\etc\systemd\network\10-wired.network:6-8`:
     ```ini
     [Network]
     Address=192.168.100.2/24
     Gateway=192.168.100.1
     DNS=192.168.100.1 1.1.1.1 8.8.8.8
     ```
   - File `d:\data\dev\beercade\astro\rootfs\usr\local\bin\boss_static_net.sh:12-13`:
     ```sh
     ip addr replace 192.168.100.2/24 dev "$IFACE" || true
     ip route replace default via 192.168.100.1 dev "$IFACE" || true
     ```
   - File `d:\data\dev\beercade\shared\arch\systemd\10-wired.network:5-10`:
     ```ini
     [Network]
     DHCP=yes
     DNSSEC=no
     LinkLocalAddressing=fallback
     MulticastDNS=yes
     LLMNR=yes
     ```
   - Exact locations where `192.168.100.2` is hardcoded across the entire repo: only `astro/rootfs/etc/systemd/network/10-wired.network:6` and `astro/rootfs/usr/local/bin/boss_static_net.sh:12`.

4. **Test Suite Status**:
   - Command: `cargo test --workspace`
   - Output: 35 passing tests (astro-daemon: 10 in netplay, astro-tune: 5, beercade-hal: 9, beercade-shared: 7, boss-server: 4).
   - Zero tests currently exist for `ota.rs` payload verification, zero tests for `load_credits`/`persist_credits`, and zero tests for command dispatcher credit handling.

---

## 2. Logic Chain
1. **R1**:
   - Because line 76 in `ota.rs` checks `if let Some(expected) = expected_sha256`, any payload download where the server omits `X-OTA-SHA256` bypasses hash verification completely.
   - Therefore, missing or empty headers must be treated as a hard rejection (`Err`), staged files must be removed, and verification must occur before flashing.
2. **R2**:
   - Because `std::fs::write` truncates the file in place and does not `sync_all`, an interrupted write leaves `credits.json` empty or with broken JSON syntax.
   - Because `load_credits` returns `CreditCounters::default()` when `from_str` fails, any subsequent `persist_credits` permanently wipes `credits_inserted_total`.
   - Therefore, writes must go to `credits.json.tmp` on the same directory, execute `sync_all()`, maintain `credits.json.bak`, and atomically `rename`. Startup loading must attempt primary, then backup, then tmp, before defaulting.
3. **R3**:
   - Because `systemd-networkd` respects static `Address=` entries even when `DHCP=yes`, every x86 cabinet boots with `192.168.100.2/24` assigned.
   - When multiple cabinets connect to the same venue switch, an IP collision occurs immediately.
   - Therefore, removing `Address=` and `Gateway=` from `10-wired.network` and replacing static assignments in `boss_static_net.sh` allows `DHCP=yes` and `LinkLocalAddressing=fallback` to provide dynamic, collision-free IP addressing with IPv4LL fallback.

---

## 3. Caveats
- No image builds or QEMU runs were executed, strictly adhering to the constraint ("Do NOT run image builds").
- Network behavior was verified via file inspection of `systemd-networkd` configuration and Netlink scripts, cross-referenced with `shared/arch/systemd/10-wired.network`.
- Server-side BOSS token and PIN rate-limiting requirements (R5) were not part of this explorer's assigned scope (covered by other survey tasks).

---

## 4. Conclusion
- **R1**: Requires changing `ota.rs` to make `X-OTA-SHA256` mandatory (validate presence and non-empty), delete `STAGING_PATH` on all errors, and extract a testable streaming/verification function for unit testing.
- **R2**: Requires updating `persist_credits` to write to `credits.json.tmp`, call `file.sync_all()`, back up existing `credits.json` to `credits.json.bak`, and atomically rename. `load_credits` must recover from `credits.json.bak` and quarantine corrupted files.
- **R3**: Requires deleting `Address=192.168.100.2/24` and `Gateway=192.168.100.1` from `astro/rootfs/etc/systemd/network/10-wired.network`, and removing `ip addr replace 192.168.100.2/24` from `astro/rootfs/usr/local/bin/boss_static_net.sh`.

---

## 5. Verification Method
1. **Verification of Findings**:
   - Inspect `d:\data\dev\beercade\astro\daemon\src\ota.rs` lines 70–82.
   - Inspect `d:\data\dev\beercade\astro\daemon\src\main.rs` lines 40–56.
   - Inspect `d:\data\dev\beercade\astro\rootfs\etc\systemd\network\10-wired.network` lines 5–8.
   - Inspect `d:\data\dev\beercade\astro\rootfs\usr\local\bin\boss_static_net.sh` lines 10–14.
2. **Post-Implementation Verification**:
   - Run `cargo test -p astro-daemon` to ensure unit tests for OTA staging rejection, atomic persistence, truncated file recovery, and rapid counter updates pass.
   - Run `cargo test --workspace` to ensure all crates pass without regression.
   - Search repository for `192.168.100.2` to ensure zero occurrences remain in network configurations.
