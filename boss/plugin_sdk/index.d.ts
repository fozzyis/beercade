/**
 * B.O.S.S. (Beercade Operational Supervisory System) Plugin SDK Type Definitions
 */

export type PluginCategory =
	| 'pos_payments'
	| 'venue_lighting'
	| 'tournament_engine'
	| 'rfid_peripherals'
	| 'alerting_sinks'
	| 'webhook_custom';

export type BossEventName =
	| '*'
	| 'telemetry_heartbeat'
	| 'node_online'
	| 'node_offline'
	| 'error_reported'
	| 'high_score'
	| 'credit_transaction'
	| 'command_updated';

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
	subscribed_events: BossEventName[];
	capabilities: string[];
	ui_panel: PluginUiPanelSpec;
	webhook_url?: string | null;
}

export interface HighScorePayload {
	node_id: string;
	active_core: string;
	active_rom: string;
	game_name: string;
	score: number;
	initials: string;
	achieved_at_epoch_ms: number;
}

export interface CreditTransactionPayload {
	node_id: string;
	kind: string;
	credits: number;
	balance_after: number;
	command_id?: string;
	created_at_epoch_ms: number;
}

export type PluginEvent =
	| { kind: 'high_score'; data: HighScorePayload }
	| { kind: 'credit_transaction'; data: CreditTransactionPayload }
	| { kind: 'node_online'; data: { node_id: string; at_epoch_ms: number } }
	| { kind: 'node_offline'; data: { node_ids: string[]; at_epoch_ms: number } }
	| { kind: 'error_reported'; data: Record<string, unknown> }
	| { kind: 'telemetry_heartbeat'; data: Record<string, unknown> }
	| { kind: 'command_updated'; data: Record<string, unknown> };

export interface BossWebhookDelivery {
	plugin_id: string;
	dispatched_at_epoch_ms: number;
	event: PluginEvent;
}

export interface PluginActionRequest<T = Record<string, unknown>> {
	action: string;
	payload: T;
}

export interface PluginActionResponse<R = unknown, S = Record<string, unknown>> {
	ok: boolean;
	plugin_id: string;
	action: string;
	result: R;
	state: S;
}
