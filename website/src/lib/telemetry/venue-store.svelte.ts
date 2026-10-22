/**
 * Venue telemetry store — Svelte 5 runes class.
 *
 * Connects to the BOSS SSE hub (proxied by /api/v1/edge/stream) and exposes
 * live node state, connection health and the rolling alert feed. Node data
 * is overwritten per heartbeat; nodes not heard from within STALENESS_MS are
 * treated as offline by consumers.
 */
import type {
	NodeTelemetry,
	VenueTelemetryReport,
	CommandRecord,
	NodeOnlineAlert,
	NodeOfflineAlert
} from '$lib/types/telemetry';

/** Matches BOSS's `STALENESS_MS`. */
export const STALENESS_MS = 15_000;

export type ConnectionState = 'connecting' | 'live' | 'reconnecting' | 'offline';

export interface AlertFeedItem {
	id: string;
	at_epoch_ms: number;
	summary: string;
	severity: 'info' | 'warn' | 'fatal';
}

const MAX_FEED = 50;
const MAX_RETRIES = 8;

function backoffMs(retry: number): number {
	return Math.min(30_000, 1000 * 2 ** retry);
}

export class VenueStore {
	nodes = $state<Record<string, NodeTelemetry>>({});
	venue = $state<{
		name: string;
		address: string;
		status: string;
		generated_at_epoch_ms: number;
	} | null>(null);
	connection = $state<ConnectionState>('connecting');
	lastEventAt = $state<number | null>(null);
	alerts = $state<AlertFeedItem[]>([]);
	commands = $state<CommandRecord[]>([]);

	#source: EventSource | null = null;
	#retry = 0;
	#disposed = false;
	#channels = 'venue,alerts,commands';

	/** Nodes currently considered online (heartbeat within staleness window). */
	onlineNodes = $derived.by(() => {
		const now = Date.now();
		return Object.values(this.nodes).filter((n) => now - n.timestamp_epoch_ms < STALENESS_MS);
	});

	/** All nodes seen in this session, online or not. */
	allNodes = $derived(Object.values(this.nodes));

	avgCpuTemp = $derived.by(() => {
		const nodes = this.onlineNodes;
		if (nodes.length === 0) return null;
		return nodes.reduce((sum, n) => sum + n.cpu_temp_c, 0) / nodes.length;
	});

	avgFps = $derived.by(() => {
		const nodes = this.onlineNodes;
		if (nodes.length === 0) return null;
		return nodes.reduce((sum, n) => sum + n.fps, 0) / nodes.length;
	});

	netplaySessions = $derived(this.onlineNodes.filter((n) => n.netplay !== null).length);

	connect(channels = 'venue,alerts,commands'): void {
		this.#channels = channels;
		this.#disposed = false;
		this.#open();
	}

	dispose(): void {
		this.#disposed = true;
		this.#source?.close();
		this.#source = null;
		this.connection = 'offline';
	}

	#open(): void {
		if (this.#disposed || typeof EventSource === 'undefined') return;
		this.connection = this.#retry === 0 ? 'connecting' : 'reconnecting';
		this.#source?.close();

		const es = new EventSource(`/api/v1/edge/stream?channels=${this.#channels}`);
		this.#source = es;

		es.addEventListener('open', () => {
			this.#retry = 0;
			this.connection = 'live';
		});

		es.addEventListener('venue', (event) => {
			this.connection = 'live';
			this.lastEventAt = Date.now();
			try {
				const report = JSON.parse((event as MessageEvent).data) as VenueTelemetryReport;
				this.#applyVenueReport(report);
			} catch (err) {
				console.error('Failed to parse venue event', err);
			}
		});

		es.addEventListener('alerts', (event) => {
			this.lastEventAt = Date.now();
			try {
				this.#applyAlert(JSON.parse((event as MessageEvent).data));
			} catch (err) {
				console.error('Failed to parse alert event', err);
			}
		});

		es.addEventListener('commands', (event) => {
			try {
				const record = JSON.parse((event as MessageEvent).data) as CommandRecord;
				this.commands = [record, ...this.commands.filter((c) => c.id !== record.id)].slice(
					0,
					MAX_FEED
				);
			} catch (err) {
				console.error('Failed to parse command event', err);
			}
		});

		es.addEventListener('error', () => {
			// EventSource retries automatically, but without a backoff cap — take
			// over with our own exponential schedule.
			es.close();
			this.#source = null;
			if (this.#disposed) return;
			this.#retry = Math.min(this.#retry + 1, MAX_RETRIES);
			this.connection = 'reconnecting';
			setTimeout(() => this.#open(), backoffMs(this.#retry));
		});
	}

	#applyVenueReport(report: VenueTelemetryReport): void {
		this.venue = {
			name: report.venue,
			address: report.address,
			status: report.status,
			generated_at_epoch_ms: report.generated_at_epoch_ms
		};
		const next: Record<string, NodeTelemetry> = { ...this.nodes };
		for (const node of report.nodes) {
			next[node.node_id] = node;
		}
		this.nodes = next;
	}

	#applyAlert(payload: unknown): void {
		const at = Date.now();
		if (
			payload &&
			typeof payload === 'object' &&
			'kind' in payload &&
			(payload as { kind: string }).kind === 'node_offline'
		) {
			const alert = payload as NodeOfflineAlert;
			this.#pushAlert({
				id: `offline-${at}-${alert.node_ids.join('+')}`,
				at_epoch_ms: alert.at_epoch_ms,
				summary: `OFFLINE: ${alert.node_ids.join(', ')}`,
				severity: 'warn'
			});
			return;
		}
		if (
			payload &&
			typeof payload === 'object' &&
			'kind' in payload &&
			(payload as { kind: string }).kind === 'node_online'
		) {
			const alert = payload as NodeOnlineAlert;
			this.#pushAlert({
				id: `online-${at}-${alert.node_id}`,
				at_epoch_ms: alert.at_epoch_ms,
				summary: `ONLINE: ${alert.node_id}`,
				severity: 'info'
			});
			return;
		}
		if (payload && typeof payload === 'object' && 'level' in payload && 'message' in payload) {
			const err = payload as {
				level: string;
				node_id: string;
				source: string;
				message: string;
				occurred_at_epoch_ms: number;
			};
			this.#pushAlert({
				id: `err-${err.occurred_at_epoch_ms}-${err.node_id}`,
				at_epoch_ms: err.occurred_at_epoch_ms,
				summary: `[${err.level.toUpperCase()}] ${err.node_id}/${err.source}: ${err.message}`,
				severity: err.level === 'fatal' ? 'fatal' : 'warn'
			});
		}
	}

	#pushAlert(item: AlertFeedItem): void {
		this.alerts = [item, ...this.alerts].slice(0, MAX_FEED);
	}
}

export const venueStore = new VenueStore();
