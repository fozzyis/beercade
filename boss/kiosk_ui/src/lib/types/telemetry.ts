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
	mode: unknown;
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
	| { type: 'reboot'; delay_secs: number }
	| { type: 'launch_game'; core: string; rom: string; netplay: boolean }
	| { type: 'add_credits'; credits: number }
	| { type: 'ping'; seq: number }
	| { type: 'start_session'; session_id: string; duration_secs: number }
	| { type: 'end_session' };

export enum CommandStatus {
	Queued = 'queued',
	Dispatched = 'dispatched',
	Acknowledged = 'acknowledged',
	Failed = 'failed'
}

export interface CommandRecord {
	id: string;
	node_id: string;
	command: CabinetCommand;
	status: CommandStatus;
	issued_at_epoch_ms: number;
	dispatched_at_epoch_ms?: number;
	completed_at_epoch_ms?: number;
	result?: unknown;
}

export interface CommandAck {
	command_id: string;
	success: boolean;
	detail?: unknown;
}

export interface ErrorEvent {
	node_id: string;
	level: string;
	source: string;
	message: string;
	detail?: unknown;
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

export type PluginCategory =
	| 'pos_payments'
	| 'venue_lighting'
	| 'tournament_engine'
	| 'rfid_peripherals'
	| 'alerting_sinks'
	| 'webhook_custom';

export interface PluginUiPanelSpec {
	tab_label: string;
	icon: string;
	summary_metric_key: string;
}

export interface PluginManifest {
	id: string;
	name: string;
	version: string;
	category: PluginCategory;
	description: string;
	author: string;
	enabled: boolean;
	builtin: boolean;
	subscribed_events: string[];
	capabilities: string[];
	ui_panel: PluginUiPanelSpec;
	webhook_url?: string | null;
}

export interface PluginDescriptor {
	manifest: PluginManifest;
	state: Record<string, any>;
	updated_at_epoch_ms: number;
}

export interface AdminNodeEntry {
	telemetry: NodeTelemetry;
	last_seen_epoch_ms: number;
	age_ms: number;
	online: boolean;
}

export interface AdminOverview {
	venue: string;
	address: string;
	status: string;
	nodes_online: number;
	nodes: AdminNodeEntry[];
	commands: CommandRecord[];
	errors: ErrorEvent[];
	scores: HighScoreEntry[];
	transactions: TokenTransaction[];
	edge_sync: {
		enabled: boolean;
		endpoint?: string | null;
		buffered_items: number;
	};
	plugins: PluginDescriptor[];
	generated_at_epoch_ms: number;
}

export interface NetplayEndpointHealth {
	node_id: string;
	addr: string;
	state: string;
	loss_pct: number;
	last_seen_ms: number;
}

export interface NetplaySessionSummary {
	session_id: number;
	endpoints: NetplayEndpointHealth[];
	desync_events: number;
	created_ms: number;
	last_seen_ms: number;
	relayed_packets: number;
}

export interface OtaPayloadEntry {
	filename: string;
	size_bytes: number;
	sha256?: string | null;
}
