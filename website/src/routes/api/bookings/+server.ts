import { json, type RequestHandler } from '@sveltejs/kit';

export const GET: RequestHandler = async ({ platform }) => {
	const db = platform?.env?.DB;
	if (db) {
		try {
			const res = await db
				.prepare(
					`SELECT booking_ref, event_type, booking_date, booking_time, headcount, area, total_amount, status, created_at_ms
					 FROM function_bookings ORDER BY created_at_ms DESC LIMIT 20`
				)
				.all();
			return json({ bookings: res.results ?? [] });
		} catch {
			// Fall through if table not yet migrated
		}
	}
	return json({ bookings: [] });
};

export const POST: RequestHandler = async ({ request, platform }) => {
	const data = (await request.json()) as Record<string, any>;

	if (data.immediate_launch) {
		try {
			const envRecord = (platform?.env ?? {}) as Record<string, unknown>;
			const bossUrl =
				(typeof envRecord.BOSS_URL === 'string' && envRecord.BOSS_URL) ||
				'http://localhost:8989';
			const token =
				(typeof envRecord.BOSS_RPC_TOKEN === 'string' && envRecord.BOSS_RPC_TOKEN) || '';
			const nodeId = String(data.node_id || 'ASTRO-01');

			await fetch(`${bossUrl}/api/v1/cabinet/${nodeId}/launch_game`, {
				method: 'POST',
				headers: {
					'Content-Type': 'application/json',
					Authorization: `Bearer ${token}`
				},
				body: JSON.stringify({
					core: data.core || 'fbneo_libretro',
					rom: data.rom || 'sfiii3.zip',
					netplay: Boolean(data.netplay)
				})
			});
		} catch (err) {
			console.warn('BOSS immediate_launch forward skipped:', err);
		}
	}

	return json({ success: true, booking_id: 'bkg_' + Date.now() });
};
