import type { RequestHandler } from '@sveltejs/kit';

/**
 * GET /api/v1/edge/stream?channels=venue,alerts,commands
 *
 * Proxies the B.O.S.S. multiplexed SSE hub (over the Cloudflare Tunnel) to
 * browser clients, passing channel selection through untouched. Every event
 * arrives as `event: <channel>` with a JSON payload, exactly as BOSS emits it.
 *
 * If BOSS_SSE_URL is not configured (or the tunnel is down), this falls back
 * to a synthetic venue generator so the dashboard stays demonstrable without
 * an on-premise BOSS instance.
 */

const FALLBACK_TICK_MS = 1000;

const GAMES = [
	{ core: 'fbneo', rom: 'sf2ce.zip', name: 'Street Fighter II: Champion Edition' },
	{ core: 'pcsx2', rom: 'SLUS-00402', name: 'Tekken 2' },
	{ core: 'mame2003_plus', rom: 'mslug3.zip', name: 'Metal Slug 3' },
	{ core: 'flycast', rom: 'mvsc2.zip', name: 'Marvel vs. Capcom 2' },
	{ core: 'fbneo', rom: 'windjammers.zip', name: 'Windjammers' }
];

function jittered(base: number, spread: number): number {
	return Math.max(0, base + (Math.random() - 0.5) * spread);
}

function syntheticNode(nodeId: string) {
	const game = GAMES[Math.floor(Math.random() * GAMES.length)];
	const netplaying = nodeId === 'ASTRO-03' && Math.random() > 0.3;
	return {
		node_id: nodeId,
		hardware: 'ASTRO Arcade Hardware',
		soc: 'RK3588',
		cpu_temp_c: jittered(58, 8),
		gpu_temp_c: jittered(63, 10),
		npu_load_pct: jittered(42, 30),
		active_core: game.core,
		active_rom: game.rom,
		frame_time_ms: jittered(16.7, 2.5),
		fps: jittered(59.9, 2.5),
		input_jitter: {
			poll_rate_hz: 1000,
			p50_us: jittered(180, 60),
			p95_us: jittered(420, 120),
			p99_us: jittered(640, 200),
			max_us: jittered(1150, 400),
			dropped_events: Math.floor(jittered(2, 4))
		},
		netplay: netplaying
			? {
					peer_id: 'ASTRO-07',
					rtt_ms: jittered(34, 18),
					jitter_ms: jittered(6, 5),
					packet_loss_pct: jittered(0.6, 1.0),
					rollback_frames: Math.floor(jittered(9, 8)),
					is_host: true
				}
			: null,
		credits: {
			credits_available: Math.floor(jittered(6, 8)),
			credits_inserted_total: Math.floor(jittered(8400, 900)),
			credits_dispensed_today: Math.floor(jittered(210, 90)),
			last_coin_epoch_ms: Date.now() - Math.floor(jittered(600_000, 550_000))
		},
		mode: { playing: true },
		timestamp_epoch_ms: Date.now()
	};
}

function sseHeaders(): HeadersInit {
	return {
		'Content-Type': 'text/event-stream',
		'Cache-Control': 'no-cache, no-transform',
		Connection: 'keep-alive',
		'X-Accel-Buffering': 'no'
	};
}

function fallbackStream(): ReadableStream<Uint8Array> {
	const encoder = new TextEncoder();
	let interval: ReturnType<typeof setInterval> | undefined;
	return new ReadableStream({
		start(controller) {
			const push = (channel: string, payload: unknown) => {
				controller.enqueue(
					encoder.encode(`event: ${channel}\ndata: ${JSON.stringify(payload)}\n\n`)
				);
			};
			const tick = () => {
				push('venue', {
					venue: 'Beercade Redfern (The BK)',
					address: '113-115 Regent St, Redfern NSW 2016',
					status: 'OPEN',
					nodes_online: 4,
					nodes: ['ASTRO-01', 'ASTRO-02', 'ASTRO-03', 'ASTRO-04'].map(syntheticNode),
					generated_at_epoch_ms: Date.now()
				});
			};
			tick();
			interval = setInterval(tick, FALLBACK_TICK_MS);
		},
		cancel() {
			if (interval) clearInterval(interval);
		}
	});
}

export const GET: RequestHandler = async ({ platform, url }) => {
	const bossUrl = platform?.env?.BOSS_SSE_URL;
	if (bossUrl) {
		const target = new URL(bossUrl, url.origin);
		const channels = url.searchParams.get('channels');
		if (channels) target.searchParams.set('channels', channels);

		try {
			const upstream = await fetch(target, {
				headers: { Accept: 'text/event-stream' }
			});
			if (upstream.ok && upstream.body) {
				return new Response(upstream.body, { headers: sseHeaders() });
			}
		} catch (err) {
			console.warn('BOSS SSE proxy unreachable, falling back to synthetic venue:', err);
		}
	}

	return new Response(fallbackStream(), { headers: sseHeaders() });
};
