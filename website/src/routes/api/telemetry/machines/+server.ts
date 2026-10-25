import { json, type RequestHandler } from '@sveltejs/kit';
import type { VenueLatestSnapshot } from '$lib/types/telemetry';

const DEFAULT_VENUE = 'Beercade Redfern (The BK)';

export const GET: RequestHandler = async ({ platform }) => {
	let astroNodes = [
		{
			node_id: 'ASTRO-01',
			hardware: 'O.R.I.O.N. Radxa ROCK 5C 32GB (RK3588S2)',
			current_core: 'fbneo',
			current_game: 'Street Fighter II: Champion Edition',
			status: 'IN_USE',
			session_duration_sec: 412,
			cpu_temp_c: 54.2,
			fps: 59.98,
			credits_available: 6
		},
		{
			node_id: 'ASTRO-02',
			hardware: 'O.R.I.O.N. Radxa ROCK 5C 32GB (RK3588S2)',
			current_core: 'pcsx2',
			current_game: 'Tekken 3',
			status: 'AVAILABLE',
			session_duration_sec: 0,
			cpu_temp_c: 49.8,
			fps: 60.0,
			credits_available: 4
		},
		{
			node_id: 'ASTRO-03',
			hardware: 'O.R.I.O.N. Orange Pi 5 Pro 16GB LPDDR5 (RK3588S)',
			current_core: 'mame2003_plus',
			current_game: 'Metal Slug 3',
			status: 'IN_USE',
			session_duration_sec: 1250,
			cpu_temp_c: 61.4,
			fps: 59.95,
			credits_available: 2
		},
		{
			node_id: 'ASTRO-04',
			hardware: 'O.R.I.O.N. Orange Pi 5 Pro 16GB LPDDR5 (RK3588S)',
			current_core: 'flycast',
			current_game: 'Marvel vs. Capcom 2',
			status: 'IN_USE',
			session_duration_sec: 680,
			cpu_temp_c: 52.0,
			fps: 60.0,
			credits_available: 3
		}
	];

	let featuredHighscores = [
		{ game: 'Street Fighter III: 3rd Strike', score: 984200, initials: 'DGO' },
		{ game: 'Attack from Mars', score: 489201940, initials: 'VAL' },
		{ game: 'Street Fighter II', score: 980500, initials: 'RYU' },
		{ game: 'Donkey Kong', score: 114200, initials: 'AND' },
		{ game: 'Galaga', score: 254890, initials: 'ACE' }
	];

	// Enrich from Cloudflare KV / D1 when available
	const kv = platform?.env?.TELEMETRY_KV;
	if (kv) {
		try {
			const raw = await kv.get(`latest:venue:${DEFAULT_VENUE}`);
			if (raw) {
				const snap = JSON.parse(raw) as VenueLatestSnapshot;
				if (Array.isArray(snap.nodes) && snap.nodes.length > 0) {
					astroNodes = snap.nodes.map((n) => ({
						node_id: n.node_id,
						hardware: `${n.hardware} (${n.soc})`,
						current_core: n.active_core,
						current_game: n.active_rom,
						status: n.credits.credits_available > 0 ? 'IN_USE' : 'AVAILABLE',
						session_duration_sec: 300,
						cpu_temp_c: n.cpu_temp_c,
						fps: n.fps,
						credits_available: n.credits.credits_available
					}));
				}
			}
		} catch {
			// Use default snapshot
		}
	}

	const db = platform?.env?.DB;
	if (db) {
		try {
			const res = await db
				.prepare(
					`SELECT game_name AS game, score, initials
					 FROM high_scores
					 ORDER BY score DESC, achieved_at_ms DESC
					 LIMIT 6`
				)
				.all<{ game: string; score: number; initials: string }>();
			if (res.results && res.results.length > 0) {
				featuredHighscores = res.results;
			}
		} catch {
			// Use default highscores
		}
	}

	return json({
		timestamp: new Date().toISOString(),
		venue_status: 'OPEN',
		machines_total: 36,
		machines_active: 28,
		astro_nodes: astroNodes,
		featured_highscores: featuredHighscores
	});
};
