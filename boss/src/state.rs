//! Shared application state: node registry, command queue, event buses and
//! the edge-forwarder buffer.

use std::collections::{HashMap, VecDeque};

use tokio::sync::{Mutex, RwLock, broadcast};

use crate::models::{
    CommandRecord, CommandStatus, ErrorEvent, HighScoreEntry, NodeTelemetry, TokenTransaction,
    now_epoch_ms,
};

/// SSE channel names understood by the multiplexed `/api/telemetry/stream`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SseChannel {
    Venue,
    Alerts,
    Commands,
}

impl SseChannel {
    pub const ALL: &'static [SseChannel] =
        &[SseChannel::Venue, SseChannel::Alerts, SseChannel::Commands];

    pub fn as_str(&self) -> &'static str {
        match self {
            SseChannel::Venue => "venue",
            SseChannel::Alerts => "alerts",
            SseChannel::Commands => "commands",
        }
    }

    pub fn parse(raw: &str) -> Option<SseChannel> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "venue" => Some(SseChannel::Venue),
            "alerts" => Some(SseChannel::Alerts),
            "commands" => Some(SseChannel::Commands),
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
            edge_ingest_url,
            edge_ingest_token: std::env::var("EDGE_INGEST_TOKEN").unwrap_or_default(),
        }
    }
}

/// Router-shared state handle.
pub type SharedState = std::sync::Arc<AppState>;

pub struct AppState {
    pub config: Config,
    /// node_id -> (latest heartbeat, last_seen epoch ms)
    pub nodes: RwLock<HashMap<String, (NodeTelemetry, u64)>>,
    /// All commands ever issued this boot, by id.
    pub commands: RwLock<HashMap<String, CommandRecord>>,
    /// Pending (not yet polled) command ids per node, FIFO order.
    pub pending_commands: RwLock<HashMap<String, VecDeque<String>>>,
    /// Broadcast bus per SSE channel. Capacity sized for bursty venues.
    pub buses: HashMap<SseChannel, broadcast::Sender<String>>,
    /// Buffer feeding the Cloudflare edge batcher.
    pub edge_buffer: Mutex<Vec<EdgeItem>>,
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
            edge_buffer: Mutex::new(Vec::new()),
        }
    }

    pub fn publish(&self, channel: SseChannel, payload: &impl serde::Serialize) {
        if let Ok(json) = serde_json::to_string(payload) {
            let _ = self.buses[&channel].send(json);
        }
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
    pub async fn buffer_edge_item(&self, item: EdgeItem) {
        let mut buf = self.edge_buffer.lock().await;
        if buf.len() >= 4_096 {
            buf.remove(0);
        }
        buf.push(item);
    }
}
