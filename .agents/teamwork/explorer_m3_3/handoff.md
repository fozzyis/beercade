# Handoff Report: Requirement R3 (Dynamic Network Config) & Requirement R6 (Command Dispatcher Execution)

**Agent**: Explorer M3-3 (`explorer_m3_3`)  
**Type**: Hard Handoff (Investigation & Synthesis Complete)  
**Target Milestone**: Milestone 3  
**Working Directory**: `d:\data\dev\beercade\.agents\teamwork\explorer_m3_3`  
**Referenced Analysis**: `d:\data\dev\beercade\.agents\teamwork\explorer_m3_3\analysis.md`

---

## 1. Observation

1. **`astro/rootfs/etc/systemd/network/10-wired.network`** lines 5-16:
   ```ini
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
   ```
   Observed that `Address=192.168.100.2/24` and `Gateway=192.168.100.1` are explicitly configured alongside `DHCP=yes` and `LinkLocalAddressing=fallback`.

2. **`astro/rootfs/usr/local/bin/boss_static_net.sh`** lines 9-14:
   ```sh
   if [ -n "$IFACE" ] && [ -d "/sys/class/net/$IFACE" ]; then
       echo "[*] Fallback: Configuring static IP on $IFACE..."
       ip link set "$IFACE" up || true
       ip addr replace 192.168.100.2/24 dev "$IFACE" || true
       ip route replace default via 192.168.100.1 dev "$IFACE" || true
   fi
   ```
   Observed that this script forcefully replaces any assigned IP on `$IFACE` with `192.168.100.2/24` and replaces default route via `192.168.100.1`.

3. **`astro/rootfs/etc/systemd/system/boss-static-net.service`** line 8:
   `ExecStart=/usr/local/bin/boss_static_net.sh`
   Cross-referenced against `shared/buildfarm/x86_64/payloads/astro.conf` line 50 (`ENABLE_UNITS`): `boss-static-net.service` is not in `ENABLE_UNITS`, but exists in `/etc/systemd/system`.

4. **Reference Implementation in `shared/buildfarm/scripts/assemble_and_flash_rootfs.sh`** lines 99-105:
   ```ini
   [Network]
   DHCP=yes
   DNSSEC=no
   LinkLocalAddressing=fallback
   MulticastDNS=yes
   LLMNR=yes
   ```
   Observed that the ARM rootfs staging script configures `10-wired.network` without any static IP or gateway, using pure `DHCP=yes` and `LinkLocalAddressing=fallback`.

5. **`astro/daemon/src/main.rs`** lines 330-405:
   `execute_cabinet_command(cmd: &CabinetCommand, state_arc: &Arc<RwLock<DaemonSharedState>>, client: &reqwest::Client) -> (bool, Option<serde_json::Value>)` handles all 6 variants:
   - `CabinetCommand::Ping { seq }`: returns `(true, Some(json!({ "seq": seq })))`.
   - `CabinetCommand::AddCredits { credits }`: calls `st.add_credits(*credits)`, returns `(true, Some(json!({ "credits_added": credits, "credits_available": available })))`.
   - `CabinetCommand::StartSession { session_id, duration_secs }`: sets `st.mode = StationMode::Managed { ... }`, returns `(true, Some(json!({ "session_id": session_id, "duration_secs": duration_secs })))`.
   - `CabinetCommand::EndSession`: sets `st.mode = StationMode::Locked { ... }`, invokes `terminate_running_game().await` and `resume_frontend().await`, returns `(true, Some(json!({ "status": "session_ended" })))`.
   - `CabinetCommand::Reboot { delay_secs }`: spawns delayed `systemctl reboot` task (under `#[cfg(target_os = "linux")]`), returns immediately `(true, Some(json!({ "delay_secs": delay })))`.
   - `CabinetCommand::LaunchGame { core, rom, netplay }`: validates non-empty `core` and `rom` (returning `(false, Some(json!({ "error": "core and rom must not be empty" })))` if invalid), pauses frontend, spawns `/bin/sh <launcher_script>`, returns `(true, ...)` on spawn success or `(false, ...)` on spawn error.

6. **`astro/daemon/src/main.rs`** lines 808-883:
   Command polling task runs every 2s, draining GET `{boss_url}/api/v1/cabinet/{node_id}/commands`. For each command entry:
   - Parses `CabinetCommand`. On parse failure, sends failure ACK: `ack_command(..., cmd_id, false, Some(json!({ "error": reason }))).await`.
   - On parse success, calls `execute_cabinet_command` and dispatches `ack_command(..., cmd_id, success, detail).await`.

7. **`astro/daemon/src/main.rs`** lines 568-592:
   `ack_command` posts to `{boss_url}/api/v1/cabinet/{node_id}/commands/ack` with payload `{"command_id": cmd_id, "success": success, "detail": detail}`.
   Matches schema of `CommandAck` in `beercade_shared::models` lines 205-210 (`pub command_id: String, pub success: bool, pub detail: Option<serde_json::Value>`).

8. **Existing Test Coverage in `astro-daemon`**:
   `cargo test -p astro-daemon -- --list` outputs exactly 10 tests, all in `netplay.rs`. There are currently **0** unit tests in `astro/daemon/src/main.rs` for command execution, ACK dispatch, or network configuration.

---

## 2. Logic Chain

1. **Static IP Collision Chain**:
   - Observation 1 shows `Address=192.168.100.2/24` in `10-wired.network`.
   - In systemd-networkd, `Address=` assigns a static IP to the interface even when `DHCP=yes` is present.
   - Observation 2 shows `boss_static_net.sh` forcefully runs `ip addr replace 192.168.100.2/24`.
   - When multiple cabinets boot on the same physical LAN switch, all instances assign `192.168.100.2/24`, causing immediate IP address collision, duplicate IP ARP faults, and broken supervisory connections.
   - Removing `Address=` and `Gateway=` from `10-wired.network` leaves `DHCP=yes` and `LinkLocalAddressing=fallback`.
   - As demonstrated by Observation 4 (reference in `assemble_and_flash_rootfs.sh`), this configuration reliably leases dynamic IPs on DHCP networks and falls back to RFC 3927 Link-Local (`169.254.0.0/16`) on isolated switches.
   - Adjusting `boss_static_net.sh` to remove `ip addr replace 192.168.100.2/24` ensures that any execution of the script brings link UP without clobbering DHCP.

2. **Command Dispatcher Verification Chain**:
   - Observation 5 confirms `execute_cabinet_command` already implements all 6 `CabinetCommand` variants.
   - Observation 6 confirms the polling loop handles both happy path execution and parse failure fallback with ACK dispatch.
   - Observation 7 confirms the ACK structure matches `CommandAck` and BOSS's `/api/v1/cabinet/:node_id/commands/ack` endpoint.
   - Observation 8 proves that despite the complete implementation, zero tests exist for command execution or network config.
   - Therefore, Requirement R6 is fulfilled by writing comprehensive unit tests directly in `astro/daemon/src/main.rs` covering all command variants, validation logic, malformed command handling, ACK construction, mock server roundtrips, and configuration assertion tests.

---

## 3. Caveats

1. **Hardware / Display Integration**:
   - During unit testing, `killall` commands (in `pause_frontend`, `resume_frontend`, `terminate_running_game`) and `systemctl reboot` (in `Reboot`) are protected by `#[cfg(target_os = "linux")]`. In non-Linux test environments (Windows/macOS), these process calls are skipped cleanly. On Linux test environments without systemd or retroarch running, process calls fail gracefully without panic (`let _ = ...`).
2. **ROM Search Paths**:
   - `resolve_rom_path` checks `/data/beercade/roms`. In test environments where this directory is absent, it safely falls back to returning the formatted default path `/data/beercade/roms/{rom}`.
3. **No Image Builds**:
   - In strict accordance with `ORIGINAL_REQUEST.md` and `PROJECT.md`, no disk image builds (`make image-x86`, `mksquashfs`, Docker rootfs exports) are executed or required.

---

## 4. Conclusion

1. **Requirement R3 is completely resolved by**:
   - Editing `astro/rootfs/etc/systemd/network/10-wired.network` to remove lines 6-8 (`Address=192.168.100.2/24`, `Gateway=192.168.100.1`, `DNS=192.168.100.1 1.1.1.1 8.8.8.8`) while retaining `DHCP=yes` and `LinkLocalAddressing=fallback`.
   - Editing `astro/rootfs/usr/local/bin/boss_static_net.sh` to remove lines 12-13 (`ip addr replace 192.168.100.2/24 dev "$IFACE" || true` and `ip route replace default via 192.168.100.1 dev "$IFACE" || true`).
2. **Requirement R6 is completely resolved by**:
   - Importing `CommandAck` into `astro/daemon/src/main.rs`.
   - Adding unit tests in `astro/daemon/src/main.rs` (`mod command_tests`) covering:
     - `test_cmd_ping_execution`
     - `test_cmd_add_credits_execution`
     - `test_cmd_start_and_end_session`
     - `test_cmd_reboot_non_blocking`
     - `test_cmd_launch_game_validation`
     - `test_path_resolution_helpers`
     - `test_command_deserialization_and_ack_payload`
     - `test_command_ack_dispatch_mock_server`
     - `test_r3_network_config_no_static_ip`
     - `test_r3_static_net_script_no_static_ip`

---

## 5. Verification Method

1. **Config Verification Command**:
   ```powershell
   git grep "192.168.100.2" astro/rootfs/
   ```
   *Pass Condition*: Exit code 1 (no occurrences found).

2. **Compilation Command**:
   ```powershell
   cargo check -p astro-daemon
   ```
   *Pass Condition*: Exit code 0, zero errors.

3. **Workspace Test Execution**:
   ```powershell
   cargo test -p astro-daemon
   ```
   *Pass Condition*: All unit tests pass cleanly (100% pass rate).

4. **Invalidation Conditions**:
   - Any recurrence of `192.168.100.2` in `astro/rootfs/`.
   - Any test failure in `cargo test -p astro-daemon`.
   - Any dependency on real image builds or external OS packaging.
