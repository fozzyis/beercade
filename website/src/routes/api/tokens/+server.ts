import { json, type RequestHandler } from '@sveltejs/kit';

export const POST: RequestHandler = async ({ request, platform }) => {
	const data = (await request.json()) as {
		node_id?: string;
		credits?: number;
		voucher_id?: string;
	};
	const nodeId = (data.node_id || 'ASTRO-01').trim();
	const credits = Math.max(1, Math.min(100, Number(data.credits || 1)));
	const now = Date.now();

	// Mark voucher redeemed or log transaction in D1 if bound
	const db = platform?.env?.DB;
	if (db) {
		try {
			if (data.voucher_id) {
				await db
					.prepare(
						`UPDATE token_vouchers SET status = 'REDEEMED', target_node_id = ?1, redeemed_at_ms = ?2
						 WHERE voucher_id = ?3`
					)
					.bind(nodeId, now, data.voucher_id)
					.run();
			}
			await db
				.prepare(
					`INSERT INTO token_transactions (venue_id, node_id, kind, credits, balance_after, command_id, created_at_ms, ingested_at_ms)
					 VALUES (?1, ?2, 'add', ?3, ?3, ?4, ?5, ?5)`
				)
				.bind('Beercade Redfern (The BK)', nodeId, credits, data.voucher_id ?? null, now)
				.run();
		} catch {
			// Non-fatal if local D1 not yet migrated
		}
	}

	let bossForwarded = false;
	try {
		const envRecord = (platform?.env ?? {}) as Record<string, unknown>;
		const bossUrl =
			(typeof envRecord.BOSS_URL === 'string' && envRecord.BOSS_URL) ||
			'http://localhost:8989';
		const token =
			(typeof envRecord.BOSS_RPC_TOKEN === 'string' && envRecord.BOSS_RPC_TOKEN) || '';

		const resp = await fetch(`${bossUrl}/api/v1/credits/add`, {
			method: 'POST',
			headers: {
				'Content-Type': 'application/json',
				Authorization: `Bearer ${token}`
			},
			body: JSON.stringify({
				node_id: nodeId,
				credits
			})
		});
		bossForwarded = resp.ok;
	} catch {
		// BOSS tunnel optional when testing offline
	}

	return json({
		success: true,
		node_id: nodeId,
		tokens_added: credits,
		boss_forwarded: bossForwarded
	});
};
