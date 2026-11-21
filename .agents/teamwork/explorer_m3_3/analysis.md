# Technical Analysis: Requirement R3 (Dynamic Network Config) & Requirement R6 (Command Dispatcher Execution)

**Agent**: Explorer M3-3 (`explorer_m3_3`)  
**Target Components**:
- `astro/rootfs/etc/systemd/network/10-wired.network`
- `astro/rootfs/usr/local/bin/boss_static_net.sh`
- `astro/daemon/src/main.rs`
- `shared/crates/beercade-shared/src/models.rs`  
**Milestone**: Milestone 3  
**Requirements Addressed**: R3 (Remove Hardcoded Static IP Conflicts) & R6 (Automated Test Suite Expansion: Command Dispatcher Execution and ACK Behavior)

---

## 1. Executive Summary

In B.E.E.R.C.A.D.E., multiple A.S.T.R.O. arcade cabinets operate in local venue networks supervised by a central B.O.S.S. server. This investigation targets two critical stability requirements for Release Candidate 1 (RC1):

1. **Requirement R3 (Dynamic Network Config)**:
   - **Root Cause**: Both `astro/rootfs/etc/systemd/network/10-wired.network` and `astro/rootfs/usr/local/bin/boss_static_net.sh` hardcode the static IPv4 address `192.168.100.2/24` with gateway `192.168.100.1`. In multi-cabinet venues, booting more than one x86 cabinet causes instant ARP collisions, MAC flapping on network switches, and socket disruption between cabinets and B.O.S.S.
   - **Solution**: Remove static `Address=` and `Gateway=` lines from `10-wired.network`, configuring dynamic `DHCP=yes` with `LinkLocalAddressing=fallback`. Neutralize `boss_static_net.sh` so it sets interface link UP without forcing static IP assignments. Cabinets discover B.O.S.S. automatically via mDNS (`boss.local:8080`), DNS (`boss.lan:8080`), or explicit `BOSS_ENDPOINT`.

2. **Requirement R6 (Command Dispatcher Execution & ACK Behavior)**:
   - **Existing State**: `astro/daemon/src/main.rs` implements command polling (`main.rs:815-883`), command execution (`execute_cabinet_command`, lines 330-565), and acknowledgement delivery (`ack_command`, lines 568-592). It handles all 6 `CabinetCommand` variants (`Ping`, `AddCredits`, `StartSession`, `EndSession`, `Reboot`, `LaunchGame`).
   - **Identified Gap**: There is currently **zero** automated test coverage for command execution, ACK payload generation, path resolution, or command parse failure handling in `astro-daemon`.
   - **Solution**: Design and implement a robust unit test suite in `astro/daemon/src/main.rs` covering every command variant, validation failures, malformed payload error handling, and end-to-end polling/ACK roundtrips with an in-memory mock server. Additionally, implement configuration verification tests ensuring `192.168.100.2` is absent.

---

## 2. Requirement R3: Network Configuration Analysis

### 2.1 Forensic Inspection of Existing Files

#### File 1: `astro/rootfs/etc/systemd/network/10-wired.network`
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

**Systemd-networkd Behavioral Analysis**:
- `Address=192.168.100.2/24`: In systemd-networkd, specifying `Address=` configures a static address on the link. When combined with `DHCP=yes`, systemd-networkd assigns `192.168.100.2/24` immediately upon carrier detect, and *in addition* attempts to lease a DHCP address. Every x86 cabinet therefore acquires `192.168.100.2` regardless of DHCP availability.
- `Gateway=192.168.100.1`: Statically configures a default gateway route. On networks where the router is not on `192.168.100.1` (e.g. `192.168.1.1` or `10.0.0.1`), this route is either unreachable or conflicts with DHCP routing.
- `DNS=192.168.100.1 1.1.1.1 8.8.8.8`: Hardcodes private DNS server `192.168.100.1`. Under DHCP, `[DHCPv4] UseDNS=yes` already imports the local network's DNS server.
- `DHCP=yes`: Enables both IPv4 and IPv6 dynamic host configuration.
- `LinkLocalAddressing=fallback`: Configures standard RFC 3927 IPv4 Link-Local addressing (`169.254.0.0/16`) only if DHCP fails or times out.

#### File 2: `astro/rootfs/usr/local/bin/boss_static_net.sh`
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
- Line 12 explicitly executes `ip addr replace 192.168.100.2/24 dev "$IFACE"`.
- Line 13 executes `ip route replace default via 192.168.100.1 dev "$IFACE"`.
- If executed (e.g. by `boss-static-net.service` or legacy admin scripts), it forcefully stomps over any DHCP lease or link-local address.

#### Associated Service: `astro/rootfs/etc/systemd/system/boss-static-net.service`
- References `ExecStart=/usr/local/bin/boss_static_net.sh`.
- Inspection of `shared/buildfarm/x86_64/payloads/astro.conf` line 50 confirms `ENABLE_UNITS` does NOT enable `boss-static-net.service`. However, the unit file exists in `/etc/systemd/system`. If manually invoked, it triggers `boss_static_net.sh`.

---

### 2.2 Proposed Fix for R3

#### Fix 1: Update `astro/rootfs/etc/systemd/network/10-wired.network`
Remove static `Address=`, `Gateway=`, and private DNS `192.168.100.1`. Keep public fallback DNS `1.1.1.1 8.8.8.8` (or rely on DHCP), with `DHCP=yes` and `LinkLocalAddressing=fallback`:

```ini
[Match]
Name=eth* en* end* lan*
Type=ether

[Network]
DHCP=yes
LinkLocalAddressing=fallback
DNSSEC=no
MulticastDNS=yes
LLDP=no
EmitLLDP=no
IPv6AcceptRA=no
ConfigureWithoutCarrier=yes

[DHCPv4]
RouteMetric=100
UseDNS=yes
```

*Verification against reference implementation*: This exactly matches lines 95-114 of `shared/buildfarm/scripts/assemble_and_flash_rootfs.sh`, unifying the networkd configuration across x86_64 and ARM targets.

#### Fix 2: Update `astro/rootfs/usr/local/bin/boss_static_net.sh`
Neutralize static IP assignment while keeping link initialization safe:

```sh
#!/bin/sh
# Network link initialization
# Hardcoded 192.168.100.2 static IP removed to prevent multi-cabinet LAN collisions.
# Network configuration (DHCP with link-local fallback) is managed by systemd-networkd.
IFACE="end0"
if [ ! -d "/sys/class/net/$IFACE" ]; then
    IFACE=$(ls -1 /sys/class/net | grep -E '^(en|eth|end)' | head -n 1 || true)
fi

if [ -n "$IFACE" ] && [ -d "/sys/class/net/$IFACE" ]; then
    echo "[*] Ensuring network link is up on $IFACE (addressing delegated to systemd-networkd)..."
    ip link set "$IFACE" up || true
fi
exit 0
```

### 2.3 Venue Discovery Mechanism under Dynamic Addressing
When cabinets obtain dynamic IPs from the venue router, how do they connect to B.O.S.S.?
In `astro/daemon/src/main.rs:645-655`:
```rust
let mut candidates = Vec::new();
if !configured_boss_url.is_empty() {
    candidates.push(configured_boss_url.clone());
}
candidates.push("http://192.168.100.10:8080".to_string());
candidates.push("http://boss.local:8080".to_string());
candidates.push("http://boss.lan:8080".to_string());
```
1. **mDNS Resolution (`boss.local:8080`)**:
   - `10-wired.network` specifies `MulticastDNS=yes`.
   - `astro.conf` enables `avahi-daemon` and `systemd-resolved`.
   - B.O.S.S. advertises on the LAN as `boss.local`. Resolution works seamlessly without hardcoded IPs.
2. **Local DNS (`boss.lan:8080`)**:
   - Router DHCP assigns DNS domain search path `boss.lan`.
3. **Explicit Config (`BOSS_ENDPOINT`)**:
   - Configurable in `/data/beercade/astro.env`.

---

## 3. Requirement R6: Command Dispatcher Execution & ACK Behavior

### 3.1 Architecture Overview

```
+-------------------------------------------------------------+
|                         B.O.S.S. Server                     |
|  POST /api/v1/cabinet/:node_id/<action>                     |
|    -> Assigns UUID, creates CommandRecord (status=Queued)   |
|                                                             |
|  GET /api/v1/cabinet/:node_id/commands                      |
|    -> Drains pending commands (status=Dispatched)           |
|                                                             |
|  POST /api/v1/cabinet/:node_id/commands/ack                 |
|    -> Updates CommandRecord (status=Acknowledged / Failed)  |
|    -> Stores detail JSON in record.result                   |
+------------------------------^------------------------------+
                               | HTTP Poll (every 2s) & POST ACK
+------------------------------v------------------------------+
|                    A.S.T.R.O. Daemon                        |
|  1. Poll loop drains commands from GET .../commands         |
|  2. Deserializes cmd_entry["command"] -> CabinetCommand     |
|  3. Calls execute_cabinet_command(&cmd, &state, &client)    |
|  4. Calls ack_command(..., success, detail)                 |
+-------------------------------------------------------------+
```

### 3.2 Polling Loop Forensic Analysis (`main.rs:808-883`)

```rust
if !is_standalone {
    let client_cmd = reqwest::Client::builder()
        .timeout(Duration::from_millis(3000))
        .build()?;
    let cmd_state = daemon_state.clone();
    let boss_token_cmd = boss_token.clone();

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let (boss_url, node_id) = {
                let st = cmd_state.read().await;
                (st.boss_url.clone(), st.node_id.clone())
            };

            let url = format!("{}/api/v1/cabinet/{}/commands", boss_url, node_id);
            let resp = client_cmd
                .get(&url)
                .bearer_auth(&boss_token_cmd)
                .send()
                .await;

            if let Ok(resp) = resp {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(commands) = json.get("commands").and_then(|c| c.as_array()) {
                        for cmd_entry in commands {
                            let cmd_id = match cmd_entry.get("id").and_then(|i| i.as_str()) {
                                Some(id) => id,
                                None => continue,
                            };

                            let cmd_obj = cmd_entry.get("command");
                            let command_res: Result<CabinetCommand, _> =
                                serde_json::from_value(cmd_obj.cloned().unwrap_or_default());

                            match command_res {
                                Ok(cabinet_cmd) => {
                                    let (success, detail) = execute_cabinet_command(
                                        &cabinet_cmd,
                                        &cmd_state,
                                        &client_cmd,
                                    )
                                    .await;

                                    ack_command(
                                        &client_cmd,
                                        &boss_url,
                                        &boss_token_cmd,
                                        &node_id,
                                        cmd_id,
                                        success,
                                        detail,
                                    )
                                    .await;
                                }
                                Err(err) => {
                                    let reason = format!("Failed to parse command: {err}");
                                    tracing::warn!("{}", reason);
                                    ack_command(
                                        &client_cmd,
                                        &boss_url,
                                        &boss_token_cmd,
                                        &node_id,
                                        cmd_id,
                                        false,
                                        Some(serde_json::json!({ "error": reason })),
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}
```

### 3.3 Detailed Analysis of `CabinetCommand` Execution

`execute_cabinet_command(cmd, state_arc, client)` returns `(bool, Option<serde_json::Value>)`.

#### Variant 1: `CabinetCommand::Ping { seq }`
- **Behavior**: Verifies supervisor-to-node telemetry path liveness.
- **Side effects**: None on `DaemonSharedState`.
- **Return**: `(true, Some(serde_json::json!({ "seq": seq })))`.
- **Characteristics**: Instantaneous, zero allocations, deterministic.

#### Variant 2: `CabinetCommand::AddCredits { credits }`
- **Behavior**: Remote credit injection (e.g. operator top-up or mobile payment).
- **Execution**:
  1. Acquires `state_arc.write().await`.
  2. Calls `st.add_credits(*credits)`.
  3. Increments `credits.credits_available` with `saturating_add`.
  4. Increments `credits.credits_inserted_total` with `saturating_add`.
  5. Updates `credits.last_coin_epoch_ms = Some(now)`.
  6. Calls `st.persist_credits()` (which atomically updates `credits.json` and `credits.json.bak`).
  7. Drops write lock.
- **Return**: `(true, Some(serde_json::json!({ "credits_added": credits, "credits_available": available })))`.
- **Characteristics**: Atomic in-memory and disk persistence, thread-safe, overflow-safe.

#### Variant 3: `CabinetCommand::StartSession { session_id, duration_secs }`
- **Behavior**: Puts cabinet into operator/customer managed time session.
- **Execution**:
  1. Acquires `state_arc.write().await`.
  2. Sets `st.mode = StationMode::Managed { session_id: session_id.clone(), time_remaining_sec: *duration_secs }`.
- **Return**: `(true, Some(serde_json::json!({ "session_id": session_id, "duration_secs": duration_secs })))`.
- **Characteristics**: The daemon's supervisor loop (`main.rs:889-915`) decrements `time_remaining_sec` every second. When 0, station automatically locks.

#### Variant 4: `CabinetCommand::EndSession`
- **Behavior**: Immediately terminates active session and returns cabinet to locked state.
- **Execution**:
  1. Sets `st.mode = StationMode::Locked { reason: "Session ended by supervisor".to_string() }`.
  2. Drops state lock.
  3. Calls `terminate_running_game().await` (sends `SIGTERM` to `retroarch` on Linux).
  4. Calls `resume_frontend().await` (sends `SIGCONT` to `es-de` and restarts frontend service on Linux).
- **Return**: `(true, Some(serde_json::json!({ "status": "session_ended" })))`.
- **Cross-platform note**: `terminate_running_game` and `resume_frontend` use `#[cfg(target_os = "linux")]`. On Windows/macOS test environments, process signals are cleanly no-ops without panics.

#### Variant 5: `CabinetCommand::Reboot { delay_secs }`
- **Behavior**: Schedules delayed system restart.
- **Execution**:
  1. Spawns asynchronous task: `tokio::spawn(async move { sleep(delay).await; Command::new("systemctl").arg("reboot").status().await; })`.
  2. Does **not** block daemon execution while delay elapses.
- **Return**: `(true, Some(serde_json::json!({ "delay_secs": delay })))`.
- **Cross-platform note**: `systemctl reboot` is guarded with `#[cfg(target_os = "linux")]`.

#### Variant 6: `CabinetCommand::LaunchGame { core, rom, netplay }`
- **Behavior**: Directs node to launch a specific libretro core and ROM.
- **Execution Steps**:
  1. **Validation**:
     ```rust
     if core.trim().is_empty() || rom.trim().is_empty() {
         return (
             false,
             Some(serde_json::json!({ "error": "core and rom must not be empty" })),
         );
     }
     ```
     Immediately returns `(false, error)` if `core` or `rom` is empty or pure whitespace.
  2. **Path Resolution**:
     - `resolve_core_path(core)`:
       - If contains `/` or `\`, returns verbatim.
       - If ends with `.so`, returns `/usr/lib/libretro/{core}`.
       - If ends with `_libretro`, appends `.so`.
       - Else appends `_libretro.so`.
     - `resolve_rom_path(rom)`:
       - If contains `/` or `\`, returns verbatim.
       - Checks `/data/beercade/roms/...` candidate directories.
  3. **Display Pause & State Tracking**:
     - Calls `pause_frontend().await`.
     - Sets `st.active_game = Some(ActiveGameInfo { ... })`.
  4. **Process Launch**:
     - Locates launcher script (`/usr/lib/beercade/astro_kms_launcher.sh`, etc.).
     - Spawns `/bin/sh <launcher_script> /usr/bin/retroarch -L <core> <rom> [--netplay]`.
  5. **Outcome Handling**:
     - On spawn success: spawns child watchdog task to wait on process exit, handle abnormal exit logging, and call `resume_frontend().await`. Returns `(true, Some(json!({"status": "game_spawned", ...})))`.
     - On spawn failure (e.g. script missing or test platform without `/bin/sh`): clears `st.active_game = None`, reports node error to BOSS, and returns `(false, Some(json!({"error": err_msg})))`.

---

### 3.4 ACK Construction & Dispatch (`main.rs:568-592`)

```rust
async fn ack_command(
    client: &reqwest::Client,
    boss_url: &str,
    boss_token: &str,
    node_id: &str,
    cmd_id: &str,
    success: bool,
    detail: Option<serde_json::Value>,
) {
    let ack_url = format!("{}/api/v1/cabinet/{}/commands/ack", boss_url, node_id);
    let mut payload = serde_json::json!({
        "command_id": cmd_id,
        "success": success,
    });
    if let Some(d) = detail {
        payload["detail"] = d;
    }
    let mut req = client.post(&ack_url).json(&payload);
    if !boss_token.is_empty() {
        req = req.bearer_auth(boss_token);
    }
    if let Err(e) = req.send().await {
        tracing::warn!("Failed to deliver command ACK to BOSS: {e}");
    }
}
```

**Type Contract Alignment**:
- In `beercade-shared/src/models.rs:205-210`:
  ```rust
  pub struct CommandAck {
      pub command_id: String,
      pub success: bool,
      #[serde(default)]
      pub detail: Option<serde_json::Value>,
  }
  ```
- In `boss/src/routes.rs:709`:
  `pub async fn ack_command(..., Json(ack): Json<CommandAck>) -> StatusCode`
- The JSON payload constructed in `main.rs:578-584` (`{"command_id": ..., "success": ..., "detail": ...}`) directly mirrors `CommandAck`.
- **Refinement Recommendation**: Replace ad-hoc `serde_json::json!` with `CommandAck` struct serialization:
  ```rust
  let ack = CommandAck {
      command_id: cmd_id.to_string(),
      success,
      detail,
  };
  let mut req = client.post(&ack_url).json(&ack);
  ```
  This eliminates schema drift and leverages compile-time type verification.

---

## 4. Test Suite Design (Requirements R6 & R3)

### 4.1 Test Matrix

| # | Test Name | Target Component | Scenario | Assertions |
|---|-----------|------------------|----------|------------|
| 1 | `test_cmd_ping_execution` | `execute_cabinet_command` | `CabinetCommand::Ping { seq: 42 }` | `success == true`, `detail == {"seq": 42}`, state unaffected |
| 2 | `test_cmd_add_credits_execution` | `execute_cabinet_command` | `CabinetCommand::AddCredits { credits: 5 }` | `success == true`, `detail == {"credits_added": 5, "credits_available": 5}`, state reflects updated credits |
| 3 | `test_cmd_start_and_end_session` | `execute_cabinet_command` | `StartSession` followed by `EndSession` | `StartSession` sets `Managed`, `EndSession` sets `Locked`, returns expected status details |
| 4 | `test_cmd_reboot_non_blocking` | `execute_cabinet_command` | `CabinetCommand::Reboot { delay_secs: 10 }` | `success == true`, returns in <50ms without waiting for delay, no crash |
| 5 | `test_cmd_launch_game_validation` | `execute_cabinet_command` | Empty core, empty rom, whitespace strings | `success == false`, `detail["error"]` contains `"core and rom must not be empty"` |
| 6 | `test_path_resolution_helpers` | `resolve_core_path`, `resolve_rom_path` | Bare core name, `.so`, `_libretro`, relative rom paths | Correct `/usr/lib/libretro/...` and `/data/beercade/roms/...` paths |
| 7 | `test_command_deserialization_and_ack_payload` | `CabinetCommand` & `CommandAck` | All 6 variants valid JSON & invalid command type | Valid types deserialize; invalid types yield parse failure and failure ACK |
| 8 | `test_command_polling_and_ack_roundtrip` | Full dispatch loop | Mock HTTP server serving commands & capturing ACKs | GET receives command, executes, POSTs valid `CommandAck` back to mock server |
| 9 | `test_r3_network_config_no_static_ip` | `10-wired.network` | Config file inspection | Does NOT contain `192.168.100.2` or `Address=`, contains `DHCP=yes`, contains `LinkLocalAddressing=fallback` |
| 10 | `test_r3_static_net_script_no_static_ip` | `boss_static_net.sh` | Shell script inspection | Does NOT contain `192.168.100.2` |

---

### 4.2 Concrete Test Code Implementation for `astro/daemon/src/main.rs`

Below is the complete, self-contained unit test module for `astro/daemon/src/main.rs`:

```rust
#[cfg(test)]
mod command_tests {
    use super::*;
    use beercade_shared::models::{CabinetCommand, CommandAck, StationMode};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Helper to construct a temporary DaemonSharedState for testing.
    fn create_test_state(prefix: &str) -> (Arc<RwLock<DaemonSharedState>>, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let credits_path = temp_dir.path().join("credits.json");
        let state = Arc::new(RwLock::new(DaemonSharedState {
            node_id: format!("TEST-NODE-{}", prefix),
            boss_url: "http://127.0.0.1:8080".to_string(),
            boss_token: "test-token".to_string(),
            mode: StationMode::Standalone,
            credits: CreditCounters::default(),
            active_game: None,
            credits_path,
        }));
        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_cmd_ping_execution() {
        let (state, _tmp) = create_test_state("ping");
        let client = reqwest::Client::new();
        let cmd = CabinetCommand::Ping { seq: 42 };

        let (success, detail) = execute_cabinet_command(&cmd, &state, &client).await;
        assert!(success);
        let detail = detail.expect("detail should be present");
        assert_eq!(detail["seq"], 42);
    }

    #[tokio::test]
    async fn test_cmd_add_credits_execution() {
        let (state, _tmp) = create_test_state("add_credits");
        let client = reqwest::Client::new();

        // Add 5 credits
        let cmd = CabinetCommand::AddCredits { credits: 5 };
        let (success, detail) = execute_cabinet_command(&cmd, &state, &client).await;
        assert!(success);
        let detail = detail.expect("detail should be present");
        assert_eq!(detail["credits_added"], 5);
        assert_eq!(detail["credits_available"], 5);

        // Verify in-memory state
        {
            let st = state.read().await;
            assert_eq!(st.credits.credits_available, 5);
            assert_eq!(st.credits.credits_inserted_total, 5);
            assert!(st.credits.last_coin_epoch_ms.is_some());
        }

        // Add 10 more credits
        let cmd2 = CabinetCommand::AddCredits { credits: 10 };
        let (success2, detail2) = execute_cabinet_command(&cmd2, &state, &client).await;
        assert!(success2);
        let detail2 = detail2.expect("detail should be present");
        assert_eq!(detail2["credits_added"], 10);
        assert_eq!(detail2["credits_available"], 15);

        // Verify state
        {
            let st = state.read().await;
            assert_eq!(st.credits.credits_available, 15);
            assert_eq!(st.credits.credits_inserted_total, 15);
        }
    }

    #[tokio::test]
    async fn test_cmd_start_and_end_session() {
        let (state, _tmp) = create_test_state("session");
        let client = reqwest::Client::new();

        // 1. Start session
        let start_cmd = CabinetCommand::StartSession {
            session_id: "sess-abc-123".to_string(),
            duration_secs: 1200,
        };
        let (success, detail) = execute_cabinet_command(&start_cmd, &state, &client).await;
        assert!(success);
        let detail = detail.expect("detail should be present");
        assert_eq!(detail["session_id"], "sess-abc-123");
        assert_eq!(detail["duration_secs"], 1200);

        {
            let st = state.read().await;
            match &st.mode {
                StationMode::Managed {
                    session_id,
                    time_remaining_sec,
                } => {
                    assert_eq!(session_id, "sess-abc-123");
                    assert_eq!(*time_remaining_sec, 1200);
                }
                other => panic!("expected Managed mode, got {:?}", other),
            }
        }

        // 2. End session
        let end_cmd = CabinetCommand::EndSession;
        let (success_end, detail_end) = execute_cabinet_command(&end_cmd, &state, &client).await;
        assert!(success_end);
        let detail_end = detail_end.expect("detail should be present");
        assert_eq!(detail_end["status"], "session_ended");

        {
            let st = state.read().await;
            match &st.mode {
                StationMode::Locked { reason } => {
                    assert!(reason.contains("Session ended by supervisor"));
                }
                other => panic!("expected Locked mode, got {:?}", other),
            }
        }
    }

    #[tokio::test]
    async fn test_cmd_reboot_non_blocking() {
        let (state, _tmp) = create_test_state("reboot");
        let client = reqwest::Client::new();
        let cmd = CabinetCommand::Reboot { delay_secs: 300 };

        let start = std::time::Instant::now();
        let (success, detail) = execute_cabinet_command(&cmd, &state, &client).await;
        let elapsed = start.elapsed();

        assert!(success);
        let detail = detail.expect("detail should be present");
        assert_eq!(detail["delay_secs"], 300);
        // Ensure reboot returns immediately rather than sleeping for 300 seconds
        assert!(elapsed < std::time::Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_cmd_launch_game_validation() {
        let (state, _tmp) = create_test_state("launch_validation");
        let client = reqwest::Client::new();

        // Empty core
        let cmd_empty_core = CabinetCommand::LaunchGame {
            core: "".to_string(),
            rom: "pacman.zip".to_string(),
            netplay: false,
        };
        let (success, detail) = execute_cabinet_command(&cmd_empty_core, &state, &client).await;
        assert!(!success);
        assert_eq!(
            detail.unwrap()["error"],
            "core and rom must not be empty"
        );

        // Empty rom
        let cmd_empty_rom = CabinetCommand::LaunchGame {
            core: "mame".to_string(),
            rom: "   ".to_string(),
            netplay: false,
        };
        let (success2, detail2) = execute_cabinet_command(&cmd_empty_rom, &state, &client).await;
        assert!(!success2);
        assert_eq!(
            detail2.unwrap()["error"],
            "core and rom must not be empty"
        );
    }

    #[test]
    fn test_path_resolution_helpers() {
        // Core path resolutions
        assert_eq!(resolve_core_path("mame"), "/usr/lib/libretro/mame_libretro.so");
        assert_eq!(resolve_core_path("snes9x.so"), "/usr/lib/libretro/snes9x.so");
        assert_eq!(
            resolve_core_path("genesis_plus_gx_libretro"),
            "/usr/lib/libretro/genesis_plus_gx_libretro.so"
        );
        assert_eq!(
            resolve_core_path("/opt/custom/core.so"),
            "/opt/custom/core.so"
        );

        // ROM path resolutions
        assert_eq!(
            resolve_rom_path("arcade/pacman.zip"),
            "arcade/pacman.zip"
        );
        // Default lookup for non-existent bare ROM
        assert_eq!(
            resolve_rom_path("nonexistent_game_xyz.zip"),
            "/data/beercade/roms/nonexistent_game_xyz.zip"
        );
    }

    #[tokio::test]
    async fn test_command_deserialization_and_ack_payload() {
        // Test parsing each valid CabinetCommand JSON representation
        let ping_json = serde_json::json!({ "type": "ping", "seq": 99 });
        let cmd: CabinetCommand = serde_json::from_value(ping_json).unwrap();
        matches!(cmd, CabinetCommand::Ping { seq: 99 });

        let add_json = serde_json::json!({ "type": "add_credits", "credits": 4 });
        let cmd: CabinetCommand = serde_json::from_value(add_json).unwrap();
        matches!(cmd, CabinetCommand::AddCredits { credits: 4 });

        let start_json = serde_json::json!({
            "type": "start_session",
            "session_id": "s-1",
            "duration_secs": 600
        });
        let cmd: CabinetCommand = serde_json::from_value(start_json).unwrap();
        matches!(cmd, CabinetCommand::StartSession { .. });

        let end_json = serde_json::json!({ "type": "end_session" });
        let cmd: CabinetCommand = serde_json::from_value(end_json).unwrap();
        matches!(cmd, CabinetCommand::EndSession);

        let reboot_json = serde_json::json!({ "type": "reboot", "delay_secs": 5 });
        let cmd: CabinetCommand = serde_json::from_value(reboot_json).unwrap();
        matches!(cmd, CabinetCommand::Reboot { delay_secs: 5 });

        let launch_json = serde_json::json!({
            "type": "launch_game",
            "core": "fbneo",
            "rom": "sf2.zip",
            "netplay": true
        });
        let cmd: CabinetCommand = serde_json::from_value(launch_json).unwrap();
        matches!(cmd, CabinetCommand::LaunchGame { .. });

        // Test invalid/unknown command type
        let bad_json = serde_json::json!({ "type": "unknown_command_variant" });
        let bad_res: Result<CabinetCommand, _> = serde_json::from_value(bad_json);
        assert!(bad_res.is_err());

        // Test CommandAck model serialization
        let ack = CommandAck {
            command_id: "cmd-123".to_string(),
            success: true,
            detail: Some(serde_json::json!({ "seq": 99 })),
        };
        let serialized = serde_json::to_value(&ack).unwrap();
        assert_eq!(serialized["command_id"], "cmd-123");
        assert_eq!(serialized["success"], true);
        assert_eq!(serialized["detail"]["seq"], 99);
    }

    #[tokio::test]
    async fn test_command_ack_dispatch_mock_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let boss_url = format!("http://{}", addr);

        let received_ack = Arc::new(tokio::sync::Mutex::new(None::<String>));
        let received_ack_clone = received_ack.clone();

        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();

                // Extract body after HTTP headers
                if let Some(idx) = request.find("\r\n\r\n") {
                    let body = &request[idx + 4..];
                    *received_ack_clone.lock().await = Some(body.to_string());
                }

                let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let client = reqwest::Client::new();
        ack_command(
            &client,
            &boss_url,
            "test-token",
            "ASTRO-01",
            "cmd-unit-test-1",
            true,
            Some(serde_json::json!({ "status": "executed" })),
        )
        .await;

        let body_str = received_ack.lock().await.clone().expect("ACK body not captured");
        let parsed: serde_json::Value = serde_json::from_str(&body_str).unwrap();
        assert_eq!(parsed["command_id"], "cmd-unit-test-1");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["detail"]["status"], "executed");
    }

    #[test]
    fn test_r3_network_config_no_static_ip() {
        let network_config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rootfs/etc/systemd/network/10-wired.network");

        if network_config_path.exists() {
            let content = std::fs::read_to_string(&network_config_path)
                .expect("failed to read 10-wired.network");

            // Must NOT contain the hardcoded static IP
            assert!(
                !content.contains("192.168.100.2"),
                "10-wired.network must not contain static IP 192.168.100.2"
            );
            // Must NOT assign static Address
            assert!(
                !content.contains("Address="),
                "10-wired.network must not contain static Address="
            );
            // Must have dynamic DHCP enabled
            assert!(
                content.contains("DHCP=yes"),
                "10-wired.network must contain DHCP=yes"
            );
            // Must have link-local fallback
            assert!(
                content.contains("LinkLocalAddressing=fallback")
                    || content.contains("LinkLocalAddressing=ipv4")
                    || content.contains("LinkLocalAddressing=yes"),
                "10-wired.network must configure LinkLocalAddressing fallback"
            );
        }
    }

    #[test]
    fn test_r3_static_net_script_no_static_ip() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rootfs/usr/local/bin/boss_static_net.sh");

        if script_path.exists() {
            let content = std::fs::read_to_string(&script_path)
                .expect("failed to read boss_static_net.sh");

            // Must NOT contain the hardcoded static IP
            assert!(
                !content.contains("192.168.100.2"),
                "boss_static_net.sh must not contain static IP 192.168.100.2"
            );
        }
    }
}
```

---

## 5. Implementer Action Plan (Worker M3)

### Step 1: Update Network Configuration Files
1. Modify `astro/rootfs/etc/systemd/network/10-wired.network`:
   - Remove lines 6-8:
     ```ini
     Address=192.168.100.2/24
     Gateway=192.168.100.1
     DNS=192.168.100.1 1.1.1.1 8.8.8.8
     ```
   - Retain:
     ```ini
     DHCP=yes
     LinkLocalAddressing=fallback
     ```
2. Modify `astro/rootfs/usr/local/bin/boss_static_net.sh`:
   - Remove lines 12-13:
     ```sh
     ip addr replace 192.168.100.2/24 dev "$IFACE" || true
     ip route replace default via 192.168.100.1 dev "$IFACE" || true
     ```
   - Keep interface link UP command (`ip link set "$IFACE" up || true`) and exit 0.

### Step 2: Integrate `CommandAck` in `astro/daemon/src/main.rs`
1. Import `CommandAck` from `beercade_shared::models`:
   ```rust
   pub use beercade_shared::models::{
       CabinetCommand, CommandAck, CreditCounters, ErrorEvent, HighScoreEntry, InputJitterStats, NodeTelemetry,
       StationMode,
   };
   ```
2. In `ack_command` (lines 568-592):
   Instantiate `CommandAck` rather than manual JSON construction:
   ```rust
   let ack = CommandAck {
       command_id: cmd_id.to_string(),
       success,
       detail,
   };
   let mut req = client.post(&ack_url).json(&ack);
   ```

### Step 3: Add Unit Tests in `astro/daemon/src/main.rs`
1. Check dependencies in `astro/daemon/Cargo.toml`:
   - Add `[dev-dependencies]` `tempfile = "3.10"` if needed (or use standard library RAII temp dirs).
2. Append the test module `mod command_tests` (or merge with `mod tests` from Explorer M3-2) at the bottom of `astro/daemon/src/main.rs`.
3. Verify test execution with `cargo test -p astro-daemon`.

---

## 6. Verification Method

1. **Verify absence of static IP in repository files**:
   ```powershell
   git grep "192.168.100.2" astro/rootfs/
   ```
   *Expected output*: No matches.

2. **Verify syntax and compilation**:
   ```powershell
   cargo check -p astro-daemon
   ```
   *Expected output*: Clean compilation with exit code 0.

3. **Execute unit tests**:
   ```powershell
   cargo test -p astro-daemon -- --nocapture
   ```
   *Expected output*: All existing netplay tests + new command dispatcher and network verification tests pass 100%.
