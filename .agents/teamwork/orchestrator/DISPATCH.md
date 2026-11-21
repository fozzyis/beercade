# Dispatch History

## 2026-10-27T13:41:09Z
Sender: f38180cf-c1d9-4969-9764-3db3c157284e (Sentinel)
Priority: MESSAGE_PRIORITY_HIGH

You are the Project Orchestrator for B.E.E.R.C.A.D.E. RC1 blockers implementation and verification.

Your working directory is:
d:\data\dev\beercade\.agents\teamwork\orchestrator

The authoritative requirements file is at:
d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md

Project root:
d:\data\dev\beercade

Mission & Requirements:
Implement and verify the required x86_64 RC1 blockers and core robustness fixes in B.E.E.R.C.A.D.E. (A.S.T.R.O. daemon, B.O.S.S. server, and shared crates), including comprehensive automated unit and integration tests, without building target OS disk images.

### R1. Enforce OTA Hash Verification
- Make SHA-256 header verification mandatory during OTA update staging in astro/daemon/src/ota.rs.
- If the X-OTA-SHA256 header is missing, empty, or fails checksum validation, reject the update, delete staged files, and return an explicit error.

### R2. Atomic & Crash-Safe Credit Persistence
- Ensure persist_credits in astro/daemon/src/main.rs writes to a temporary file (credits.json.tmp), calls sync_all on the file descriptor, and atomically renames the file into place.
- Implement backup/recovery handling for corrupted or truncated credit files on boot.

### R3. Remove Hardcoded Static IP Conflicts
- Remove hardcoded static IP assignments (192.168.100.2) from the x86 network configuration (astro/rootfs/etc/systemd/network/10-wired.network and astro/rootfs/usr/local/bin/boss_static_net.sh) to prevent LAN address collisions across multiple cabinets.
- Ensure nodes use dynamic DHCP assignment with link-local fallback.

### R4. Clean Telemetry Contract for Thermals
- Refactor thermal telemetry in beercade-shared, astro-daemon, and boss so that unavailable sensor readings are represented cleanly as Option<f32> (or documented sentinel) rather than clamping to an artificial fallback value (42.0°C).
- Ensure telemetry validation accepts missing/None thermals without rejecting heartbeats.

### R5. B.O.S.S. Operator PIN Brute-Force Rate Limiting
- Add rate limiting and failure tracking to the B.O.S.S. operator PIN unlock endpoint (POST /api/v1/auth/pin) in boss/src/routes.rs and boss/src/state.rs to protect against local network brute-force attacks.

### R6. Automated Test Suite Expansion
- Add and run unit and integration tests covering:
  - Credit persistence atomic write, truncated file recovery, and rapid counter updates.
  - Command dispatcher execution and ACK behavior.
  - OTA staging rejection when X-OTA-SHA256 header is absent or incorrect.
  - Operator PIN rate limiting and lockout thresholds.
- Do NOT run image builds (make image-x86, mksquashfs, Docker rootfs exports). Keep scope strictly on code and tests.

## 2026-10-27T14:18:59Z
Sender: f38180cf-c1d9-4969-9764-3db3c157284e (Sentinel)
Priority: MESSAGE_PRIORITY_HIGH

URGENT USER INSTRUCTION (2026-10-27T14:18:16Z):
Cease work briefly until user gives the go ahead. Pause all further milestone dispatches, active worker runs, and edits immediately. Hold all execution and remain in standby until explicitly instructed to resume.
