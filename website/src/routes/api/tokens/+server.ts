import { json, type RequestHandler } from '@sveltejs/kit';

export const POST: RequestHandler = async ({ request, platform }) => {
	let data: {
		node_id?: string;
		credits?: number;
		voucher_id?: string;
	};
	try {
		data = (await request.json()) as typeof data;
	} catch {
		return json({ error: 'Invalid JSON request body' }, { status: 400 });
	}

	const nodeId = (data.node_id || 'ASTRO-01').trim();
	const voucherId = data.voucher_id?.trim() || undefined;
	let credits = Math.max(1, Math.min(100, Number(data.credits || 1)));
	const now = Date.now();

	const db = platform?.env?.DB;
	if (voucherId && db) {
		const updateRes = await db
			.prepare(
				`UPDATE token_vouchers SET status = 'REDEEMED', target_node_id = ?1, redeemed_at_ms = ?2
				 WHERE voucher_id = ?3 AND status = 'ISSUED'`
			)
			.bind(nodeId, now, voucherId)
			.run();
		if ((updateRes?.meta?.changes ?? 0) === 0) {
			return json(
				{ error: 'Voucher is pending payment, expired, or already redeemed' },
				{ status: 409 }
			);
		}

		const voucherRow = await db
			.prepare(`SELECT total_tokens FROM token_vouchers WHERE voucher_id = ?1 LIMIT 1`)
			.bind(voucherId)
			.first<{ total_tokens: number }>();
		if (voucherRow && Number.isFinite(Number(voucherRow.total_tokens)) && Number(voucherRow.total_tokens) > 0) {
			credits = Number(voucherRow.total_tokens);
		}
	}

	const bossUrl = (
		platform?.env?.BOSS_URL ||
		(typeof process !== 'undefined' ? process.env?.BOSS_URL : undefined) ||
		''
	)
		.trim()
		.replace(/\/$/, '');
	const token = (
		platform?.env?.BOSS_RPC_TOKEN ||
		(typeof process !== 'undefined' ? process.env?.BOSS_RPC_TOKEN : undefined) ||
		''
	).trim();

	let bossForwarded = false;
	if (bossUrl) {
		const revertVoucherIfNeeded = async () => {
			if (db && voucherId) {
				await db
					.prepare(
						`UPDATE token_vouchers SET status = 'ISSUED', redeemed_at_ms = NULL
						 WHERE voucher_id = ?1 AND redeemed_at_ms = ?2`
					)
					.bind(voucherId, now)
					.run();
			}
		};

		try {
			const headers: Record<string, string> = { 'Content-Type': 'application/json' };
			if (token) headers['Authorization'] = `Bearer ${token}`;
			const resp = await fetch(`${bossUrl}/api/v1/credits/add`, {
				method: 'POST',
				headers,
				body: JSON.stringify({
					node_id: nodeId,
					credits
				})
			});
			if (!resp.ok) {
				await revertVoucherIfNeeded();
				return json(
					{ error: `B.O.S.S. credit dispatch failed (${resp.status})` },
					{ status: 502 }
				);
			}
			bossForwarded = true;
		} catch {
			await revertVoucherIfNeeded();
			return json({ error: 'B.O.S.S. tunnel unreachable' }, { status: 502 });
		}
	}

	if (db) {
		await db
			.prepare(
				`INSERT INTO token_transactions (venue_id, node_id, kind, credits, balance_after, command_id, created_at_ms, ingested_at_ms)
				 VALUES (?1, ?2, 'add', ?3, ?3, ?4, ?5, ?5)`
			)
			.bind('Beercade Redfern (The BK)', nodeId, credits, voucherId ?? null, now)
			.run();
	}

	return json({
		success: true,
		node_id: nodeId,
		tokens_added: credits,
		boss_forwarded: bossForwarded
	});
};
