export * from '../../../../../shared/proto/telemetry';
import type {
	CommandRecord,
	ErrorEvent,
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
