export * from '../../../../../shared/proto/telemetry';
import type {
	CabinetEvent,
	CommandRecord,
	ErrorEvent,
	GameRef,
	HighScoreEntry,
	NodeTelemetry,
	TokenTransaction
} from '../../../../../shared/proto/telemetry';

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
	active_sessions?: ActiveSessionSummary[];
	catalogue_count?: { total: number; enabled: number };
	generated_at_epoch_ms: number;
}

export interface CatalogueGame extends GameRef {
	enabled?: boolean;
}

export interface ActiveSessionSummary {
	session_id: string;
	session_code: string;
	node_id: string;
	patron_name?: string | null;
	games: GameRef[];
	max_games?: number;
	in_grace?: boolean;
	remaining_secs?: number;
	grace_remaining_secs?: number;
	started_at_epoch_ms: number;
	expires_at_epoch_ms: number;
	grace_until_epoch_ms?: number | null;
	topup_grace_secs?: number;
	status?: string;
}

export interface SessionLookupResponse {
	ok: boolean;
	session_id?: string;
	session_code?: string;
	node_id?: string;
	games?: GameRef[];
	max_games?: number;
	in_grace?: boolean;
	remaining_secs?: number;
	grace_remaining_secs?: number;
	expires_at_epoch_ms?: number;
	grace_until_epoch_ms?: number | null;
	started_at_epoch_ms?: number;
	error?: string;
	detail?: string;
}

export interface CabinetAlertEvent {
	kind: 'cabinet_event';
	node_id: string;
	session_code?: string;
	grace_until_epoch_ms?: number | null;
	at_epoch_ms: number;
	event: CabinetEvent;
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
