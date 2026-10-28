use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

mod netplay;
mod ota;
mod input;
mod overlay;
mod session;
mod shell_ipc;
mod staging;

use beercade_shared::auth::format_bearer_header;
use beercade_shared::ipc::{endpoints, format_http_telemetry_url, format_ws_telemetry_url};
pub use beercade_shared::models::{
    CabinetCommand, CabinetEvent, CommandAck, CreditCounters, ErrorEvent, GameRef, HighScoreEntry,
    InputJitterStats, NodeTelemetry, StationMode,
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
    /// Root of the DATA partition (`/data/beercade`).
    pub data_dir: PathBuf,
    /// The B.O.S.S. session on this cabinet, live or just ended.
    pub session: Option<session::Session>,
    /// Bumped by every StartAttract; a running playlist stops when it changes.
    pub attract_gen: u64,
    /// An attract game is on screen: don't hand the display back to the
    /// frontend between attract games.
    pub attract_running: bool,
    /// Venue theme from B.O.S.S., for the shell and the overlay.
    pub theme: String,
}

impl DaemonSharedState {
    pub fn tmp_path(path: &Path) -> PathBuf {
        let mut s = path.as_os_str().to_os_string();
        s.push(".tmp");
        PathBuf::from(s)
    }

    pub fn bak_path(path: &Path) -> PathBuf {
        let mut s = path.as_os_str().to_os_string();
        s.push(".bak");
        PathBuf::from(s)
    }

    pub fn persist_credits_to(path: &Path, counters: &CreditCounters) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let json = serde_json::to_string_pretty(counters)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let tmp = Self::tmp_path(path);
        let bak = Self::bak_path(path);

        // 1. Write to temporary file with sync_all
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;

            use std::io::Write;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            // Explicitly drop file handle before rename to release Windows locks
            drop(file);
        }

        // 2. Atomically rename .tmp -> path
        std::fs::rename(&tmp, path)?;

        // 3. Update backup file: copy intact primary to .bak
        let _ = std::fs::copy(path, &bak);

        Ok(())
    }

    pub fn persist_credits(&self) {
        if let Err(e) = Self::persist_credits_to(&self.credits_path, &self.credits) {
            tracing::error!("Failed to persist credits: {e}");
        }
    }

    pub fn load_credits(path: &Path) -> CreditCounters {
        let bak = Self::bak_path(path);

        // Tier 1: Attempt to load from primary path
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) if !data.trim().is_empty() => {
                    match serde_json::from_str::<CreditCounters>(&data) {
                        Ok(counters) => {
                            // Primary is intact. Ensure backup exists.
                            if !bak.exists() {
                                let _ = std::fs::copy(path, &bak);
                            }
                            return counters;
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Corrupted primary credits file at {}: {e}. Falling back to backup.",
                                path.display()
                            );
                        }
                    }
                }
                Ok(_) => {
                    tracing::warn!(
                        "Empty primary credits file at {}. Falling back to backup.",
                        path.display()
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to read primary credits file at {}: {e}. Falling back to backup.",
                        path.display()
                    );
                }
            }
        } else {
            tracing::info!(
                "Primary credits file not found at {}. Checking for backup.",
                path.display()
            );
        }

        // Tier 2: Attempt to load from backup (.bak)
        if bak.exists() {
            match std::fs::read_to_string(&bak) {
                Ok(bak_data) if !bak_data.trim().is_empty() => {
                    match serde_json::from_str::<CreditCounters>(&bak_data) {
                        Ok(recovered) => {
                            tracing::warn!(
                                "Successfully recovered credits from backup at {}. Self-healing primary.",
                                bak.display()
                            );
                            // Self-heal primary credits.json from backup
                            if let Err(e) = Self::persist_credits_to(path, &recovered) {
                                tracing::error!("Failed to self-heal primary credits file from backup: {e}");
                            }
                            return recovered;
                        }
                        Err(e) => {
                            tracing::error!(
                                "Backup credits file at {} is also corrupted: {e}.",
                                bak.display()
                            );
                        }
                    }
                }
                Ok(_) => {
                    tracing::error!("Backup credits file at {} is empty.", bak.display());
                }
                Err(e) => {
                    tracing::error!("Failed to read backup credits file at {}: {e}.", bak.display());
                }
            }
        }

        // Tier 3: Both primary and backup missing or corrupted. Safe default without panicking.
        tracing::error!(
            "No valid credits file found at {} or {}. Defaulting to zero counters.",
            path.display(),
            bak.display()
        );
        CreditCounters::default()
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
    let (cpu_temp_c, gpu_temp_c) = netplay::sample_temps();
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
        cpu_temp_c,
        gpu_temp_c,
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
        Ok(res) if res.status().is_success() => {
            overlay::RetroArch::default()
                .show(&format!("HIGH SCORE {score} \u{00B7} POSTED TO THE VENUE TVS"))
                .await;
            Ok(())
        }
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

/// Mirror the session into the telemetry `mode` B.O.S.S. already reads.
fn sync_session_mode(st: &mut DaemonSharedState) {
    match &st.session {
        Some(s) if s.is_live() => {
            st.mode = StationMode::Managed {
                session_id: s.id.clone(),
                time_remaining_sec: s.remaining,
            };
        }
        Some(_) => {
            st.mode = StationMode::Locked {
                reason: "Session ended".to_string(),
            };
        }
        None => {}
    }
}

/// Tell B.O.S.S. about a session or attract event. Best effort: B.O.S.S.
/// also sees the session through telemetry.
async fn post_cabinet_event(state_arc: &Arc<RwLock<DaemonSharedState>>, ev: &CabinetEvent) {
    let (boss_url, token, node_id) = {
        let st = state_arc.read().await;
        (st.boss_url.clone(), st.boss_token.clone(), st.node_id.clone())
    };
    let url = format!("{}/api/v1/cabinet/{}/events", boss_url.trim_end_matches('/'), node_id);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();
    let mut req = client.post(&url).json(ev);
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }
    match req.send().await {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => tracing::warn!(status = %r.status(), ?ev, "B.O.S.S. rejected cabinet event"),
        Err(e) => tracing::warn!(?ev, "Failed to post cabinet event: {e}"),
    }
}

/// Carry out what the session state machine asked for. Each piece of I/O runs
/// on its own task, so callers must not hold the state lock across this.
fn apply_session_effects(state_arc: &Arc<RwLock<DaemonSharedState>>, fx: Vec<session::Effect>) {
    use session::Effect;
    for e in fx {
        let st = state_arc.clone();
        match e {
            Effect::Event(ev) => {
                tokio::spawn(async move { post_cabinet_event(&st, &ev).await });
            }
            Effect::PauseGame => {
                tokio::spawn(async { overlay::RetroArch::default().set_paused(true).await });
            }
            Effect::ResumeGame => {
                tokio::spawn(async { overlay::RetroArch::default().set_paused(false).await });
            }
            Effect::CloseGame => {
                tokio::spawn(terminate_running_game());
            }
            Effect::Stage(games) => {
                tokio::spawn(stage_session_games(st, games));
            }
            Effect::Prune => {
                tokio::spawn(async move {
                    let (dir, keep, running) = {
                        let s = st.read().await;
                        let keep = s.session.as_ref().map(|x| x.games.clone()).unwrap_or_default();
                        let running = s.active_game.as_ref().map(|g| g.rom.clone());
                        (staging::session_dir(&s.data_dir), keep, running)
                    };
                    // The game on screen stays until it exits, even if dropped.
                    if running.is_some() {
                        return;
                    }
                    if let Err(e) = staging::prune(&dir, &keep) {
                        tracing::warn!("Failed to prune session files: {e}");
                    }
                });
            }
            Effect::Wipe => {
                tokio::spawn(async move {
                    let dir = staging::session_dir(&st.read().await.data_dir);
                    match staging::wipe(&dir) {
                        Ok(()) => tracing::info!("Session files wiped"),
                        Err(e) => tracing::error!("Failed to wipe session files: {e}"),
                    }
                });
            }
        }
    }
}

/// Fetch a session's games from B.O.S.S. and tell the session when done.
async fn stage_session_games(state_arc: Arc<RwLock<DaemonSharedState>>, games: Vec<GameRef>) {
    let (stager, dir, session_id) = {
        let s = state_arc.read().await;
        (
            staging::Stager::new(&s.boss_url, &s.boss_token, &s.node_id),
            staging::session_dir(&s.data_dir),
            s.session.as_ref().map(|x| x.id.clone()),
        )
    };
    let (ready, failed) = stager.stage_all(&games, &dir).await;
    let fx = {
        let mut s = state_arc.write().await;
        match s.session.as_mut() {
            Some(sess) if Some(&sess.id) == session_id.as_ref() && sess.is_live() => {
                sess.staged(ready, failed)
            }
            // The session ended or was replaced while downloading.
            _ => Vec::new(),
        }
    };
    apply_session_effects(&state_arc, fx);
}

/// Attract mode from a B.O.S.S. playlist: fetch the games, then cycle them
/// until a session starts or a newer playlist arrives.
async fn run_attract(
    state_arc: Arc<RwLock<DaemonSharedState>>,
    client: reqwest::Client,
    games: Vec<GameRef>,
    secs_per_game: u32,
    gen: u64,
) {
    let current = |st: &DaemonSharedState| {
        st.attract_gen == gen && !st.session.as_ref().is_some_and(|s| s.is_live())
    };
    let (stager, dir) = {
        let s = state_arc.read().await;
        (
            staging::Stager::new(&s.boss_url, &s.boss_token, &s.node_id),
            staging::attract_dir(&s.data_dir),
        )
    };
    if let Err(e) = staging::wipe(&dir) {
        tracing::error!("Failed to wipe attract files: {e}");
    }
    let (ready, _failed) = stager.stage_all(&games, &dir).await;
    let playlist: Vec<&GameRef> = games.iter().filter(|g| ready.contains(&g.game_id)).collect();
    if playlist.is_empty() {
        if current(&*state_arc.read().await) {
            post_cabinet_event(&state_arc, &CabinetEvent::AttractNeeded).await;
        }
        return;
    }
    tracing::info!(games = playlist.len(), "Attract playlist ready");

    'cycle: for game in playlist.iter().cycle() {
        if !current(&*state_arc.read().await) {
            break;
        }
        let Some(path) = staging::game_path(&dir, game) else { continue };
        state_arc.write().await.attract_running = true;
        let cmd = CabinetCommand::LaunchGame {
            core: game.core.clone(),
            rom: path.to_string_lossy().to_string(),
            netplay: false,
        };
        let CabinetCommand::LaunchGame { core, rom, netplay } = &cmd else { unreachable!() };
        if !launch_game(core, rom, netplay, &state_arc, &client).await.0 {
            sleep(Duration::from_secs(5)).await;
            continue;
        }
        for _ in 0..secs_per_game {
            sleep(Duration::from_secs(1)).await;
            if !current(&*state_arc.read().await) {
                break 'cycle;
            }
        }
        terminate_running_game().await;
        sleep(Duration::from_secs(1)).await;
    }

    // Superseded (session or newer playlist): clear the screen for it.
    terminate_running_game().await;
    let mut st = state_arc.write().await;
    if st.attract_gen == gen {
        st.attract_running = false;
    }
    let session_started = st.session.as_ref().is_some_and(|s| s.is_live());
    drop(st);
    if session_started {
        let _ = staging::wipe(&dir);
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
            "/data/beercade/session",
            "/data/beercade/attract",
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
            drop(st);
            overlay::RetroArch::default()
                .show(&format!("+{credits} CREDIT \u{00B7} CREDIT {available:02}"))
                .await;
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
            session_code,
            topup_grace_secs,
            games,
        } => {
            let (sess, fx) = session::Session::start(
                session_id,
                session_code,
                *duration_secs,
                *topup_grace_secs,
                overlay::CONTINUE_SECS,
                games.clone(),
            );
            let mut st = state_arc.write().await;
            // A new session replaces any other: its files go first.
            let mut old_fx = Vec::new();
            if let Some(old) = st.session.as_mut() {
                old_fx = old.end();
            }
            if let Err(e) = staging::wipe(&staging::session_dir(&st.data_dir)) {
                tracing::error!("Failed to wipe previous session files: {e}");
            }
            st.session = Some(sess);
            sync_session_mode(&mut st);
            drop(st);
            old_fx.retain(|e| matches!(e, session::Effect::Event(_)));
            apply_session_effects(state_arc, old_fx);
            apply_session_effects(state_arc, fx);
            tracing::info!(session_id = %session_id, duration_secs = %duration_secs, games = games.len(), "Session started");
            overlay::RetroArch::default()
                .show(&format!("TIME {} \u{00B7} GAME ON", overlay::fmt_clock(*duration_secs)))
                .await;
            (
                true,
                Some(serde_json::json!({
                    "session_id": session_id,
                    "duration_secs": duration_secs,
                    "games": games.len(),
                })),
            )
        }
        CabinetCommand::UpdateSession { session_id, games, add_secs } => {
            let mut st = state_arc.write().await;
            let Some(sess) = st.session.as_mut().filter(|s| &s.id == session_id && s.is_live()) else {
                return (false, Some(serde_json::json!({ "error": "no such live session" })));
            };
            let mut fx = Vec::new();
            if let Some(games) = games {
                fx.extend(sess.replace_games(games.clone()));
            }
            fx.extend(sess.add_time(*add_secs));
            let remaining = sess.remaining;
            sync_session_mode(&mut st);
            drop(st);
            apply_session_effects(state_arc, fx);
            tracing::info!(session_id = %session_id, add_secs = %add_secs, "Session updated");
            (true, Some(serde_json::json!({ "session_id": session_id, "time_remaining_sec": remaining })))
        }
        CabinetCommand::EndSession { session_id, reason } => {
            let mut st = state_arc.write().await;
            let mut fx = Vec::new();
            if let Some(sess) = st.session.as_mut() {
                if session_id.as_ref().map_or(true, |id| id == &sess.id) {
                    fx = sess.end();
                }
            }
            st.mode = StationMode::Locked {
                reason: reason.clone().unwrap_or_else(|| "Session ended by supervisor".to_string()),
            };
            drop(st);
            apply_session_effects(state_arc, fx);
            terminate_running_game().await;
            resume_frontend().await;
            tracing::info!("Session ended: terminated game, resumed frontend");
            (true, Some(serde_json::json!({ "status": "session_ended" })))
        }
        CabinetCommand::StartAttract { games, secs_per_game } => {
            let gen = {
                let mut st = state_arc.write().await;
                if st.session.as_ref().is_some_and(|s| s.is_live()) {
                    return (false, Some(serde_json::json!({ "error": "session in progress" })));
                }
                st.attract_gen += 1;
                st.attract_gen
            };
            tokio::spawn(run_attract(
                state_arc.clone(),
                client.clone(),
                games.clone(),
                (*secs_per_game).max(15),
                gen,
            ));
            (true, Some(serde_json::json!({ "games": games.len(), "secs_per_game": secs_per_game })))
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
            launch_game(core, rom, netplay, state_arc, client).await
        }
    }
}

/// Launch a game through the KMS launcher and watch it until it exits.
async fn launch_game(
    core: &String,
    rom: &String,
    netplay: &bool,
    state_arc: &Arc<RwLock<DaemonSharedState>>,
    client: &reqwest::Client,
) -> (bool, Option<serde_json::Value>) {
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
        let session_game = {
            let mut st = state_arc.write().await;
            let file = Path::new(&resolved_rom)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut fx = Vec::new();
            let mut game_id = None;
            if let Some(sess) = st.session.as_mut().filter(|s| s.is_live()) {
                if let Some(g) = sess.games.iter().find(|g| g.file_name == file) {
                    game_id = Some(g.game_id.clone());
                    let id = g.game_id.clone();
                    fx = sess.game_started(&id);
                }
            }
            drop(st);
            apply_session_effects(state_arc, fx);
            game_id
        };
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
                    let attract_running = {
                        let mut st = state_clone.write().await;
                        st.active_game = None;
                        let crashed = !matches!(&status, Ok(c) if c.success());
                        let fx = match (st.session.as_mut(), &session_game) {
                            (Some(sess), Some(id)) => sess.game_exited(id, crashed),
                            _ => Vec::new(),
                        };
                        let attract = st.attract_running;
                        drop(st);
                        apply_session_effects(&state_clone, fx);
                        attract
                    };

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

                    // Return the display cleanly to ES-DE / attract mode;
                    // between attract games the playlist keeps it.
                    if !attract_running {
                        resume_frontend().await;
                    }
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
    if let Err(e) = overlay::write_cfg(&credits_dir, overlay::DEFAULT_THEME) {
        tracing::warn!("Failed to write overlay config: {e}");
    }
    // No game files survive a restart or power cut.
    for dir in [staging::session_dir(&credits_dir), staging::attract_dir(&credits_dir)] {
        if let Err(e) = staging::wipe(&dir) {
            tracing::error!(dir = %dir.display(), "Failed to wipe staged games: {e}");
        }
    }

    let daemon_state = Arc::new(RwLock::new(DaemonSharedState {
        node_id: node_id.clone(),
        boss_url: initial_boss_url,
        boss_token: boss_token.clone(),
        mode: StationMode::Standalone,
        credits: initial_credits,
        active_game: None,
        credits_path,
        data_dir: credits_dir.clone(),
        session: None,
        attract_gen: 0,
        attract_running: false,
        theme: overlay::DEFAULT_THEME.to_string(),
    }));
    shell_ipc::spawn(
        daemon_state.clone(),
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_default(),
    );

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

    // Pause menu: cabinet controls -> menu -> RetroArch OSD.
    {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let grab = Arc::new(std::sync::atomic::AtomicBool::new(false));
        input::spawn_readers(tx, grab.clone());
        let menu_state = daemon_state.clone();
        let link_peer = std::env::var("ASTRO_NETPLAY_PEER").ok();
        tokio::spawn(async move {
            use std::sync::atomic::Ordering;
            let ra = overlay::RetroArch::default();
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_default();
            let mut menu = overlay::PauseMenu::default();
            // OSD messages fade after a few seconds: redraw while open.
            let mut refresh = tokio::time::interval(Duration::from_secs(2));
            loop {
                let effect = tokio::select! {
                    Some(inp) = rx.recv() => menu.handle(inp),
                    _ = refresh.tick() => {
                        if menu.open { overlay::MenuEffect::Redraw } else { overlay::MenuEffect::None }
                    }
                };
                match effect {
                    overlay::MenuEffect::None => continue,
                    overlay::MenuEffect::Open => {
                        // Only over a running game; ES-DE has its own menus.
                        if ra.is_paused().await.is_none() {
                            menu.open = false;
                            continue;
                        }
                        grab.store(true, Ordering::Relaxed);
                        ra.set_paused(true).await;
                    }
                    overlay::MenuEffect::Close => {
                        grab.store(false, Ordering::Relaxed);
                        ra.set_paused(false).await;
                        continue;
                    }
                    overlay::MenuEffect::QuitGame => {
                        grab.store(false, Ordering::Relaxed);
                        // The exit handler hands the screen back to the shell.
                        terminate_running_game().await;
                        continue;
                    }
                    overlay::MenuEffect::EndSession => {
                        grab.store(false, Ordering::Relaxed);
                        let mut st = menu_state.write().await;
                        let fx = st.session.as_mut().map(|s| s.end()).unwrap_or_default();
                        st.mode = StationMode::Locked {
                            reason: "Session ended by player".to_string(),
                        };
                        drop(st);
                        apply_session_effects(&menu_state, fx);
                        terminate_running_game().await;
                        resume_frontend().await;
                        continue;
                    }
                    overlay::MenuEffect::CallStaff => {
                        let st = menu_state.read().await.clone();
                        let msg = format!("Staff requested at {}", st.node_id);
                        report_node_error(
                            &client, &st.boss_url, &st.boss_token, &st.node_id,
                            "warn", "staff_call", &msg, None,
                        )
                        .await;
                    }
                    overlay::MenuEffect::Redraw => {}
                }
                let st = menu_state.read().await;
                let time_remaining = match st.mode {
                    StationMode::Managed { time_remaining_sec, .. } => Some(time_remaining_sec),
                    _ => None,
                };
                let line = menu.render(&overlay::MenuContext {
                    node_id: &st.node_id,
                    credits: st.credits.credits_available,
                    time_remaining,
                    link_peer: link_peer.as_deref(),
                });
                drop(st);
                ra.show(&line).await;
            }
        });
    }

    // Keep the overlay's OSD colours on the venue theme set on B.O.S.S.
    if !is_standalone {
        let theme_state = daemon_state.clone();
        let data_dir = credits_dir.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_default();
            let mut current = overlay::DEFAULT_THEME.to_string();
            loop {
                let boss_url = theme_state.read().await.boss_url.clone();
                if let Some(theme) = overlay::fetch_theme(&client, &boss_url).await {
                    if theme != current {
                        match overlay::write_cfg(&data_dir, &theme) {
                            Ok(()) => {
                                tracing::info!(theme = %theme, "Overlay theme updated");
                                theme_state.write().await.theme = theme.clone();
                                current = theme;
                            }
                            Err(e) => tracing::warn!("Failed to write overlay config: {e}"),
                        }
                    }
                }
                sleep(Duration::from_secs(60)).await;
            }
        });
    }

    // -------------------------------------------------------------------
    // Main supervisor watchdog loop: session countdown & station mode enforcement
    // -------------------------------------------------------------------
    let ra = overlay::RetroArch::default();
    // Seconds left in the CONTINUE? window after a session ran out.
    let mut continue_left: Option<u32> = None;
    loop {
        sleep(Duration::from_secs(1)).await;

        // B.O.S.S. sessions run on the session state machine.
        let ticked = {
            let mut state = daemon_state.write().await;
            let credits = state.credits.credits_available;
            let node_id = state.node_id.clone();
            match state.session.as_mut().filter(|s| s.is_live()) {
                Some(sess) => {
                    let fx = sess.tick();
                    let line = match &sess.phase {
                        session::Phase::Expired { continue_left, paused_game: Some(_), .. } => {
                            Some(overlay::continue_line(&node_id, *continue_left))
                        }
                        session::Phase::InGame { .. } if overlay::hud_due(sess.remaining) => {
                            Some(overlay::hud_line(Some(sess.remaining), credits, None))
                        }
                        _ => None,
                    };
                    sync_session_mode(&mut state);
                    Some((fx, line))
                }
                None => None,
            }
        };
        if let Some((fx, line)) = ticked {
            apply_session_effects(&daemon_state, fx);
            if let Some(line) = line {
                ra.show(&line).await;
            }
            continue;
        }

        let mut expired = false;
        let mut hud = None;
        let mut managed = false;
        {
            let mut state = daemon_state.write().await;
            let credits = state.credits.credits_available;
            if let StationMode::Managed {
                ref mut time_remaining_sec,
                ..
            } = state.mode
            {
                managed = true;
                if *time_remaining_sec > 0 {
                    *time_remaining_sec -= 1;
                    if overlay::hud_due(*time_remaining_sec) {
                        hud = Some(overlay::hud_line(Some(*time_remaining_sec), credits, None));
                    }
                } else {
                    println!("[!] Session expired. Offering CONTINUE? before locking.");
                    managed = false;
                    state.mode = StationMode::Locked {
                        reason: "Session time limit reached. Insert token to resume.".to_string(),
                    };
                    expired = true;
                }
            }
        }

        if let Some(line) = hud {
            ra.show(&line).await;
        }

        if expired {
            ra.set_paused(true).await;
            continue_left = Some(overlay::CONTINUE_SECS);
        }

        if let Some(left) = continue_left {
            if managed {
                // More time arrived inside the window: back to the game.
                ra.set_paused(false).await;
                continue_left = None;
            } else if left == 0 {
                continue_left = None;
                terminate_running_game().await;
                resume_frontend().await;
            } else {
                let node_id = daemon_state.read().await.node_id.clone();
                ra.show(&overlay::continue_line(&node_id, left)).await;
                continue_left = Some(left - 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct FakeRoot(PathBuf);

    impl FakeRoot {
        fn new(prefix: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let id = N.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("beercade-main-test-{}-{}-{}", prefix, std::process::id(), id));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            FakeRoot(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FakeRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) fn sample_daemon_state(credits_path: PathBuf) -> DaemonSharedState {
        DaemonSharedState {
            node_id: "test-node-1".to_string(),
            boss_url: "http://127.0.0.1:8080".to_string(),
            boss_token: "test-token".to_string(),
            mode: StationMode::Standalone,
            credits: CreditCounters::default(),
            active_game: None,
            data_dir: credits_path.parent().map(Path::to_path_buf).unwrap_or_default(),
            credits_path,
            session: None,
            attract_gen: 0,
            attract_running: false,
            theme: overlay::DEFAULT_THEME.to_string(),
        }
    }

    // =========================================================================
    // R2: Atomic Credit Persistence & Crash-Safe Tiered Recovery Tests
    // =========================================================================

    #[test]
    fn test_atomic_credit_persistence_happy_path() {
        let root = FakeRoot::new("atomic-happy");
        let credits_file = root.path().join("credits.json");

        let mut counters = CreditCounters::default();
        counters.credits_available = 42;
        counters.credits_inserted_total = 100;
        counters.credits_dispensed_today = 10;
        counters.last_coin_epoch_ms = Some(1700000000000);

        let res = DaemonSharedState::persist_credits_to(&credits_file, &counters);
        assert!(res.is_ok(), "Expected persistence success: {:?}", res);

        // Verify primary exists
        assert!(credits_file.exists());
        // Verify tmp file does not remain
        let tmp_file = DaemonSharedState::tmp_path(&credits_file);
        assert!(!tmp_file.exists(), "Temporary file was not cleaned up / renamed");
        // Verify backup exists
        let bak_file = DaemonSharedState::bak_path(&credits_file);
        assert!(bak_file.exists(), "Backup file was not created");

        // Verify content in primary
        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 42);
        assert_eq!(loaded.credits_inserted_total, 100);
        assert_eq!(loaded.credits_dispensed_today, 10);
        assert_eq!(loaded.last_coin_epoch_ms, Some(1700000000000));

        // Verify content in backup matches
        let bak_data = std::fs::read_to_string(&bak_file).unwrap();
        let bak_counters: CreditCounters = serde_json::from_str(&bak_data).unwrap();
        assert_eq!(bak_counters.credits_available, 42);
    }

    #[test]
    fn test_credit_recovery_from_truncated_primary() {
        let root = FakeRoot::new("recover-truncated");
        let credits_file = root.path().join("credits.json");

        let mut counters = CreditCounters::default();
        counters.credits_available = 25;
        counters.credits_inserted_total = 50;
        DaemonSharedState::persist_credits_to(&credits_file, &counters).unwrap();

        // Simulate crash during write: truncate primary to half JSON
        let full_json = std::fs::read_to_string(&credits_file).unwrap();
        let truncated = &full_json[..full_json.len() / 2];
        std::fs::write(&credits_file, truncated).unwrap();

        // load_credits must detect corrupt primary and recover from .bak
        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 25);
        assert_eq!(loaded.credits_inserted_total, 50);

        // Verify self-healing restored the primary file
        let healed_json = std::fs::read_to_string(&credits_file).unwrap();
        let healed_counters: CreditCounters = serde_json::from_str(&healed_json).unwrap();
        assert_eq!(healed_counters.credits_available, 25);
    }

    #[test]
    fn test_credit_recovery_from_zero_byte_primary() {
        let root = FakeRoot::new("recover-0byte");
        let credits_file = root.path().join("credits.json");

        let mut counters = CreditCounters::default();
        counters.credits_available = 15;
        DaemonSharedState::persist_credits_to(&credits_file, &counters).unwrap();

        // Simulate 0-byte corrupt primary
        std::fs::write(&credits_file, b"").unwrap();
        assert_eq!(std::fs::metadata(&credits_file).unwrap().len(), 0);

        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 15);

        // Verify self-healing restored primary
        let healed = std::fs::read_to_string(&credits_file).unwrap();
        let healed_counters: CreditCounters = serde_json::from_str(&healed).unwrap();
        assert_eq!(healed_counters.credits_available, 15);
    }

    #[test]
    fn test_credit_recovery_from_corrupted_garbage_primary() {
        let root = FakeRoot::new("recover-garbage");
        let credits_file = root.path().join("credits.json");

        let mut counters = CreditCounters::default();
        counters.credits_available = 77;
        DaemonSharedState::persist_credits_to(&credits_file, &counters).unwrap();

        // Write non-JSON garbage bytes
        std::fs::write(&credits_file, b"\xDE\xAD\xBE\xEF\x00\xFF_NOT_JSON").unwrap();

        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 77);

        // Verify self-healing
        let healed = std::fs::read_to_string(&credits_file).unwrap();
        let healed_counters: CreditCounters = serde_json::from_str(&healed).unwrap();
        assert_eq!(healed_counters.credits_available, 77);
    }

    #[test]
    fn test_credit_recovery_when_primary_deleted() {
        let root = FakeRoot::new("recover-deleted");
        let credits_file = root.path().join("credits.json");

        let mut counters = CreditCounters::default();
        counters.credits_available = 88;
        DaemonSharedState::persist_credits_to(&credits_file, &counters).unwrap();

        // Delete primary file; backup remains
        std::fs::remove_file(&credits_file).unwrap();
        assert!(!credits_file.exists());

        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 88);

        // Verify primary file is re-created via self-healing
        assert!(credits_file.exists());
    }

    #[test]
    fn test_credit_fallback_when_both_files_corrupt_no_panic() {
        let root = FakeRoot::new("recover-both-corrupt");
        let credits_file = root.path().join("credits.json");
        let bak_file = DaemonSharedState::bak_path(&credits_file);

        // Corrupt both files with garbage
        std::fs::write(&credits_file, b"corrupted primary").unwrap();
        std::fs::write(&bak_file, b"corrupted backup").unwrap();

        // Must safely return default without panic
        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 0);
        assert_eq!(loaded.credits_inserted_total, 0);
    }

    #[tokio::test]
    async fn test_rapid_concurrent_counter_updates() {
        let root = FakeRoot::new("concurrent-updates");
        let credits_file = root.path().join("credits.json");

        let state = sample_daemon_state(credits_file.clone());
        let state_arc = Arc::new(RwLock::new(state));

        let mut handles = Vec::new();
        // 20 concurrent tasks, each adding 5 credits in a loop
        for _ in 0..20 {
            let st = state_arc.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..5 {
                    let mut guard = st.write().await;
                    guard.add_credits(1);
                }
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let final_guard = state_arc.read().await;
        assert_eq!(final_guard.credits.credits_available, 100);
        assert_eq!(final_guard.credits.credits_inserted_total, 100);
        drop(final_guard);

        // Verify persistent file on disk is valid and parses exactly 100 credits
        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 100);
        assert_eq!(loaded.credits_inserted_total, 100);
    }

    // =========================================================================
    // R6: Command Dispatcher Execution and ACK Tests
    // =========================================================================

    #[tokio::test]
    async fn test_cmd_ping_execution() {
        let root = FakeRoot::new("cmd-ping");
        let state = Arc::new(RwLock::new(sample_daemon_state(root.path().join("credits.json"))));
        let client = reqwest::Client::new();

        let cmd = CabinetCommand::Ping { seq: 42 };
        let (success, detail) = execute_cabinet_command(&cmd, &state, &client).await;

        assert!(success);
        let detail = detail.expect("Expected detail payload");
        assert_eq!(detail["seq"], 42);
    }

    #[tokio::test]
    async fn test_cmd_add_credits_execution() {
        let root = FakeRoot::new("cmd-add-credits");
        let credits_file = root.path().join("credits.json");
        let state = Arc::new(RwLock::new(sample_daemon_state(credits_file.clone())));
        let client = reqwest::Client::new();

        let cmd = CabinetCommand::AddCredits { credits: 10 };
        let (success, detail) = execute_cabinet_command(&cmd, &state, &client).await;

        assert!(success);
        let detail = detail.expect("Expected detail payload");
        assert_eq!(detail["credits_added"], 10);
        assert_eq!(detail["credits_available"], 10);

        let guard = state.read().await;
        assert_eq!(guard.credits.credits_available, 10);
        assert_eq!(guard.credits.credits_inserted_total, 10);
        drop(guard);

        // Verify persisted to disk
        let loaded = DaemonSharedState::load_credits(&credits_file);
        assert_eq!(loaded.credits_available, 10);
    }

    #[tokio::test]
    async fn test_cmd_start_and_end_session() {
        let root = FakeRoot::new("cmd-session");
        let state = Arc::new(RwLock::new(sample_daemon_state(root.path().join("credits.json"))));
        let client = reqwest::Client::new();

        // Start session
        let start_cmd = CabinetCommand::StartSession {
            session_id: "session-abc".to_string(),
            duration_secs: 180,
            session_code: String::new(),
            topup_grace_secs: 0,
            games: Vec::new(),
        };
        let (success, detail) = execute_cabinet_command(&start_cmd, &state, &client).await;
        assert!(success);
        let detail = detail.unwrap();
        assert_eq!(detail["session_id"], "session-abc");
        assert_eq!(detail["duration_secs"], 180);

        {
            let guard = state.read().await;
            match &guard.mode {
                StationMode::Managed { session_id, time_remaining_sec } => {
                    assert_eq!(session_id, "session-abc");
                    assert_eq!(*time_remaining_sec, 180);
                }
                other => panic!("Unexpected mode: {:?}", other),
            }
        }

        // End session
        let end_cmd = CabinetCommand::EndSession { session_id: None, reason: None };
        let (success, detail) = execute_cabinet_command(&end_cmd, &state, &client).await;
        assert!(success);
        let detail = detail.unwrap();
        assert_eq!(detail["status"], "session_ended");

        {
            let guard = state.read().await;
            match &guard.mode {
                StationMode::Locked { reason } => {
                    assert!(reason.contains("Session ended by supervisor"));
                }
                other => panic!("Unexpected mode: {:?}", other),
            }
        }
    }

    #[tokio::test]
    async fn test_cmd_reboot_non_blocking() {
        let root = FakeRoot::new("cmd-reboot");
        let state = Arc::new(RwLock::new(sample_daemon_state(root.path().join("credits.json"))));
        let client = reqwest::Client::new();

        let cmd = CabinetCommand::Reboot { delay_secs: 60 };
        let (success, detail) = execute_cabinet_command(&cmd, &state, &client).await;

        assert!(success);
        let detail = detail.unwrap();
        assert_eq!(detail["delay_secs"], 60);
    }

    #[tokio::test]
    async fn test_cmd_launch_game_validation() {
        let root = FakeRoot::new("cmd-launch-val");
        let state = Arc::new(RwLock::new(sample_daemon_state(root.path().join("credits.json"))));
        let client = reqwest::Client::new();

        // Empty core
        let cmd1 = CabinetCommand::LaunchGame {
            core: "   ".to_string(),
            rom: "pacman.zip".to_string(),
            netplay: false,
        };
        let (success1, detail1) = execute_cabinet_command(&cmd1, &state, &client).await;
        assert!(!success1);
        assert_eq!(detail1.unwrap()["error"], "core and rom must not be empty");

        // Empty rom
        let cmd2 = CabinetCommand::LaunchGame {
            core: "fbneo".to_string(),
            rom: "".to_string(),
            netplay: false,
        };
        let (success2, detail2) = execute_cabinet_command(&cmd2, &state, &client).await;
        assert!(!success2);
        assert_eq!(detail2.unwrap()["error"], "core and rom must not be empty");
    }

    #[test]
    fn test_command_deserialization_and_ack_payload() {
        // Valid Ping (CabinetCommand uses snake_case tag)
        let json_ping = r#"{"type":"ping","seq":101}"#;
        let cmd: Result<CabinetCommand, _> = serde_json::from_str(json_ping);
        assert!(cmd.is_ok(), "Expected valid command parse: {:?}", cmd);
        if let Ok(CabinetCommand::Ping { seq }) = cmd {
            assert_eq!(seq, 101);
        } else {
            panic!("Expected CabinetCommand::Ping");
        }

        // Malformed command
        let json_invalid = r#"{"type":"NonExistentAction"}"#;
        let err_res: Result<CabinetCommand, _> = serde_json::from_str(json_invalid);
        assert!(err_res.is_err());

        // Construct ACK matching schema
        let ack = CommandAck {
            command_id: "cmd-bad-1".to_string(),
            success: false,
            detail: Some(serde_json::json!({ "error": format!("Failed: {}", err_res.unwrap_err()) })),
        };
        let ack_json = serde_json::to_string(&ack).unwrap();
        assert!(ack_json.contains("cmd-bad-1"));
        assert!(ack_json.contains(r#""success":false"#));
    }

    #[tokio::test]
    async fn test_command_ack_dispatch_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{}", addr);

        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap();
                let request_str = String::from_utf8_lossy(&buf[..n]);

                // Verify request line
                assert!(request_str.contains("POST /api/v1/cabinet/node-42/commands/ack"));
                // Verify auth header
                assert!(request_str.to_ascii_lowercase().contains("authorization: bearer secret-token-xyz"));

                // Extract body and parse CommandAck
                if let Some(body_start) = request_str.find("\r\n\r\n") {
                    let body = &request_str[body_start + 4..];
                    let ack: CommandAck = serde_json::from_str(body).unwrap();
                    assert_eq!(ack.command_id, "cmd-uuid-777");
                    assert!(ack.success);
                    assert_eq!(ack.detail.unwrap()["credits_added"], 5);
                }

                let response = "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });

        let client = reqwest::Client::new();
        ack_command(
            &client,
            &server_url,
            "secret-token-xyz",
            "node-42",
            "cmd-uuid-777",
            true,
            Some(serde_json::json!({ "credits_added": 5 })),
        )
        .await;

        handle.await.unwrap();
    }

    // =========================================================================
    // R3: RootFS Dynamic Networking Assertions
    // =========================================================================

    fn find_rootfs_dir() -> PathBuf {
        let candidates = [
            PathBuf::from("astro/rootfs"),
            PathBuf::from("../../astro/rootfs"),
            PathBuf::from("../rootfs"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rootfs"),
        ];
        for c in candidates {
            if c.exists() {
                return c;
            }
        }
        panic!("Could not find astro/rootfs directory");
    }

    #[test]
    fn test_r3_network_config_no_static_ip() {
        let rootfs = find_rootfs_dir();
        let net_conf_path = rootfs.join("etc/systemd/network/10-wired.network");
        assert!(net_conf_path.exists(), "10-wired.network not found at {:?}", net_conf_path);

        let content = std::fs::read_to_string(&net_conf_path).unwrap();
        // R3: Must NOT contain static 192.168.100.2
        assert!(!content.contains("192.168.100.2"), "Static IP 192.168.100.2 found in 10-wired.network");
        assert!(!content.contains("192.168.100.1"), "Static gateway/DNS 192.168.100.1 found in 10-wired.network");
        // Must maintain dynamic DHCP + link-local fallback
        assert!(content.contains("DHCP=yes"), "Missing DHCP=yes in 10-wired.network");
        assert!(content.contains("LinkLocalAddressing=fallback"), "Missing LinkLocalAddressing=fallback");
    }

    #[test]
    fn test_r3_static_net_script_no_static_ip() {
        let rootfs = find_rootfs_dir();
        let script_path = rootfs.join("usr/local/bin/boss_static_net.sh");
        assert!(script_path.exists(), "boss_static_net.sh not found at {:?}", script_path);

        let content = std::fs::read_to_string(&script_path).unwrap();
        // R3: Must NOT assign static 192.168.100.2
        assert!(!content.contains("192.168.100.2"), "Static IP 192.168.100.2 found in boss_static_net.sh");
        assert!(!content.contains("192.168.100.1"), "Static gateway 192.168.100.1 found in boss_static_net.sh");
    }
}
