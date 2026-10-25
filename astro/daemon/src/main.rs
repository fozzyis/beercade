use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

mod netplay;
mod ota;

use beercade_shared::auth::format_bearer_header;
use beercade_shared::ipc::{format_http_telemetry_url, format_ws_telemetry_url};
pub use beercade_shared::models::{NodeTelemetry, StationMode};

struct NodeState {
    node_id: String,
    boss_url: String,
    mode: StationMode,
}

fn sample_telemetry(state: &NodeState) -> NodeTelemetry {
    let (cpu_temp, gpu_temp) = netplay::sample_temps();
    let (hardware, soc) = beercade_hal::Platform::detect().hardware_info();

    NodeTelemetry {
        node_id: state.node_id.clone(),
        hardware: hardware.to_string(),
        soc: soc.to_string(),
        cpu_temp_c: cpu_temp,
        gpu_temp_c: gpu_temp,
        npu_load_pct: 0.0,
        active_core: "FBNeo".to_string(),
        active_rom: "sf2ce".to_string(),
        frame_time_ms: 16.63,
        fps: 60.1,
        input_jitter: Default::default(),
        netplay: None,
        credits: Default::default(),
        mode: serde_json::to_value(&state.mode).unwrap_or_default(),
        timestamp_epoch_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    println!("===============================================================");
    println!("   A.S.T.R.O. Node Daemon v2.0 - RK3588S O.R.I.O.N. Station   ");
    println!("===============================================================");

    let is_standalone = std::env::var("ASTRO_STANDALONE").unwrap_or_default() == "1"
        || std::env::var("BOSS_ENDPOINT").unwrap_or_default() == "none";

    let configured_boss_url = std::env::var("BOSS_ENDPOINT").unwrap_or_default();
    let initial_boss_url = if configured_boss_url.is_empty() {
        "http://192.168.100.10:8080".to_string()
    } else {
        configured_boss_url.clone()
    };

    let node_state = Arc::new(RwLock::new(NodeState {
        node_id: std::env::var("ASTRO_NODE_ID").unwrap_or_else(|_| "ASTRO-01".to_string()),
        boss_url: initial_boss_url,
        mode: StationMode::Standalone,
    }));

    if is_standalone {
        println!("[✓] A.S.T.R.O. Daemon running in Standalone Mode (BOSS hub connection disabled).");
    } else {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()?;

        // The node authenticates to BOSS with BOSS_RPC_TOKEN.  Both the
        // WebSocket upgrade request and the HTTP fallback POST carry this as
        // `Authorization: Bearer <token>` so the server can accept them.
        let boss_token = std::env::var("BOSS_RPC_TOKEN").unwrap_or_default();

        let telemetry_state = node_state.clone();
        tokio::spawn(async move {
            let mut ws_failures = 0;
            let max_failures_before_fallback = 3;

            // Discovery candidate list: custom endpoint, static VLAN IP, mDNS, venue LAN DNS
            let mut candidates = Vec::new();
            if !configured_boss_url.is_empty() {
                candidates.push(configured_boss_url.clone());
            }
            candidates.push("http://192.168.100.10:8080".to_string());
            candidates.push("http://boss.local:8080".to_string());
            candidates.push("http://boss.lan:8080".to_string());

            let mut candidate_idx = 0;
        
            loop {
                let boss_url = {
                    let state = telemetry_state.read().await;
                    state.boss_url.clone()
                };
            
                let ws_url = format_ws_telemetry_url(&boss_url);

                // Build the upgrade request, attaching the bearer token so
                // BOSS's RequireNodeToken extractor accepts the connection.
                let mut ws_req = ws_url
                    .as_str()
                    .into_client_request()
                    .expect("valid WS URL");
                if !boss_token.is_empty() {
                    ws_req.headers_mut().insert(
                        "Authorization",
                        format_bearer_header(&boss_token)
                            .parse()
                            .expect("valid header value"),
                    );
                }

                match connect_async(ws_req).await {
                    Ok((mut ws_stream, _)) => {
                        println!("[✓] WebSocket telemetry connected to {}", ws_url);
                        ws_failures = 0; // Reset failures on successful connect
                        
                        loop {
                            let telemetry = {
                                let state = telemetry_state.read().await;
                                sample_telemetry(&state)
                            };
                            
                            let msg = serde_json::to_string(&telemetry).unwrap();
                            if let Err(e) = ws_stream.send(Message::Text(msg)).await {
                                println!("[!] WebSocket telemetry send failed: {}", e);
                                break;
                            }

                            // Wait before next heartbeat, also check if B.O.S.S. sent us anything
                            tokio::select! {
                                _ = sleep(Duration::from_millis(1000)) => {}
                                msg = ws_stream.next() => {
                                    match msg {
                                        Some(Ok(Message::Text(text))) => {
                                            if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text) {
                                                if cmd.get("kind").and_then(|v| v.as_str()) == Some("payload_available") {
                                                    if let Some(filename) = cmd.get("filename").and_then(|v| v.as_str()) {
                                                        println!("[i] Received OTA payload notification: {}", filename);
                                                        let boss = boss_url.replace("ws://", "http://").replace("wss://", "https://");
                                                        let filename_owned = filename.to_string();
                                                        let token_for_ota = boss_token.clone();
                                                        tokio::spawn(async move {
                                                            if let Err(e) = ota::execute_ota_update(&boss, &filename_owned, &token_for_ota).await {
                                                                println!("[!] OTA Update failed: {}", e);
                                                            }
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                        Some(Ok(_)) => {}
                                        _ => break, // Connection closed or error
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        ws_failures += 1;
                        println!("[!] WebSocket connect failed (attempt {}): {}", ws_failures, e);
                        
                        if ws_failures >= max_failures_before_fallback {
                            println!("[!] Falling back to HTTP telemetry...");
                            let http_url = format_http_telemetry_url(&boss_url);
                            let telemetry = {
                                let state = telemetry_state.read().await;
                                sample_telemetry(&state)
                            };
                            let http_res = client
                                .post(&http_url)
                                .bearer_auth(&boss_token)
                                .json(&telemetry)
                                .send()
                                .await;
                            
                            if http_res.is_err() {
                                // Rotate candidate endpoint to find BOSS dynamically
                                candidate_idx = (candidate_idx + 1) % candidates.len();
                                let next_url = candidates[candidate_idx].clone();
                                println!("[i] Probing next candidate BOSS endpoint: {}", next_url);
                                let mut st = telemetry_state.write().await;
                                st.boss_url = next_url;
                            }

                            // Retry after 3s
                            sleep(Duration::from_secs(3)).await;
                        } else {
                            // Exponential backoff up to 3 seconds
                            let backoff = (1 << ws_failures).min(3);
                            sleep(Duration::from_secs(backoff)).await;
                        }
                    }
                }
            }
        });
        println!("[✓] Telemetry loop initialized. Listening for supervisor directives...");
    }

    // -------------------------------------------------------------------
    // Rollback netplay session (P2P UDP to the peer O.R.I.O.N. cabinet).
    // Enabled by env: ASTRO_NETPLAY_PEER="192.168.x.y:9750"
    // Optional: ASTRO_NETPLAY_SESSION (u64), ASTRO_NETPLAY_HOST=1,
    //           ASTRO_NETPLAY_PLAYER=0|1, ASTRO_NETPLAY_PORT (bind port)
    // The emulator core bridge (FBNeo/libretro) must implement
    // netplay::DeterministicCore; SyntheticCore is the reference stub.
    // -------------------------------------------------------------------
    if let Ok(peer) = std::env::var("ASTRO_NETPLAY_PEER") {
        let cfg = netplay::NetplayConfig {
            session_id: std::env::var("ASTRO_NETPLAY_SESSION")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            bind_port: std::env::var("ASTRO_NETPLAY_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(9750),
            peer_addr: peer.parse().unwrap_or_else(|_| {
                panic!("ASTRO_NETPLAY_PEER must be a valid socket address, got {peer}")
            }),
            local_player: std::env::var("ASTRO_NETPLAY_PLAYER")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(0),
            is_host: std::env::var("ASTRO_NETPLAY_HOST").map(|v| v == "1").unwrap_or(false),
            node_id: std::env::var("ASTRO_NODE_ID").unwrap_or_else(|_| "ORION-01".to_string()),
            ..Default::default()
        };
        let boss = Some(node_state.read().await.boss_url.clone());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            // TODO(astro-core): replace SyntheticCore with the FBNeo
            // DeterministicCore bridge when the libretro hookup lands.
            let outcome = netplay::run_station(
                cfg,
                netplay::SyntheticCore::default(),
                Box::new(|_frame| 0), // joypad GPIO poll goes here
                shutdown_rx,
                boss,
            )
            .await;
            match outcome {
                Ok(o) => println!("[i] netplay session ended: {}", o.end_reason),
                Err(e) => println!("[!] netplay session failed: {e}"),
            }
        });
        println!("[✓] Rollback netplay armed against peer {peer}");
        // Graceful shutdown hook: SIGTERM drops the session into local mode.
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = shutdown_tx.send(true);
        });
    }

    // -------------------------------------------------------------------
    // Scaffold: Command Polling Loop for Game Delivery (BOSS -> Astro)
    // -------------------------------------------------------------------
    if !is_standalone {
        let client_clone = reqwest::Client::new();
        let cmd_state = node_state.clone();
        let boss_token = std::env::var("BOSS_RPC_TOKEN").unwrap_or_default();
        
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(2)).await;
                let (boss_url, node_id) = {
                    let st = cmd_state.read().await;
                    (st.boss_url.clone(), st.node_id.clone())
                };
                let url = format!("{}/api/v1/cabinet/{}/commands", boss_url, node_id);
                if let Ok(resp) = client_clone.get(&url).bearer_auth(&boss_token).send().await {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        if let Some(commands) = json.get("commands").and_then(|c| c.as_array()) {
                            for cmd in commands {
                                println!("[i] Received command: {:?}", cmd);
                                if let Some(cmd_id) = cmd.get("id").and_then(|i| i.as_str()) {
                                    let c_type = cmd.get("command").and_then(|c| c.get("type")).and_then(|t| t.as_str());
                                    let success = true;
                                    if c_type == Some("launch_game") {
                                        let core = cmd.get("command").unwrap().get("core").and_then(|v| v.as_str()).unwrap_or("");
                                        let rom = cmd.get("command").unwrap().get("rom").and_then(|v| v.as_str()).unwrap_or("");
                                        println!("[+] Launching game: core={} rom={}", core, rom);
                                        // Scaffold: In reality, launch RetroArch with specific core and ROM
                                        let _ = std::process::Command::new("/data/dev/beercade/emu_fe/boot_esde.sh").spawn();
                                    } else if c_type == Some("add_credits") {
                                        println!("[+] Added credits!");
                                    }
                                    
                                    // ACK
                                    let ack_url = format!("{}/api/v1/cabinet/{}/commands/ack", boss_url, node_id);
                                    let _ = client_clone.post(&ack_url)
                                        .bearer_auth(&boss_token)
                                        .json(&serde_json::json!({
                                            "command_id": cmd_id,
                                            "success": success
                                        })).send().await;
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    // Main supervisor watchdog loop
    loop {
        sleep(Duration::from_secs(1)).await;
        let mut state = node_state.write().await;
        if let StationMode::Managed { ref mut time_remaining_sec, .. } = state.mode {
            if *time_remaining_sec > 0 {
                *time_remaining_sec -= 1;
            } else {
                println!("[!] Session expired. Transitioning to Locked mode.");
                state.mode = StationMode::Locked {
                    reason: "Session time limit reached. Insert token to resume.".to_string(),
                };
            }
        }
    }
}
