//! Shared application state: node registry, command queue, event buses,
//! operator session store, plugin registry, and the edge-forwarder buffer.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock, broadcast};

use crate::models::{
    CommandRecord, CommandStatus, ErrorEvent, HighScoreEntry, NodeTelemetry, TokenTransaction,
    GameRef, now_epoch_ms,
};
use crate::plugins::PluginRegistry;

/// SSE channel names understood by the multiplexed `/api/telemetry/stream`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SseChannel {
    Venue,
    Alerts,
    Commands,
    Plugins,
    Ui,
}

impl SseChannel {
    pub const ALL: &'static [SseChannel] = &[
        SseChannel::Venue,
        SseChannel::Alerts,
        SseChannel::Commands,
        SseChannel::Plugins,
        SseChannel::Ui,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            SseChannel::Venue => "venue",
            SseChannel::Alerts => "alerts",
            SseChannel::Commands => "commands",
            SseChannel::Plugins => "plugins",
            SseChannel::Ui => "ui",
        }
    }

    pub fn parse(raw: &str) -> Option<SseChannel> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "venue" => Some(SseChannel::Venue),
            "alerts" => Some(SseChannel::Alerts),
            "commands" => Some(SseChannel::Commands),
            "plugins" => Some(SseChannel::Plugins),
            "ui" => Some(SseChannel::Ui),
            _ => None,
        }
    }
}

/// Buffered edge-sync item. Telemetry is tagged with the venue report time.
#[derive(Debug, Clone)]
pub enum EdgeItem {
    Telemetry(NodeTelemetry),
    Error(ErrorEvent),
    Transaction(TokenTransaction),
    Score(HighScoreEntry),
}

/// B.O.S.S. runtime configuration sourced from the environment.
#[derive(Debug, Clone)]
pub struct Config {
    pub venue_name: String,
    pub venue_address: String,
    /// Bearer token required by operator control-plane / RPC endpoints.
    pub rpc_token: String,
    /// Bearer token accepted by node ingest endpoints (telemetry, scores,
    /// errors, command poll/ack). Falls back to `rpc_token` if
    /// `BOSS_NODE_TOKEN` is not set, preserving single-token deployments.
    pub node_token: String,
    /// 6-digit physical kiosk operator PIN for Dual-Mode touch unlock.
    pub operator_pin: String,
    /// Cloudflare Worker ingest endpoint (over the tunnel), if cloud sync is on.
    pub edge_ingest_url: Option<String>,
    pub edge_ingest_token: String,
}

impl Config {
    pub fn from_env() -> Self {
        let rpc_token = match std::env::var("BOSS_RPC_TOKEN") {
            Ok(t) if !t.trim().is_empty() => t,
            _ => {
                let generated = uuid::Uuid::new_v4().to_string();
                tracing::warn!(
                    token = %generated,
                    "BOSS_RPC_TOKEN not set - generated an ephemeral token for this boot"
                );
                generated
            }
        };

        let node_token = match std::env::var("BOSS_NODE_TOKEN") {
            Ok(t) if !t.trim().is_empty() => t,
            _ => {
                tracing::warn!(
                    "BOSS_NODE_TOKEN not set - falling back to BOSS_RPC_TOKEN for node auth \
                     (set BOSS_NODE_TOKEN to isolate the node ingest credential)"
                );
                rpc_token.clone()
            }
        };

        let operator_pin = match std::env::var("BOSS_OPERATOR_PIN") {
            Ok(p) if !p.trim().is_empty() => p.trim().to_string(),
            _ => "898900".to_string(),
        };

        let edge_ingest_url = std::env::var("EDGE_INGEST_URL")
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty());

        Config {
            venue_name: std::env::var("BOSS_VENUE_NAME")
                .unwrap_or_else(|_| "Beercade Redfern (The BK)".to_string()),
            venue_address: std::env::var("BOSS_VENUE_ADDRESS")
                .unwrap_or_else(|_| "113-115 Regent St, Redfern NSW 2016".to_string()),
            rpc_token,
            node_token,
            operator_pin,
            edge_ingest_url,
            edge_ingest_token: std::env::var("EDGE_INGEST_TOKEN").unwrap_or_default(),
        }
    }
}

/// Router-shared state handle.
pub type SharedState = Arc<AppState>;

/// SSE payload: serialised once, shared by every subscriber without copying.
pub type BusMessage = Arc<str>;

/// Upper bound on items held for the Cloudflare edge batcher.
pub const EDGE_BUFFER_CAP: usize = 4_096;

/// Default TTL for a physical kiosk PIN-minted operator session (15 minutes).
pub const OPERATOR_SESSION_TTL_MS: u64 = 15 * 60 * 1_000;

/// Maximum failed operator PIN attempts permitted before triggering temporary lockout.
pub const PIN_MAX_ATTEMPTS: u32 = 5;

/// Lockout duration in seconds once maximum attempts are reached.
pub const PIN_LOCKOUT_SECS: u64 = 60;

/// Lockout duration in milliseconds.
pub const PIN_LOCKOUT_MS: u64 = PIN_LOCKOUT_SECS * 1_000;

/// Maximum record retention before garbage collection (15 minutes).
pub const PIN_RECORD_TTL_MS: u64 = 15 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinFailureResult {
    Invalid { remaining_attempts: u32 },
    LockedOut { retry_after_secs: u64 },
}

#[derive(Debug, Clone)]
pub struct PinAttemptRecord {
    pub failure_count: u32,
    pub locked_until_epoch_ms: Option<u64>,
    pub last_attempt_epoch_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PinAttemptTracker {
    pub attempts: HashMap<String, PinAttemptRecord>,
}

impl PinAttemptTracker {
    pub fn new() -> Self {
        Self {
            attempts: HashMap::new(),
        }
    }

    /// Check if a client is locked out at a given timestamp.
    /// Returns Some(retry_after_secs) if locked out, or None if attempts are allowed.
    /// If an existing lockout has expired, resets the record.
    pub fn check_lockout_at(&mut self, client_id: &str, now: u64) -> Option<u64> {
        if let Some(record) = self.attempts.get_mut(client_id) {
            if let Some(locked_until) = record.locked_until_epoch_ms {
                if now < locked_until {
                    let diff_ms = locked_until - now;
                    let retry_secs = (diff_ms + 999) / 1000;
                    return Some(retry_secs.max(1));
                } else {
                    // Lockout period has elapsed; clear lockout and reset count
                    record.locked_until_epoch_ms = None;
                    record.failure_count = 0;
                }
            }
        }
        None
    }

    pub fn check_lockout(&mut self, client_id: &str) -> Option<u64> {
        self.check_lockout_at(client_id, now_epoch_ms())
    }

    /// Record a failed PIN attempt at a given timestamp.
    pub fn record_failure_at(&mut self, client_id: &str, now: u64) -> PinFailureResult {
        if self.attempts.len() > 4096 {
            self.clean_expired(now);
        }

        let record = self
            .attempts
            .entry(client_id.to_string())
            .or_insert_with(|| PinAttemptRecord {
                failure_count: 0,
                locked_until_epoch_ms: None,
                last_attempt_epoch_ms: now,
            });

        // If previously locked out and lockout expired, reset before incrementing
        if let Some(until) = record.locked_until_epoch_ms {
            if now >= until {
                record.locked_until_epoch_ms = None;
                record.failure_count = 0;
            }
        }

        record.last_attempt_epoch_ms = now;
        record.failure_count += 1;

        if record.failure_count >= PIN_MAX_ATTEMPTS {
            record.failure_count = PIN_MAX_ATTEMPTS;
            let locked_until = now + PIN_LOCKOUT_MS;
            record.locked_until_epoch_ms = Some(locked_until);
            PinFailureResult::LockedOut {
                retry_after_secs: PIN_LOCKOUT_SECS,
            }
        } else {
            let remaining = PIN_MAX_ATTEMPTS - record.failure_count;
            PinFailureResult::Invalid {
                remaining_attempts: remaining,
            }
        }
    }

    pub fn record_failure(&mut self, client_id: &str) -> PinFailureResult {
        self.record_failure_at(client_id, now_epoch_ms())
    }

    /// Reset failure history for a client after successful PIN authentication.
    pub fn record_success(&mut self, client_id: &str) {
        self.attempts.remove(client_id);
    }

    /// Clean expired lockout and stale attempt records.
    pub fn clean_expired(&mut self, now: u64) {
        self.attempts.retain(|_, record| {
            if let Some(until) = record.locked_until_epoch_ms {
                until > now
            } else {
                now.saturating_sub(record.last_attempt_epoch_ms) < PIN_RECORD_TTL_MS
            }
        });
    }

    pub fn clean_expired_now(&mut self) {
        self.clean_expired(now_epoch_ms());
    }
}

/// Active player session assigned to a specific cabinet node.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActiveNodeSession {
    pub session_id: String,
    pub session_code: String,
    pub duration_secs: u32,
    pub topup_grace_secs: u32,
    pub games: Vec<GameRef>,
    pub expires_at_epoch_ms: u64,
}

/// Cabinet session, attract playlist, and attract history tracked per node.
#[derive(Debug, Clone, Default)]
pub struct NodeCabinetState {
    pub session: Option<ActiveNodeSession>,
    pub attract_playlist: Vec<GameRef>,
    pub recent_attract_game_ids: VecDeque<String>,
}

pub struct AppState {
    pub config: Config,
    /// node_id -> (latest heartbeat, last_seen epoch ms)
    pub nodes: RwLock<HashMap<String, (NodeTelemetry, u64)>>,
    /// All commands ever issued this boot, by id.
    pub commands: RwLock<HashMap<String, CommandRecord>>,
    /// Pending (not yet polled) command ids per node, FIFO order.
    pub pending_commands: RwLock<HashMap<String, VecDeque<String>>>,
    /// Broadcast bus per SSE channel. Capacity sized for bursty venues.
    pub buses: HashMap<SseChannel, broadcast::Sender<BusMessage>>,
    /// Buffer feeding the Cloudflare edge batcher.
    pub edge_buffer: Mutex<VecDeque<EdgeItem>>,
    /// Short-lived operator session tokens minted via `POST /api/v1/auth/pin` -> expires_at_ms.
    pub operator_sessions: RwLock<HashMap<String, u64>>,
    /// Rate limiter and lockout tracker for physical kiosk operator PIN attempts.
    pub pin_tracker: Mutex<PinAttemptTracker>,
    /// Rolling history of recent cabinet error events for the operator console.
    pub recent_errors: RwLock<VecDeque<ErrorEvent>>,
    /// Rolling history of recent high scores for the operator console & kiosk leaderboard.
    pub recent_scores: RwLock<VecDeque<HighScoreEntry>>,
    /// Rolling history of recent credit transactions.
    pub recent_transactions: RwLock<VecDeque<TokenTransaction>>,
    /// Hybrid Event-Hook + UI Panel Plugin Registry.
    pub plugins: Arc<PluginRegistry>,
    /// Venue-wide UI theme id (see `ui_theme`).
    pub ui_theme: RwLock<String>,
    pub catalogue: RwLock<crate::catalogue::GameCatalogue>,
    pub node_cabinets: RwLock<HashMap<String, NodeCabinetState>>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let mut buses = HashMap::new();
        for ch in SseChannel::ALL {
            let (tx, _) = broadcast::channel(if *ch == SseChannel::Venue { 256 } else { 128 });
            buses.insert(*ch, tx);
        }

        Self {
            config,
            nodes: RwLock::new(HashMap::new()),
            commands: RwLock::new(HashMap::new()),
            pending_commands: RwLock::new(HashMap::new()),
            buses,
            edge_buffer: Mutex::new(VecDeque::new()),
            operator_sessions: RwLock::new(HashMap::new()),
            pin_tracker: Mutex::new(PinAttemptTracker::new()),
            recent_errors: RwLock::new(VecDeque::new()),
            recent_scores: RwLock::new(VecDeque::new()),
            recent_transactions: RwLock::new(VecDeque::new()),
            plugins: Arc::new(PluginRegistry::new_with_builtins()),
            ui_theme: RwLock::new(crate::ui_theme::load_initial()),
            catalogue: RwLock::new(crate::catalogue::GameCatalogue::new()),
            node_cabinets: RwLock::new(HashMap::new()),
        }
    }

    pub fn publish(&self, channel: SseChannel, payload: &impl serde::Serialize) {
        if let Ok(json) = serde_json::to_string(payload) {
            let _ = self.buses[&channel].send(Arc::from(json));
        }
    }

    /// Get the current active session for a cabinet node.
    pub async fn get_node_session(&self, node_id: &str) -> Option<ActiveNodeSession> {
        let cabs = self.node_cabinets.read().await;
        cabs.get(node_id).and_then(|c| c.session.clone())
    }

    /// Set an active session for a cabinet node.
    pub async fn set_node_session(&self, node_id: &str, session: ActiveNodeSession) {
        let mut cabs = self.node_cabinets.write().await;
        let entry = cabs.entry(node_id.to_string()).or_default();
        entry.session = Some(session);
    }

    /// Clear and return the active session for a cabinet node.
    pub async fn clear_node_session(&self, node_id: &str) -> Option<ActiveNodeSession> {
        let mut cabs = self.node_cabinets.write().await;
        cabs.get_mut(node_id).and_then(|c| c.session.take())
    }

    /// Get current attract playlist for a cabinet node.
    pub async fn get_node_attract_playlist(&self, node_id: &str) -> Vec<GameRef> {
        let cabs = self.node_cabinets.read().await;
        cabs.get(node_id)
            .map(|c| c.attract_playlist.clone())
            .unwrap_or_default()
    }

    /// Set attract playlist for a cabinet node and record into rolling history.
    pub async fn set_node_attract_playlist(&self, node_id: &str, playlist: Vec<GameRef>) {
        let mut cabs = self.node_cabinets.write().await;
        let entry = cabs.entry(node_id.to_string()).or_default();
        for game in &playlist {
            if !entry.recent_attract_game_ids.contains(&game.game_id) {
                entry.recent_attract_game_ids.push_back(game.game_id.clone());
            }
        }
        while entry.recent_attract_game_ids.len() > 10 {
            entry.recent_attract_game_ids.pop_front();
        }
        entry.attract_playlist = playlist;
    }

    /// Retrieve the recent attract game IDs for a cabinet node.
    pub async fn get_node_recent_attract_ids(&self, node_id: &str) -> Vec<String> {
        let cabs = self.node_cabinets.read().await;
        cabs.get(node_id)
            .map(|c| c.recent_attract_game_ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if a game_id is authorized for file download by a node (in active session or attract playlist).
    pub async fn is_game_authorized_for_node(&self, node_id: &str, game_id: &str) -> bool {
        let cabs = self.node_cabinets.read().await;
        if let Some(c) = cabs.get(node_id) {
            if let Some(sess) = &c.session {
                if sess.games.iter().any(|g| g.game_id == game_id) {
                    return true;
                }
            }
            if c.attract_playlist.iter().any(|g| g.game_id == game_id) {
                return true;
            }
        }
        false
    }

    /// Mint a short-lived operator session token after PIN verification.
    pub async fn mint_operator_session(&self) -> (String, u64) {
        let now = now_epoch_ms();
        let expires_at = now + OPERATOR_SESSION_TTL_MS;
        let token = format!("boss_sess_{}", uuid::Uuid::new_v4());
        let mut sessions = self.operator_sessions.write().await;
        sessions.retain(|_, exp| *exp > now);
        sessions.insert(token.clone(), expires_at);
        (token, expires_at)
    }

    /// Check whether `presented` is either `BOSS_RPC_TOKEN` or an active operator session.
    pub async fn is_valid_operator_token(&self, presented: &str) -> bool {
        if presented.is_empty() {
            return false;
        }
        if crate::auth::constant_time_eq(presented.as_bytes(), self.config.rpc_token.as_bytes()) {
            return true;
        }
        let now = now_epoch_ms();
        let sessions = self.operator_sessions.read().await;
        if let Some(&expires_at) = sessions.get(presented) {
            return expires_at > now;
        }
        false
    }

    /// Record an error event into the rolling in-memory audit log.
    pub async fn record_error(&self, event: ErrorEvent) {
        let mut list = self.recent_errors.write().await;
        if list.len() >= 100 {
            list.pop_back();
        }
        list.push_front(event);
    }

    /// Record a high score submission into the rolling in-memory audit log.
    pub async fn record_score(&self, score: HighScoreEntry) {
        let mut list = self.recent_scores.write().await;
        if list.len() >= 100 {
            list.pop_back();
        }
        list.push_front(score);
    }

    /// Record a token/credit transaction into the rolling in-memory audit log.
    pub async fn record_transaction(&self, tx: TokenTransaction) {
        let mut list = self.recent_transactions.write().await;
        if list.len() >= 100 {
            list.pop_back();
        }
        list.push_front(tx);
    }

    /// Queue a command for a node, track it, and notify the commands bus.
    pub async fn enqueue_command(
        &self,
        node_id: &str,
        command: crate::models::CabinetCommand,
    ) -> CommandRecord {
        let record = CommandRecord {
            id: uuid::Uuid::new_v4().to_string(),
            node_id: node_id.to_string(),
            command,
            status: CommandStatus::Queued,
            issued_at_epoch_ms: now_epoch_ms(),
            dispatched_at_epoch_ms: None,
            completed_at_epoch_ms: None,
            result: None,
        };

        self.pending_commands
            .write()
            .await
            .entry(node_id.to_string())
            .or_default()
            .push_back(record.id.clone());
        self.commands
            .write()
            .await
            .insert(record.id.clone(), record.clone());
        self.publish(SseChannel::Commands, &record);
        record
    }

    /// Nodes seen within the staleness window.
    /// Whether any SSE client is listening on `channel`.
    pub fn has_subscribers(&self, channel: SseChannel) -> bool {
        self.buses[&channel].receiver_count() > 0
    }

    /// Ids of nodes heard from within `staleness_ms`, without cloning telemetry.
    pub async fn online_node_ids(&self, staleness_ms: u64) -> std::collections::HashSet<String> {
        let now = now_epoch_ms();
        self.nodes
            .read()
            .await
            .iter()
            .filter(|(_, (_, last_seen))| now.saturating_sub(*last_seen) < staleness_ms)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub async fn online_nodes(&self, staleness_ms: u64) -> Vec<NodeTelemetry> {
        let now = now_epoch_ms();
        self.nodes
            .read()
            .await
            .values()
            .filter(|(_, last_seen)| now.saturating_sub(*last_seen) < staleness_ms)
            .map(|(t, _)| t.clone())
            .collect()
    }

    pub async fn build_venue_report(
        &self,
        staleness_ms: u64,
    ) -> crate::models::VenueTelemetryReport {
        let node_list = self.online_nodes(staleness_ms).await;
        crate::models::VenueTelemetryReport {
            venue: self.config.venue_name.clone(),
            address: self.config.venue_address.clone(),
            status: "OPEN".to_string(),
            nodes_online: node_list.len(),
            nodes: node_list,
            generated_at_epoch_ms: now_epoch_ms(),
        }
    }

    /// Buffer an item for the Cloudflare edge batcher (bounded to protect RAM).
    /// No-op in local-only mode, where nothing would ever drain the buffer.
    pub async fn buffer_edge_item(&self, item: EdgeItem) {
        if !self.edge_sync_enabled() {
            return;
        }
        let mut buf = self.edge_buffer.lock().await;
        if buf.len() >= EDGE_BUFFER_CAP {
            buf.pop_front();
        }
        buf.push_back(item);
    }

    /// Same conditions under which `edge::run_edge_forwarder` keeps running.
    pub fn edge_sync_enabled(&self) -> bool {
        self.config.edge_ingest_url.is_some() && !self.config.edge_ingest_token.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_tracker_failure_counting() {
        let mut tracker = PinAttemptTracker::new();
        let client = "192.168.1.50";
        let now = 100_000;

        assert_eq!(tracker.check_lockout_at(client, now), None);

        assert_eq!(
            tracker.record_failure_at(client, now),
            PinFailureResult::Invalid { remaining_attempts: 4 }
        );
        assert_eq!(
            tracker.record_failure_at(client, now + 1_000),
            PinFailureResult::Invalid { remaining_attempts: 3 }
        );
        assert_eq!(
            tracker.record_failure_at(client, now + 2_000),
            PinFailureResult::Invalid { remaining_attempts: 2 }
        );
        assert_eq!(
            tracker.record_failure_at(client, now + 3_000),
            PinFailureResult::Invalid { remaining_attempts: 1 }
        );

        assert_eq!(tracker.check_lockout_at(client, now + 4_000), None);
    }

    #[test]
    fn test_pin_tracker_lockout_trigger() {
        let mut tracker = PinAttemptTracker::new();
        let client = "10.0.0.1";
        let now = 200_000;

        for _ in 0..4 {
            let res = tracker.record_failure_at(client, now);
            assert!(matches!(res, PinFailureResult::Invalid { .. }));
        }

        // 5th attempt triggers 60s lockout
        let res = tracker.record_failure_at(client, now);
        assert_eq!(
            res,
            PinFailureResult::LockedOut {
                retry_after_secs: 60
            }
        );

        // Immediate check confirms lockout with 60 seconds
        let retry = tracker.check_lockout_at(client, now);
        assert_eq!(retry, Some(60));

        // 10 seconds later, 50 seconds remaining
        let retry_later = tracker.check_lockout_at(client, now + 10_000);
        assert_eq!(retry_later, Some(50));
    }

    #[test]
    fn test_pin_tracker_success_resets_count() {
        let mut tracker = PinAttemptTracker::new();
        let client = "172.16.0.4";
        let now = 300_000;

        tracker.record_failure_at(client, now);
        tracker.record_failure_at(client, now + 500);
        tracker.record_failure_at(client, now + 1_000);

        // Success resets failure count
        tracker.record_success(client);

        // Next failure starts fresh
        let res = tracker.record_failure_at(client, now + 2_000);
        assert_eq!(
            res,
            PinFailureResult::Invalid { remaining_attempts: 4 }
        );
    }

    #[test]
    fn test_pin_tracker_lockout_expiry() {
        let mut tracker = PinAttemptTracker::new();
        let client = "192.168.100.10";
        let now = 500_000;

        for _ in 0..5 {
            tracker.record_failure_at(client, now);
        }

        assert_eq!(tracker.check_lockout_at(client, now), Some(60));

        // After 60 seconds and 1 millisecond, lockout expires
        let expired_time = now + 60_001;
        assert_eq!(tracker.check_lockout_at(client, expired_time), None);

        // New failure after expiry starts fresh count at 1 (4 remaining)
        let res = tracker.record_failure_at(client, expired_time + 100);
        assert_eq!(
            res,
            PinFailureResult::Invalid { remaining_attempts: 4 }
        );
    }

    #[test]
    fn test_pin_tracker_clean_expired() {
        let mut tracker = PinAttemptTracker::new();
        let stale_client = "10.0.0.99";
        let locked_client = "10.0.0.100";
        let now = 2_000_000;

        // Record a failure 20 minutes ago (stale, beyond 15m TTL)
        tracker.record_failure_at(stale_client, now - (20 * 60 * 1_000));

        // Lock out another client at `now` (active lockout for 60s)
        for _ in 0..5 {
            tracker.record_failure_at(locked_client, now);
        }

        assert_eq!(tracker.attempts.len(), 2);
        tracker.clean_expired(now);

        // Stale client was pruned, active locked client remains
        assert!(!tracker.attempts.contains_key(stale_client));
        assert!(tracker.attempts.contains_key(locked_client));

        // Test clean_expired_now executes without error
        tracker.clean_expired_now();
    }

    #[test]
    fn test_pin_tracker_multi_client_isolation() {
        let mut tracker = PinAttemptTracker::new();
        let attacker = "198.51.100.1";
        let legitimate = "198.51.100.2";
        let now = 700_000;

        // Attacker gets locked out after 5 failures
        for _ in 0..5 {
            tracker.record_failure_at(attacker, now);
        }
        assert_eq!(tracker.check_lockout_at(attacker, now), Some(60));

        // Legitimate client is completely unaffected
        assert_eq!(tracker.check_lockout_at(legitimate, now), None);
        let res = tracker.record_failure_at(legitimate, now);
        assert_eq!(
            res,
            PinFailureResult::Invalid { remaining_attempts: 4 }
        );
    }
}
