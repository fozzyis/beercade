export * from '../../../../shared/proto/telemetry';
import type { NodeTelemetry } from '../../../../shared/proto/telemetry';

export interface NodeOfflineAlert {
	kind: 'node_offline';
	node_ids: string[];
	at_epoch_ms: number;
}

export interface NodeOnlineAlert {
	kind: 'node_online';
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
