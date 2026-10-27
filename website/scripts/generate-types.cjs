const fs = require('fs');
const path = require('path');

const tsCode = `
export interface NodeTelemetry {
    node_id: string;
    hardware: string;
    soc: string;
    cpu_temp_c: number;
    gpu_temp_c: number;
    npu_load_pct: number;
    active_core: string;
    active_rom: string;
    frame_time_ms: number;
    fps: number;
    input_jitter: InputJitterStats;
    netplay?: NetplayStats;
    credits: CreditCounters;
    mode: any;
    timestamp_epoch_ms: number;
}

export interface InputJitterStats {
    poll_rate_hz: number;
    p50_us: number;
    p95_us: number;
    p99_us: number;
    max_us: number;
    dropped_events: number;
}

export interface NetplayStats {
    peer_id: string;
    rtt_ms: number;
    jitter_ms: number;
    packet_loss_pct: number;
    rollback_frames: number;
    is_host: boolean;
}

export interface CreditCounters {
    credits_available: number;
    credits_inserted_total: number;
    credits_dispensed_today: number;
    last_coin_epoch_ms?: number;
}

export interface VenueTelemetryReport {
    venue: string;
    address: string;
    status: string;
    nodes_online: number;
    nodes: NodeTelemetry[];
    generated_at_epoch_ms: number;
}

export type CabinetCommand = 
  | { type: "reboot"; delay_secs: number }
  | { type: "launch_game"; core: string; rom: string; netplay: boolean }
  | { type: "add_credits"; credits: number }
  | { type: "ping"; seq: number }
  | { type: "start_session"; session_id: string; duration_secs: number }
  | { type: "end_session" };

export enum CommandStatus {
    Queued = "queued",
    Dispatched = "dispatched",
    Acknowledged = "acknowledged",
    Failed = "failed",
}

export interface CommandRecord {
    id: string;
    node_id: string;
    command: CabinetCommand;
    status: CommandStatus;
    issued_at_epoch_ms: number;
    dispatched_at_epoch_ms?: number;
    completed_at_epoch_ms?: number;
    result?: any;
}

export interface CommandAck {
    command_id: string;
    success: boolean;
    detail?: any;
}

export interface ErrorEvent {
    node_id: string;
    level: string;
    source: string;
    message: string;
    detail?: any;
    occurred_at_epoch_ms: number;
}

export interface TokenTransaction {
    node_id: string;
    kind: string;
    credits: number;
    balance_after: number;
    command_id?: string;
    created_at_epoch_ms: number;
}

export interface HighScoreEntry {
    node_id: string;
    active_core: string;
    active_rom: string;
    game_name: string;
    score: number;
    initials: string;
    achieved_at_epoch_ms: number;
}

export interface EdgeBatchEnvelope {
    batch_id: string;
    venue: string;
    generated_at_epoch_ms: number;
    nodes: NodeTelemetry[];
    errors: ErrorEvent[];
    transactions: TokenTransaction[];
    scores: HighScoreEntry[];
}

export interface NodeOfflineAlert {
    kind: "node_offline";
    node_ids: string[];
    at_epoch_ms: number;
}

export interface NodeOnlineAlert {
    kind: "node_online";
    node_id: string;
    at_epoch_ms: number;
}

export interface EdgeIngestResult {
    ok: boolean;
    batch_id: string;
    deduped: boolean;
    accepted: {
        nodes: number;
        errors: number;
        transactions: number;
        scores: number;
    };
}

export interface VenueLatestSnapshot {
    venue: string;
    generated_at_epoch_ms: number;
    nodes: NodeTelemetry[];
}
`;

const dir = path.join(__dirname, '../src/lib/types');
if (!fs.existsSync(dir)){
    fs.mkdirSync(dir, { recursive: true });
}

fs.writeFileSync(path.join(dir, 'telemetry.ts'), tsCode.trim());
console.log('Successfully generated telemetry.ts');
