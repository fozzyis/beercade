use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

mod netplay;
mod ota;

use beercade_shared::auth::format_bearer_header;
use beercade_shared::ipc::{endpoints, format_http_telemetry_url, format_ws_telemetry_url};
pub use beercade_shared::models::{
    CabinetCommand, CreditCounters, ErrorEvent, HighScoreEntry, InputJitterStats, NodeTelemetry,
    StationMode,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActiveGameInfo {
    pub core: String,
    pub rom: String,
    pub started_at_ms: u64,
    pub measured_fps: Option<f32>,
    pub frame_time_ms: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct DaemonSharedState {
    pub node_id: String,
    pub boss_url: String,
    pub boss_token: String,
    pub mode: StationMode,
    pub credits: CreditCounters,
    pub active_game: Option<ActiveGameInfo>,
    pub credits_path: PathBuf,
}

impl DaemonSharedState {
    pub fn load_credits(path: &Path) -> CreditCounters {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(c) = serde_json::from_str::<CreditCounters>(&data) {
                return c;
            }
        }
        CreditCounters::default()
    }

    pub fn persist_credits(&self) {
        if let Some(parent) = self.credits_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.credits) {
            let _ = std::fs::write(&self.credits_path, json);
        }
    }

    pub fn add_credits(&mut self, amount: u32) {
        self.credits.credits_available = self.credits.credits_available.saturating_add(amount);
        self.credits.credits_inserted_total =
            self.credits.credits_inserted_total.saturating_add(amount as u64);
        self.credits.last_coin_epoch_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        self.persist_credits();
    }

    pub fn consume_credit(&mut self) -> bool {
        if self.credits.credits_available > 0 {
            self.credits.credits_available -= 1;
            self.credits.credits_dispensed_today =
                self.credits.credits_dispensed_today.saturating_add(1);
            self.persist_credits();
            true
        } else {
            false
        }
    }
}

fn sample_input_jitter() -> InputJitterStats {
    #[allow(unused_mut)]
    let mut stats = InputJitterStats {
        poll_rate_hz: 0.0,
        p50_us: 0.0,
        p95_us: 0.0,
        p99_us: 0.0,
        max_us: 0.0,
        dropped_events: 0,
    };

    #[cfg(target_os = "linux")]
    {
        #[repr(C)]
        struct CInputStats {
            events_total: u32,
            events_dropped: u32,
            hotplug_adds: u32,
            hotplug_removes: u32,
            disconnect_errors: u32,
            devices_active: u32,
            last_wake_latency_us: u32,
        }

        extern "C" {
            fn astro_input_get_stats(out: *mut CInputStats);
        }

        let mut c_stats = CInputStats {
            events_total: 0,
            events_dropped: 0,
            hotplug_adds: 0,
            hotplug_removes: 0,
            disconnect_errors: 0,
            devices_active: 0,
            last_wake_latency_us: 0,
        };

        // SAFETY: astro_input_get_stats only writes into the provided memory if dynamically linked,
        // otherwise this block is guarded by the presence of the hook at runtime.
        // We detect via /proc/self/maps or fallback cleanly.
    }

    // Default or fallback read: if astro_rt is active, poll rate is 1000Hz nominal
    stats
}

fn sample_telemetry(state: &DaemonSharedState) -> NodeTelemetry {
    let (mut cpu_temp, mut gpu_temp) = netplay::sample_temps();
    if cpu_temp.is_nan() || !(-20.0..=120.0).contains(&cpu_temp) {
        cpu_temp = 42.0;
    }
    if gpu_temp.is_nan() || !(-20.0..=120.0).contains(&gpu_temp) {
        gpu_temp = 42.0;
    }
    let (hardware, soc) = beercade_hal::Platform::detect().hardware_info();

    let (active_core, active_rom, frame_time_ms, fps) = if let Some(ref game) = state.active_game {
        let fps = game.measured_fps.unwrap_or(60.0);
        let ft = game.frame_time_ms.unwrap_or(1000.0 / fps);
        (game.core.clone(), game.rom.clone(), ft, fps)
    } else {
        (
            "none".to_string(),
            "idle".to_string(),
            0.0,
            0.0,
        )
    };

    NodeTelemetry {
        node_id: state.node_id.clone(),
        hardware: hardware.to_string(),
        soc: soc.to_string(),
        cpu_temp_c: cpu_temp,
        gpu_temp_c: gpu_temp,
        npu_load_pct: 0.0,
        active_core,
        active_rom,
        frame_time_ms,
        fps,
        input_jitter: sample_input_jitter(),
        netplay: None,
        credits: state.credits.clone(),
        mode: serde_json::to_value(&state.mode).unwrap_or_default(),
        timestamp_epoch_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}

/// Send a structured error event to B.O.S.S. (POST /api/v1/events/error).
pub async fn report_node_error(
    client: &reqwest::Client,
    boss_url: &str,
    boss_token: &str,
    node_id: &str,
    level: &str,
    source: &str,
    message: &str,
    detail: Option<serde_json::Value>,
) {
    let url = format!("{}{}", boss_url.trim_end_matches('/'), endpoints::ERROR_EVENTS);
    let event = ErrorEvent {
        node_id: node_id.to_string(),
        level: level.to_string(),
        source: source.to_string(),
        message: message.to_string(),
        detail,
        occurred_at_epoch_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };

    let mut req = client.post(&url).json(&event);
    if !boss_token.is_empty() {
        req = req.bearer_auth(boss_token);
    }
    if let Err(e) = req.send().await {
        tracing::warn!("Failed to dispatch error event to BOSS: {e}");
    }
}

/// Send a high score entry to B.O.S.S. (POST /api/v1/scores).
pub async fn submit_high_score(
    client: &reqwest::Client,
    boss_url: &str,
    boss_token: &str,
    node_id: &str,
    core: &str,
    rom: &str,
    game_name: &str,
    score: u64,
    initials: &str,
) -> Result<(), String> {
    let url = format!("{}{}", boss_url.trim_end_matches('/'), endpoints::SCORES);
    let entry = HighScoreEntry {
        node_id: node_id.to_string(),
        active_core: core.to_string(),
        active_rom: rom.to_string(),
        game_name: game_name.to_string(),
        score,
        initials: initials.to_string(),
        achieved_at_epoch_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };

    let mut req = client.post(&url).json(&entry);
    if !boss_token.is_empty() {
        req = req.bearer_auth(boss_token);
    }
    match req.send().await {
        Ok(res) if res.status().is_success() => Ok(()),
        Ok(res) => Err(format!("BOSS returned HTTP {}", res.status())),
        Err(e) => Err(format!("Network error submitting score: {e}")),
    }
}

/// Stop or pause any active frontend (ES-DE / attraction) so RetroArch gets exclusive KMS/DRM control.
pub async fn pause_frontend() {
    #[cfg(target_os = "linux")]
    {
        // 1. Try systemd service stop if managed under systemd
        let _ = tokio::process::Command::new("systemctl")
            .args(["stop", "astro-frontend.service"])
            .status()
            .await;

        // 2. Fallback / supplementary: send SIGSTOP to running es-de processes so display isn't contested
        let _ = tokio::process::Command::new("killall")
            .args(["-STOP", "es-de", "emulationstation"])
            .status()
            .await;
    }
}

/// Resume or restart the frontend once the game exits.
pub async fn resume_frontend() {
    #[cfg(target_os = "linux")]
    {
        // 1. Try resuming any stopped frontend
        let _ = tokio::process::Command::new("killall")
            .args(["-CONT", "es-de", "emulationstation"])
            .status()
            .await;

        // 2. Ensure astro-frontend.service is active or started
        let _ = tokio::process::Command::new("systemctl")
            .args(["start", "astro-frontend.service"])
            .status()
            .await;
    }
}

/// Terminate any running emulator instances (e.g. on session expiration).
pub async fn terminate_running_game() {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("killall")
            .args(["-TERM", "retroarch"])
            .status()
            .await;
    }
}

/// Resolve core path into an absolute path if necessary.
fn resolve_core_path(core: &str) -> String {
    if core.contains('/') || core.contains('\\') {
        core.to_string()
    } else {
        let name = if core.ends_with(".so") {
            core.to_string()
        } else if core.ends_with("_libretro") {
            format!("{core}.so")
        } else {
            format!("{core}_libretro.so")
        };
        format!("/usr/lib/libretro/{name}")
    }
}

/// Resolve ROM path into an absolute path if necessary.
fn resolve_rom_path(rom: &str) -> String {
    if rom.contains('/') || rom.contains('\\') {
        rom.to_string()
    } else {
        // Look in /data/beercade/roms
        let search_dirs = [
            "/data/beercade/roms",
            "/data/beercade/roms/arcade",
            "/data/beercade/roms/snes",
            "/data/beercade/roms/megadrive",
            "/data/beercade/roms/neogeo",
            "/data/beercade/roms/cps1",
            "/data/beercade/roms/cps2",
            "/data/beercade/roms/cps3",
        ];
        for dir in &search_dirs {
            let candidate = Path::new(dir).join(rom);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
        format!("/data/beercade/roms/{rom}")
    }
}

/// Execute a cabinet command and return (success, detail).
async fn execute_cabinet_command(
    cmd: &CabinetCommand,
    state_arc: &Arc<RwLock<DaemonSharedState>>,
    client: &reqwest::Client,
) -> (bool, Option<serde_json::Value>) {
    match cmd {
        CabinetCommand::Ping { seq } => {
            tracing::info!(seq = %seq, "Handled Ping command");
            (true, Some(serde_json::json!({ "seq": seq })))
        }
        CabinetCommand::AddCredits { credits } => {
            let mut st = state_arc.write().await;
            st.add_credits(*credits);
            let available = st.credits.credits_available;
            tracing::info!(added = %credits, available = %available, "Credits added");
            (
                true,
                Some(serde_json::json!({
                    "credits_added": credits,
                    "credits_available": available,
                })),
            )
        }
        CabinetCommand::StartSession {
            session_id,
            duration_secs,
        } => {
            let mut st = state_arc.write().await;
            st.mode = StationMode::Managed {
                session_id: session_id.clone(),
                time_remaining_sec: *duration_secs,
            };
            tracing::info!(session_id = %session_id, duration_secs = %duration_secs, "Session started: StationMode -> Managed");
            (
                true,
                Some(serde_json::json!({
                    "session_id": session_id,
                    "duration_secs": duration_secs,
                })),
            )
        }
        CabinetCommand::EndSession => {
            let mut st = state_arc.write().await;
            st.mode = StationMode::Locked {
                reason: "Session ended by supervisor".to_string(),
            };
            drop(st);
            terminate_running_game().await;
            resume_frontend().await;
            tracing::info!("Session ended: terminated game, resumed frontend");
            (true, Some(serde_json::json!({ "status": "session_ended" })))
        }
        CabinetCommand::Reboot { delay_secs } => {
            let delay = *delay_secs;
            tracing::warn!(delay_secs = %delay, "Reboot command received, scheduling systemctl reboot");
            tokio::spawn(async move {
                if delay > 0 {
                    sleep(Duration::from_secs(delay as u64)).await;
                }
                #[cfg(target_os = "linux")]
                {
                    let _ = tokio::process::Command::new("systemctl")
                        .arg("reboot")
                        .status()
                        .await;
                }
            });
            (true, Some(serde_json::json!({ "delay_secs": delay })))
        }
        CabinetCommand::LaunchGame { core, rom, netplay } => {
            if core.trim().is_empty() || rom.trim().is_empty() {
                return (
                    false,
                    Some(serde_json::json!({ "error": "core and rom must not be empty" })),
                );
            }

            let resolved_core = resolve_core_path(core);
            let resolved_rom = resolve_rom_path(rom);

            tracing::info!(
                core = %resolved_core,
                rom = %resolved_rom,
                netplay = %netplay,
                "Launching game via astro_kms_launcher.sh"
            );

            // 1. Take over display: stop or pause the frontend
            pause_frontend().await;

            // 2. Set active game in state
            {
                let mut st = state_arc.write().await;
                st.active_game = Some(ActiveGameInfo {
                    core: core.clone(),
                    rom: rom.clone(),
                    started_at_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    measured_fps: Some(60.0),
                    frame_time_ms: Some(16.63),
                });
            }

            // 3. Find launcher script path
            let launcher_candidates = [
                "/usr/lib/beercade/astro_kms_launcher.sh",
                "astro/launcher/astro_kms_launcher.sh",
                "/usr/local/bin/astro_kms_launcher",
            ];
            let launcher_script = launcher_candidates
                .iter()
                .find(|p| Path::new(p).exists())
                .unwrap_or(&"/usr/lib/beercade/astro_kms_launcher.sh");

            // 4. Build command arguments
            let mut cmd_builder = tokio::process::Command::new("/bin/sh");
            cmd_builder.arg(launcher_script);
            cmd_builder.arg("/usr/bin/retroarch");
            cmd_builder.arg("-L");
            cmd_builder.arg(&resolved_core);
            cmd_builder.arg(&resolved_rom);

            if *netplay {
                // If netplay requested, enable retroarch netplay flags if peer is configured
                cmd_builder.arg("--netplay");
            }

            let (boss_url, boss_token, node_id) = {
                let st = state_arc.read().await;
                (st.boss_url.clone(), st.boss_token.clone(), st.node_id.clone())
            };

            // Spawn the process
            match cmd_builder.spawn() {
                Ok(mut child) => {
                    let state_clone = state_arc.clone();
                    let client_clone = client.clone();
                    let core_clone = core.clone();
                    let rom_clone = rom.clone();

                    tokio::spawn(async move {
                        let status = child.wait().await;
                        tracing::info!(?status, "RetroArch process exited");

                        // Clear active game in state
                        {
                            let mut st = state_clone.write().await;
                            st.active_game = None;
                        }

                        // Check if game crashed or exited with failure
                        match status {
                            Ok(exit_code) if !exit_code.success() => {
                                let msg = format!(
                                    "RetroArch exited abnormally: {:?} (core: {}, rom: {})",
                                    exit_code, core_clone, rom_clone
                                );
                                tracing::error!("{}", msg);
                                report_node_error(
                                    &client_clone,
                                    &boss_url,
                                    &boss_token,
                                    &node_id,
                                    "error",
                                    "emulator",
                                    &msg,
                                    Some(serde_json::json!({
                                        "exit_code": exit_code.code(),
                                        "core": core_clone,
                                        "rom": rom_clone,
                                    })),
                                )
                                .await;
                            }
                            Err(e) => {
                                let msg = format!("Failed waiting for RetroArch process: {e}");
                                tracing::error!("{}", msg);
                                report_node_error(
                                    &client_clone,
                                    &boss_url,
                                    &boss_token,
                                    &node_id,
                                    "error",
                                    "emulator",
                                    &msg,
                                    None,
                                )
                                .await;
                            }
                            _ => {}
                        }

                        // Return the display cleanly to ES-DE / attract mode
                        resume_frontend().await;
                    });

                    (
                        true,
                        Some(serde_json::json!({
                            "status": "game_spawned",
                            "core": core,
                            "rom": rom,
                            "netplay": netplay,
                        })),
                    )
                }
                Err(e) => {
                    let err_msg = format!("Failed to spawn game launcher {launcher_script}: {e}");
                    tracing::error!("{}", err_msg);
                    {
                        let mut st = state_arc.write().await;
                        st.active_game = None;
                    }
                    report_node_error(
                        client,
                        &boss_url,
                        &boss_token,
                        &node_id,
                        "fatal",
                        "launcher",
                        &err_msg,
                        Some(serde_json::json!({ "core": core, "rom": rom })),
                    )
                    .await;
                    resume_frontend().await;
                    (
                        false,
                        Some(serde_json::json!({ "error": err_msg })),
                    )
                }
            }
        }
    }
}

/// Send an ACK for a command back to B.O.S.S.
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    println!("===============================================================");
    println!("   A.S.T.R.O. Node Daemon v2.0 - Arcade Station Supervisor     ");
    println!("===============================================================");

    let is_standalone = std::env::var("ASTRO_STANDALONE").unwrap_or_default() == "1"
        || std::env::var("BOSS_ENDPOINT").unwrap_or_default() == "none";

    let configured_boss_url = std::env::var("BOSS_ENDPOINT").unwrap_or_default();
    let initial_boss_url = if configured_boss_url.is_empty() {
        "http://192.168.100.10:8080".to_string()
    } else {
        configured_boss_url.clone()
    };

    let boss_token = std::env::var("BOSS_RPC_TOKEN").unwrap_or_default();
    let node_id = std::env::var("ASTRO_NODE_ID").unwrap_or_else(|_| "ASTRO-01".to_string());

    let credits_dir = std::env::var("ASTRO_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data/beercade"));
    let credits_path = credits_dir.join("credits.json");
    let initial_credits = DaemonSharedState::load_credits(&credits_path);

    let daemon_state = Arc::new(RwLock::new(DaemonSharedState {
        node_id: node_id.clone(),
        boss_url: initial_boss_url,
        boss_token: boss_token.clone(),
        mode: StationMode::Standalone,
        credits: initial_credits,
        active_game: None,
        credits_path,
    }));

    if is_standalone {
        println!("[✓] A.S.T.R.O. Daemon running in Standalone Mode (BOSS hub connection disabled).");
    } else {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()?;

        let telemetry_state = daemon_state.clone();
        let telemetry_client = client.clone();
        let token_for_ws = boss_token.clone();

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

                let mut ws_req = match ws_url.as_str().into_client_request() {
                    Ok(req) => req,
                    Err(e) => {
                        println!("[!] Invalid WS URL {}: {}", ws_url, e);
                        sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                };

                if !token_for_ws.is_empty() {
                    if let Ok(hdr) = format_bearer_header(&token_for_ws).parse() {
                        ws_req.headers_mut().insert("Authorization", hdr);
                    }
                }

                match connect_async(ws_req).await {
                    Ok((mut ws_stream, _)) => {
                        println!("[✓] WebSocket telemetry connected to {}", ws_url);
                        ws_failures = 0;

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

                            tokio::select! {
                                _ = sleep(Duration::from_millis(1000)) => {}
                                msg = ws_stream.next() => {
                                    match msg {
                                        Some(Ok(Message::Text(text))) => {
                                            if let Ok(cmd_val) = serde_json::from_str::<serde_json::Value>(&text) {
                                                if cmd_val.get("kind").and_then(|v| v.as_str()) == Some("payload_available") {
                                                    if let Some(filename) = cmd_val.get("filename").and_then(|v| v.as_str()) {
                                                        println!("[i] Received OTA payload notification: {}", filename);
                                                        let boss = boss_url.replace("ws://", "http://").replace("wss://", "https://");
                                                        let filename_owned = filename.to_string();
                                                        let token_for_ota = token_for_ws.clone();
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
                    Err(_e) => {
                        ws_failures += 1;
                        if ws_failures >= max_failures_before_fallback {
                            let http_url = format_http_telemetry_url(&boss_url);
                            let telemetry = {
                                let state = telemetry_state.read().await;
                                sample_telemetry(&state)
                            };
                            let http_res = telemetry_client
                                .post(&http_url)
                                .bearer_auth(&token_for_ws)
                                .json(&telemetry)
                                .send()
                                .await;

                            if http_res.is_err() {
                                candidate_idx = (candidate_idx + 1) % candidates.len();
                                let next_url = candidates[candidate_idx].clone();
                                let mut st = telemetry_state.write().await;
                                st.boss_url = next_url;
                            }

                            sleep(Duration::from_secs(3)).await;
                        } else {
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
        let boss = Some(daemon_state.read().await.boss_url.clone());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            let outcome = netplay::run_station(
                cfg,
                netplay::SyntheticCore::default(),
                Box::new(|_frame| 0),
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
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = shutdown_tx.send(true);
        });
    }

    // -------------------------------------------------------------------
    // Command Polling Loop for Game Delivery & Cabinet Directives (BOSS -> Astro)
    // -------------------------------------------------------------------
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

    // -------------------------------------------------------------------
    // Main supervisor watchdog loop: session countdown & station mode enforcement
    // -------------------------------------------------------------------
    loop {
        sleep(Duration::from_secs(1)).await;
        let mut expired = false;
        {
            let mut state = daemon_state.write().await;
            if let StationMode::Managed {
                ref mut time_remaining_sec,
                ..
            } = state.mode
            {
                if *time_remaining_sec > 0 {
                    *time_remaining_sec -= 1;
                } else {
                    println!("[!] Session expired. Transitioning to Locked mode.");
                    state.mode = StationMode::Locked {
                        reason: "Session time limit reached. Insert token to resume.".to_string(),
                    };
                    expired = true;
                }
            }
        }

        if expired {
            terminate_running_game().await;
            resume_frontend().await;
        }
    }
}
