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
    #[serde(default)]
    pub cpu_temp_c: Option<f32>,
    #[serde(default)]
    pub gpu_temp_c: Option<f32>,
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
        if let Some(temp) = self.cpu_temp_c {
            if temp.is_nan() || !(-20.0..=120.0).contains(&temp) {
                return Err("cpu_temp_c out of plausible range");
            }
        }
        if let Some(temp) = self.gpu_temp_c {
            if temp.is_nan() || !(-20.0..=120.0).contains(&temp) {
                return Err("gpu_temp_c out of plausible range");
            }
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
        /// What the player types at B.O.S.S. to change games or buy time.
        #[serde(default)]
        session_code: String,
        /// How long the cabinet keeps the session's files after time runs
        /// out, waiting for a top-up.
        #[serde(default)]
        topup_grace_secs: u32,
        /// The player's games; fetched from B.O.S.S. and wiped at the end.
        #[serde(default)]
        games: Vec<GameRef>,
    },
    /// Change a live session: replace its games and/or add time.
    UpdateSession {
        session_id: String,
        /// Full replacement list when present.
        #[serde(default)]
        games: Option<Vec<GameRef>>,
        #[serde(default)]
        add_secs: u32,
    },
    /// End a session now. Older B.O.S.S. builds send it without fields.
    EndSession {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        reason: Option<String>,
    },
    /// Idle cabinet: cycle these games until a session starts.
    StartAttract {
        games: Vec<GameRef>,
        secs_per_game: u32,
    },
}

/// A game B.O.S.S. sends to a cabinet for a session or attract playlist.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "typeshare", typeshare)]
pub struct GameRef {
    /// Stable catalogue id; also the file endpoint's path segment.
    pub game_id: String,
    pub title: String,
    /// "snes", "megadrive", "arcade", "psx", "pc", ...
    pub system: String,
    /// libretro core name, e.g. "snes9x".
    pub core: String,
    /// Name the cabinet stores the file under, e.g. "Donkey Kong Country.zip".
    pub file_name: String,
    pub size_bytes: u64,
    /// Lower-case hex SHA-256; the cabinet refuses a file that doesn't match.
    pub sha256: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
}

/// Cabinet → B.O.S.S. session and attract events
/// (`POST /api/v1/cabinet/:node_id/events`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "typeshare", typeshare)]
pub enum CabinetEvent {
    SessionStaged {
        session_id: String,
        ready_game_ids: Vec<String>,
        failed_game_ids: Vec<String>,
    },
    GameStarted {
        session_id: String,
        game_id: String,
    },
    GameExited {
        session_id: String,
        game_id: String,
        crashed: bool,
    },
    /// Time ran out; the top-up grace window has started.
    SessionExpired {
        session_id: String,
    },
    SessionEnded {
        session_id: String,
        files_wiped: bool,
    },
    /// The player chose "Change my games" on the cabinet.
    ChangeRequested {
        session_id: String,
    },
    /// Idle with no attract playlist.
    AttractNeeded,
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

/// Multiplexed server-to-cabinet push message over `/api/v1/ws/telemetry`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "typeshare", typeshare)]
pub enum WsServerMessage {
    Command {
        record: CommandRecord,
    },
    ThemeChanged {
        theme: String,
    },
    PayloadAvailable {
        filename: String,
        #[serde(default)]
        sha256: Option<String>,
        #[serde(default)]
        size_bytes: Option<u64>,
    },
}

/// Helper returning the current Unix timestamp in milliseconds.
pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_telemetry() -> NodeTelemetry {
        NodeTelemetry {
            node_id: "CAB-01".to_string(),
            hardware: "x86_64".to_string(),
            soc: "generic".to_string(),
            cpu_temp_c: None,
            gpu_temp_c: None,
            npu_load_pct: 0.0,
            active_core: "fbneo".to_string(),
            active_rom: "sfiii3.zip".to_string(),
            frame_time_ms: 16.6,
            fps: 60.0,
            input_jitter: InputJitterStats::default(),
            netplay: None,
            credits: CreditCounters::default(),
            mode: serde_json::Value::Null,
            timestamp_epoch_ms: 1700000000000,
        }
    }

    #[test]
    fn test_telemetry_validate_with_none_thermals() {
        let mut t = dummy_telemetry();
        t.cpu_temp_c = None;
        t.gpu_temp_c = None;
        assert_eq!(t.validate(), Ok(()));
    }

    #[test]
    fn test_telemetry_validate_with_valid_thermals() {
        let mut t = dummy_telemetry();
        t.cpu_temp_c = Some(55.5);
        t.gpu_temp_c = Some(62.0);
        assert_eq!(t.validate(), Ok(()));

        // Test boundary values (-20.0 and 120.0 inclusive)
        t.cpu_temp_c = Some(-20.0);
        t.gpu_temp_c = Some(120.0);
        assert_eq!(t.validate(), Ok(()));
    }

    #[test]
    fn test_telemetry_validate_with_out_of_range_thermals() {
        let mut t = dummy_telemetry();

        // CPU too high
        t.cpu_temp_c = Some(120.1);
        t.gpu_temp_c = None;
        assert_eq!(t.validate(), Err("cpu_temp_c out of plausible range"));

        // CPU too low
        t.cpu_temp_c = Some(-20.1);
        t.gpu_temp_c = None;
        assert_eq!(t.validate(), Err("cpu_temp_c out of plausible range"));

        // CPU NaN
        t.cpu_temp_c = Some(f32::NAN);
        t.gpu_temp_c = None;
        assert_eq!(t.validate(), Err("cpu_temp_c out of plausible range"));

        // GPU too high
        t.cpu_temp_c = Some(50.0);
        t.gpu_temp_c = Some(150.0);
        assert_eq!(t.validate(), Err("gpu_temp_c out of plausible range"));

        // GPU too low
        t.cpu_temp_c = Some(50.0);
        t.gpu_temp_c = Some(-30.0);
        assert_eq!(t.validate(), Err("gpu_temp_c out of plausible range"));

        // GPU NaN
        t.cpu_temp_c = Some(50.0);
        t.gpu_temp_c = Some(f32::NAN);
        assert_eq!(t.validate(), Err("gpu_temp_c out of plausible range"));
    }

    #[test]
    fn test_telemetry_serde_deserialization() {
        // Explicit numeric values
        let json_explicit = r#"{
            "node_id": "CAB-01",
            "hardware": "x86_64",
            "soc": "intel",
            "cpu_temp_c": 54.5,
            "gpu_temp_c": 60.2,
            "active_core": "mame",
            "active_rom": "pacman",
            "frame_time_ms": 16.6,
            "timestamp_epoch_ms": 1700000000
        }"#;
        let t1: NodeTelemetry = serde_json::from_str(json_explicit).expect("deserialize explicit");
        assert_eq!(t1.cpu_temp_c, Some(54.5));
        assert_eq!(t1.gpu_temp_c, Some(60.2));
        assert_eq!(t1.validate(), Ok(()));

        // Explicit null values
        let json_null = r#"{
            "node_id": "CAB-01",
            "hardware": "x86_64",
            "soc": "intel",
            "cpu_temp_c": null,
            "gpu_temp_c": null,
            "active_core": "mame",
            "active_rom": "pacman",
            "frame_time_ms": 16.6,
            "timestamp_epoch_ms": 1700000000
        }"#;
        let t2: NodeTelemetry = serde_json::from_str(json_null).expect("deserialize null");
        assert_eq!(t2.cpu_temp_c, None);
        assert_eq!(t2.gpu_temp_c, None);
        assert_eq!(t2.validate(), Ok(()));

        // Omitted fields (using #[serde(default)])
        let json_omitted = r#"{
            "node_id": "CAB-01",
            "hardware": "x86_64",
            "soc": "intel",
            "active_core": "mame",
            "active_rom": "pacman",
            "frame_time_ms": 16.6,
            "timestamp_epoch_ms": 1700000000
        }"#;
        let t3: NodeTelemetry = serde_json::from_str(json_omitted).expect("deserialize omitted");
        assert_eq!(t3.cpu_temp_c, None);
        assert_eq!(t3.gpu_temp_c, None);
        assert_eq!(t3.validate(), Ok(()));

        // Partial: cpu present, gpu omitted or null
        let json_partial = r#"{
            "node_id": "CAB-01",
            "hardware": "x86_64",
            "soc": "intel",
            "cpu_temp_c": 48.0,
            "active_core": "mame",
            "active_rom": "pacman",
            "frame_time_ms": 16.6,
            "timestamp_epoch_ms": 1700000000
        }"#;
        let t4: NodeTelemetry = serde_json::from_str(json_partial).expect("deserialize partial");
        assert_eq!(t4.cpu_temp_c, Some(48.0));
        assert_eq!(t4.gpu_temp_c, None);
        assert_eq!(t4.validate(), Ok(()));
    }

    fn game(id: &str) -> GameRef {
        GameRef {
            game_id: id.to_string(),
            title: "Donkey Kong Country".to_string(),
            system: "snes".to_string(),
            core: "snes9x".to_string(),
            file_name: "Donkey Kong Country.zip".to_string(),
            size_bytes: 2_514_751,
            sha256: "ab".repeat(32),
            artwork_url: None,
        }
    }

    #[test]
    fn start_session_round_trips_with_games() {
        let cmd = CabinetCommand::StartSession {
            session_id: "sess_1".into(),
            duration_secs: 1800,
            session_code: "BRC-4KQ7".into(),
            topup_grace_secs: 90,
            games: vec![game("dkc")],
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"start_session\""));
        let back: CabinetCommand = serde_json::from_str(&json).unwrap();
        match back {
            CabinetCommand::StartSession { games, session_code, .. } => {
                assert_eq!(games, vec![game("dkc")]);
                assert_eq!(session_code, "BRC-4KQ7");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn old_session_json_still_parses() {
        let start: CabinetCommand =
            serde_json::from_str(r#"{"type":"start_session","session_id":"s","duration_secs":60}"#).unwrap();
        match start {
            CabinetCommand::StartSession { games, topup_grace_secs, .. } => {
                assert!(games.is_empty());
                assert_eq!(topup_grace_secs, 0);
            }
            other => panic!("wrong variant: {other:?}"),
        }
        let end: CabinetCommand = serde_json::from_str(r#"{"type":"end_session"}"#).unwrap();
        assert!(matches!(end, CabinetCommand::EndSession { session_id: None, reason: None }));
    }

    #[test]
    fn update_session_and_attract_parse() {
        let upd: CabinetCommand =
            serde_json::from_str(r#"{"type":"update_session","session_id":"s","add_secs":900}"#).unwrap();
        assert!(matches!(upd, CabinetCommand::UpdateSession { games: None, add_secs: 900, .. }));
        let attract = CabinetCommand::StartAttract { games: vec![game("a"), game("b")], secs_per_game: 90 };
        let back: CabinetCommand = serde_json::from_str(&serde_json::to_string(&attract).unwrap()).unwrap();
        assert!(matches!(back, CabinetCommand::StartAttract { secs_per_game: 90, ref games } if games.len() == 2));
    }

    #[test]
    fn cabinet_events_are_tagged() {
        let ev = CabinetEvent::SessionExpired { session_id: "s".into() };
        assert_eq!(serde_json::to_string(&ev).unwrap(), r#"{"type":"session_expired","session_id":"s"}"#);
        let idle: CabinetEvent = serde_json::from_str(r#"{"type":"attract_needed"}"#).unwrap();
        assert_eq!(idle, CabinetEvent::AttractNeeded);
    }
}
