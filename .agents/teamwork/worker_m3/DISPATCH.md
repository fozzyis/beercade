# Dispatch: Worker M3 (A.S.T.R.O. Daemon Robustness & Network)
Scope:
- Requirement R1: Mandatory SHA-256 OTA verification & cleanup in astro/daemon/src/ota.rs
- Requirement R2: Atomic & crash-safe credit persistence & backup recovery in astro/daemon/src/main.rs
- Requirement R3: Dynamic network configuration (DHCP + IPv4LL fallback, removing static 192.168.100.2) in astro/rootfs/etc/systemd/network/10-wired.network and astro/rootfs/usr/local/bin/boss_static_net.sh
- Requirement R6: Automated unit and integration tests covering credit persistence atomic write/recovery, command dispatcher execution/ACK, and OTA staging rejection/cleanup in astro-daemon.
Exclusive files:
- astro/daemon/src/ota.rs
- astro/daemon/src/main.rs
- astro/rootfs/etc/systemd/network/10-wired.network
- astro/rootfs/usr/local/bin/boss_static_net.sh
See handoffs in explorer_m3_1, explorer_m3_2, explorer_m3_3.

## 2026-10-27T23:47:32Z
[Message] timestamp=2026-10-27T23:47:32Z sender=bd770878-29b7-48ec-a1e1-b5265681a707 priority=MESSAGE_PRIORITY_HIGH content=You are Worker M3 for Milestone 3 of B.E.E.R.C.A.D.E.
Working directory: d:\data\dev\beercade\.agents\teamwork\worker_m3

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

MANDATORY: Read the authoritative user request at:
d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md
Also read:
d:\data\dev\beercade\.agents\teamwork\PROJECT.md
d:\data\dev\beercade\.agents\teamwork\TEST_INFRA.md
Also read the detailed Explorer handoff reports:
- d:\data\dev\beercade\.agents\teamwork\explorer_m3_1\handoff.md
- d:\data\dev\beercade\.agents\teamwork\explorer_m3_2\handoff.md
- d:\data\dev\beercade\.agents\teamwork\explorer_m3_3\handoff.md

Your Scope for Milestone 3:
1. Requirement R1: Mandatory SHA-256 OTA verification & cleanup in `astro/daemon/src/ota.rs`:
   - Enforce mandatory `X-OTA-SHA256` header in `stage_ota_payload` / `execute_ota_update`.
   - If header is missing, empty, or invalid UTF-8: reject with explicit descriptive error and delete staged files.
   - If computed SHA-256 checksum does not match expected hash: reject with explicit mismatch error and delete staged files.
   - Drop file handles prior to deletion to avoid Windows file lock sharing violations (`ERROR_SHARING_VIOLATION`).
   - Add unit tests in `astro/daemon/src/ota.rs` covering: valid hash match, missing header rejection, empty header rejection, checksum mismatch rejection, deletion of pre-existing/partial staged files, and uppercase header normalization.
2. Requirement R2: Atomic & crash-safe credit persistence & backup recovery in `astro/daemon/src/main.rs`:
   - In `persist_credits`: serialize JSON, write to temporary file (`credits.json.tmp`), call `sync_all()` on the file descriptor, drop handle, atomically rename to `credits.json`, and update `credits.json.bak`.
   - In `load_credits`: implement tiered recovery. Check primary `credits.json`. If missing, truncated, or invalid JSON, attempt reading `credits.json.bak`, log a warning, self-heal `credits.json` from `credits.json.bak`, and return valid counters. If both are corrupted/missing, log an error and safely return `CreditCounters::default()` without panicking.
   - Add unit tests in `astro/daemon/src/main.rs` covering: atomic persistence happy path, recovery from truncated JSON, 0-byte file recovery, non-JSON corruption recovery, recovery when primary is deleted, graceful fallback when both files are corrupt, and rapid concurrent counter updates across multiple tasks.
3. Requirement R3: Dynamic network configuration (DHCP + IPv4LL fallback, removing static 192.168.100.2):
   - In `astro/rootfs/etc/systemd/network/10-wired.network`: remove `Address=192.168.100.2/24`, `Gateway=192.168.100.1`, and `DNS=...`. Keep `DHCP=yes` and `LinkLocalAddressing=fallback`.
   - In `astro/rootfs/usr/local/bin/boss_static_net.sh`: remove `ip addr replace 192.168.100.2/24` and `ip route replace default via 192.168.100.1`.
4. Requirement R6: Command dispatcher execution and ACK behavior tests in `astro-daemon`:
   - In `astro/daemon/src/main.rs`: Add unit tests for `execute_cabinet_command` and `ack_command` covering `Ping`, `AddCredits`, `StartSession`, `EndSession`, `Reboot`, `LaunchGame` validation, malformed payload error handling, mock server ACK dispatch, and assertions that `astro/rootfs` contains no static `192.168.100.2`.
5. Verification:
   - Run `cargo test -p astro-daemon` and verify that all existing tests + all new tests pass 100%.
   - Run `cargo test --workspace` to ensure 100% pass across all workspace crates.
   - STRICT CONSTRAINT: Do NOT run any image build commands (`make image-x86`, `mksquashfs`, Docker rootfs exports). Keep scope strictly on coding and testing.

Exclusive file ownership:
- `astro/daemon/src/ota.rs`
- `astro/daemon/src/main.rs`
- `astro/rootfs/etc/systemd/network/10-wired.network`
- `astro/rootfs/usr/local/bin/boss_static_net.sh`
Do NOT modify files outside this list!

Write your handoff report to:
`d:\data\dev\beercade\.agents\teamwork\worker_m3\handoff.md`
Report your status and results back to the orchestrator via `send_message`.
