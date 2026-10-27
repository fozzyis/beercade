//! Canonical data models shared across B.O.S.S., A.S.T.R.O. and Website.

use serde::{Deserialize, Serialize};

#[cfg(feature = "typeshare")]
use typeshare::typeshare;

/// Station execution mode reported by an A.S.T.R.O. / O.R.I.O.N. node.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "payload")]
#[cfg_attr(feature = "typeshare", typeshare)]
pub enum StationMode {
    Standalone,
    Managed {
        session_id: String,
        time_remaining_sec: u32,
    },
    Locked {
        reason: String,
    },
}

impl Default for StationMode {
    fn default() -> Self {
        StationMode::Standalone
    }
}

/// Jitter statistics for the dedicated 1000Hz evdev polling thread.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct InputJitterStats {
    #[serde(default)]
    pub poll_rate_hz: f32,
    #[serde(default)]
    pub p50_us: f32,
    #[serde(default)]
    pub p95_us: f32,
    #[serde(default)]
    pub p99_us: f32,
    #[serde(default)]
    pub max_us: f32,
    #[serde(default)]
    pub dropped_events: u32,
}

/// Live P2P netplay session statistics (rollback netcode).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct NetplayStats {
    pub peer_id: String,
    pub rtt_ms: f32,
    pub jitter_ms: f32,
    pub packet_loss_pct: f32,
    pub rollback_frames: u32,
    pub is_host: bool,
}

/// Credit hardware counters mirrored from the cabinet's coin/credit subsystem.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct CreditCounters {
    #[serde(default)]
    pub credits_available: u32,
    #[serde(default)]
    pub credits_inserted_total: u64,
    #[serde(default)]
    pub credits_dispensed_today: u32,
    #[serde(default)]
    pub last_coin_epoch_ms: Option<u64>,
}

/// Telemetry reported by a single ASTRO cabinet node every heartbeat.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct NodeTelemetry {
    pub node_id: String,
    pub hardware: String,
    pub soc: String,
    pub cpu_temp_c: f32,
    pub gpu_temp_c: f32,
    #[serde(default)]
    pub npu_load_pct: f32,
    pub active_core: String,
    pub active_rom: String,
    pub frame_time_ms: f32,
    #[serde(default)]
    pub fps: f32,
    #[serde(default)]
    pub input_jitter: InputJitterStats,
    #[serde(default)]
    pub netplay: Option<NetplayStats>,
    #[serde(default)]
    pub credits: CreditCounters,
    #[serde(default)]
    pub mode: serde_json::Value,
    pub timestamp_epoch_ms: u64,
}

impl NodeTelemetry {
    /// Cheap sanity validation applied before a heartbeat is accepted.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.node_id.trim().is_empty() {
            return Err("node_id must not be empty");
        }
        if self.node_id.len() > 64 {
            return Err("node_id too long (max 64 chars)");
        }
        if !(-20.0..=120.0).contains(&self.cpu_temp_c) {
            return Err("cpu_temp_c out of plausible range");
        }
        if !(-20.0..=120.0).contains(&self.gpu_temp_c) {
            return Err("gpu_temp_c out of plausible range");
        }
        if !(0.0..=100.0).contains(&self.npu_load_pct) {
            return Err("npu_load_pct must be within 0..=100");
        }
        if self.frame_time_ms < 0.0 || self.frame_time_ms > 10_000.0 {
            return Err("frame_time_ms out of plausible range");
        }
        if let Some(np) = &self.netplay {
            if np.rtt_ms < 0.0 || np.packet_loss_pct < 0.0 || np.packet_loss_pct > 100.0 {
                return Err("netplay stats out of plausible range");
            }
        }
        Ok(())
    }
}

/// A single venue's aggregated telemetry snapshot.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct VenueTelemetryReport {
    pub venue: String,
    pub address: String,
    pub status: String,
    pub nodes_online: usize,
    pub nodes: Vec<NodeTelemetry>,
    pub generated_at_epoch_ms: u64,
}

/// Operator- or cloud-initiated command destined for a cabinet node.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "typeshare", typeshare)]
pub enum CabinetCommand {
    Reboot {
        delay_secs: u32,
    },
    LaunchGame {
        core: String,
        rom: String,
        netplay: bool,
    },
    AddCredits {
        credits: u32,
    },
    Ping {
        seq: u64,
    },
    StartSession {
        session_id: String,
        duration_secs: u32,
    },
    EndSession,
}

/// Lifecycle state of a dispatched command.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typeshare", typeshare)]
pub enum CommandStatus {
    Queued,
    Dispatched,
    Acknowledged,
    Failed,
}

/// A command tracked from issuance to acknowledgement.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct CommandRecord {
    pub id: String,
    pub node_id: String,
    pub command: CabinetCommand,
    pub status: CommandStatus,
    pub issued_at_epoch_ms: u64,
    #[serde(default)]
    pub dispatched_at_epoch_ms: Option<u64>,
    #[serde(default)]
    pub completed_at_epoch_ms: Option<u64>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
}

/// Node-poll payload acknowledging a dispatched command.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct CommandAck {
    pub command_id: String,
    pub success: bool,
    #[serde(default)]
    pub detail: Option<serde_json::Value>,
}

/// Structured error reported by a node.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct ErrorEvent {
    pub node_id: String,
    pub level: String,
    pub source: String,
    pub message: String,
    #[serde(default)]
    pub detail: Option<serde_json::Value>,
    #[serde(default)]
    pub occurred_at_epoch_ms: u64,
}

/// A credit/dispense transaction.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct TokenTransaction {
    pub node_id: String,
    pub kind: String,
    pub credits: u32,
    pub balance_after: u32,
    #[serde(default)]
    pub command_id: Option<String>,
    pub created_at_epoch_ms: u64,
}

/// A score submission from a node.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct HighScoreEntry {
    pub node_id: String,
    pub active_core: String,
    pub active_rom: String,
    pub game_name: String,
    pub score: u64,
    #[serde(default)]
    pub initials: String,
    #[serde(default)]
    pub achieved_at_epoch_ms: u64,
}

/// Envelope BOSS pushes to the Cloudflare edge worker once per flush window.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct EdgeBatchEnvelope {
    pub batch_id: String,
    pub venue: String,
    pub generated_at_epoch_ms: u64,
    pub nodes: Vec<NodeTelemetry>,
    pub errors: Vec<ErrorEvent>,
    pub transactions: Vec<TokenTransaction>,
    pub scores: Vec<HighScoreEntry>,
}

/// Helper returning the current Unix timestamp in milliseconds.
pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
