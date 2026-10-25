import { json, type RequestHandler } from '@sveltejs/kit';

const ALLOWED_TIERS = [5, 10, 12, 20, 35, 50, 100, 200, 300, 500];

const ASTRO_GAME_TIME_PASSES: Record<
	string,
	{ label: string; durationMins: number; includedCredits: number; priceAud: number }
> = {
	'astro-30m': {
		label: '30-Min A.S.T.R.O. Session Pass',
		durationMins: 30,
		includedCredits: 10,
		priceAud: 12
	},
	'astro-60m': {
		label: '60-Min Unlimited A.S.T.R.O. Pass',
		durationMins: 60,
		includedCredits: 25,
		priceAud: 20
	},
	'astro-120m': {
		label: '2-Hour A.S.T.R.O. Marathon Pass',
		durationMins: 120,
		includedCredits: 60,
		priceAud: 35
	}
};

function computeBonusPct(amount: number): number {
	if (amount >= 500) return 30;
	if (amount >= 300) return 25;
	if (amount >= 200) return 20;
	if (amount >= 100) return 15;
	if (amount >= 50) return 10;
	return 0;
}

export const GET: RequestHandler = async ({ url, platform }) => {
	const voucherId = url.searchParams.get('voucher')?.trim();
	if (!voucherId) {
		return json({ error: 'Missing voucher parameter' }, { status: 400 });
	}

	const db = platform?.env?.DB;
	if (db) {
		try {
			const row = await db
				.prepare(
					`SELECT voucher_id, email, amount_aud, base_tokens, bonus_tokens, total_tokens, target_node_id, status, created_at_ms
					 FROM token_vouchers WHERE voucher_id = ?1 LIMIT 1`
				)
				.bind(voucherId)
				.first<Record<string, unknown>>();
			if (row) {
				return json({
					success: true,
					voucherId: row.voucher_id,
					email: row.email,
					amount: row.amount_aud,
					tokens: row.total_tokens,
					bonusTokens: row.bonus_tokens,
					targetNodeId: row.target_node_id,
					status: row.status,
					qrData: `BEERCADE:VOUCHER:${row.voucher_id}:${row.total_tokens}`,
					passUrl: `/api/checkout/token-pack?voucher=${row.voucher_id}&format=pass`
				});
			}
		} catch {
			// Fall through if table not yet migrated in local dev
		}
	}

	return json({ error: 'Voucher not found' }, { status: 404 });
};

export const POST: RequestHandler = async ({ request, platform, url }) => {
	try {
		const body = (await request.json()) as {
			amount?: number;
			email: string;
			quantity?: number;
			targetNodeId?: string;
			passType?: string;
			provider?: 'square' | 'stripe';
		};
		const passSpec = body.passType ? ASTRO_GAME_TIME_PASSES[body.passType] : undefined;
		const amount = passSpec ? passSpec.priceAud : Number(body.amount);
		const email = (body.email ?? '').trim();
		const quantity = Math.max(1, Math.min(10, Number(body.quantity ?? 1)));
		const targetNodeId = body.targetNodeId?.trim() || null;
		const durationMins = passSpec ? passSpec.durationMins * quantity : null;

		if (!passSpec && !ALLOWED_TIERS.includes(amount)) {
			return json({ error: 'Invalid token pack or A.S.T.R.O. game-time tier' }, { status: 400 });
		}

		if (!email || !email.includes('@')) {
			return json({ error: 'Valid email is required for voucher delivery' }, { status: 400 });
		}

		const bonusPercentage = passSpec ? 0 : computeBonusPct(amount);
		const baseTokens = passSpec ? passSpec.includedCredits * quantity : amount * quantity;
		const bonusTokens = passSpec ? 0 : Math.floor(baseTokens * (bonusPercentage / 100));
		const totalTokens = baseTokens + bonusTokens;
		const now = Date.now();
		const voucherPrefix = passSpec ? 'ASTRO-SQ' : 'BK-VCH';
		const voucherId = `${voucherPrefix}-${now.toString(36).slice(-4).toUpperCase()}-${Math.random().toString(36).slice(2, 6).toUpperCase()}`;
		const qrData = passSpec
			? `BEERCADE:SQUARE_PASS:${voucherId}:${durationMins}M:${totalTokens}CR:${targetNodeId ?? 'ANY'}`
			: `BEERCADE:VOUCHER:${voucherId}:${totalTokens}:${targetNodeId ?? 'ANY'}`;
		const passUrl = `${url.origin}/tokens?voucher=${encodeURIComponent(voucherId)}`;

		const envRecord = (platform?.env ?? {}) as Record<string, unknown>;
		const squareAccessToken =
			(typeof envRecord.SQUARE_ACCESS_TOKEN === 'string' ? envRecord.SQUARE_ACCESS_TOKEN : '') ||
			process.env.SQUARE_ACCESS_TOKEN ||
			'';
		const squareLocationId =
			(typeof envRecord.SQUARE_LOCATION_ID === 'string' ? envRecord.SQUARE_LOCATION_ID : '') ||
			process.env.SQUARE_LOCATION_ID ||
			'L_BEERCADE_REDFERN';
		const stripeSecretKey =
			(typeof envRecord.STRIPE_SECRET_KEY === 'string' ? envRecord.STRIPE_SECRET_KEY : '') ||
			process.env.STRIPE_SECRET_KEY ||
			'';
		const bossUrl =
			(typeof envRecord.BOSS_URL === 'string' ? envRecord.BOSS_URL : '') ||
			process.env.BOSS_URL ||
			'';

		let stripeSessionId: string | null = null;
		let checkoutUrl: string | null = null;
		let paymentProvider = 'Square POS & Online Bridge';

		// 1. Prefer Square Online Checkout Payment Links when SQUARE_ACCESS_TOKEN is configured
		if (squareAccessToken.trim() && body.provider !== 'stripe') {
			try {
				const sqRes = await fetch(
					'https://connect.squareup.com/v2/online-checkout/payment-links',
					{
						method: 'POST',
						headers: {
							Authorization: `Bearer ${squareAccessToken.trim()}`,
							'Content-Type': 'application/json',
							'Square-Version': '2024-06-04'
						},
						body: JSON.stringify({
							idempotency_key: voucherId,
							quick_pay: {
								name: passSpec
									? `${passSpec.label} (${voucherId})`
									: `Beercade ${totalTokens} Arcade Tokens (${voucherId})`,
								price_money: {
									amount: amount * quantity * 100,
									currency: 'AUD'
								},
								location_id: squareLocationId
							},
							checkout_options: {
								redirect_url: `${url.origin}/tokens?voucher=${encodeURIComponent(voucherId)}`
							}
						})
					}
				);
				if (sqRes.ok) {
					const sqData = (await sqRes.json()) as {
						payment_link?: { id?: string; url?: string };
					};
					stripeSessionId = sqData.payment_link?.id ?? null;
					checkoutUrl = sqData.payment_link?.url ?? null;
					paymentProvider = 'Square Online Checkout';
				}
			} catch (sqErr) {
				console.warn('Square payment-link fallback to instant voucher:', sqErr);
			}
		} else if (stripeSecretKey.trim()) {
			const Stripe = (await import('stripe')).default;
			const stripe = new Stripe(stripeSecretKey.trim(), {
				apiVersion: '2023-10-16' as any
			});

			const session = await stripe.checkout.sessions.create({
				customer_email: email,
				line_items: [
					{
						price_data: {
							currency: 'aud',
							product_data: {
								name: passSpec
									? `${passSpec.label} (${voucherId})`
									: `Beercade ${totalTokens} Arcade Tokens (${voucherId})`,
								description: passSpec
									? `${durationMins} mins A.S.T.R.O. cabinet play + ${totalTokens} credits`
									: `Base: ${baseTokens} tokens + Bonus: ${bonusTokens} tokens`
							},
							unit_amount: amount * 100
						},
						quantity
					}
				],
				metadata: {
					voucher_id: voucherId,
					total_tokens: String(totalTokens),
					duration_mins: durationMins ? String(durationMins) : '',
					target_node_id: targetNodeId ?? ''
				},
				mode: 'payment',
				success_url: `${url.origin}/tokens?voucher=${encodeURIComponent(voucherId)}&session_id={CHECKOUT_SESSION_ID}`,
				cancel_url: `${url.origin}/tokens?cancelled=1`
			});

			stripeSessionId = session.id;
			checkoutUrl = session.url;
			paymentProvider = 'Stripe Checkout';
		}

		// Sync A.S.T.R.O. Game-Time Pass directly with B.O.S.S. `pos-payments` plugin when BOSS_URL is reachable
		let bossSynced = false;
		if (bossUrl.trim() && (passSpec || targetNodeId)) {
			try {
				const bossRes = await fetch(
					`${bossUrl.replace(/\/$/, '')}/api/v1/plugins/pos-payments/action`,
					{
						method: 'POST',
						headers: { 'Content-Type': 'application/json' },
						body: JSON.stringify({
							action: 'activate_game_time_pass',
							payload: {
								node_id: targetNodeId || 'ASTRO-01',
								pass_code: voucherId,
								duration_mins: durationMins ?? 30,
								credits: totalTokens,
								amount_cents: amount * quantity * 100,
								patron_name: email.split('@')[0],
								provider: paymentProvider
							}
						})
					}
				);
				bossSynced = bossRes.ok;
			} catch {
				// B.O.S.S. sync optional when edge-only
			}
		}

		// Persist to Cloudflare D1 when bound
		const db = platform?.env?.DB;
		if (db) {
			try {
				await db
					.prepare(
						`INSERT INTO token_vouchers
						 (voucher_id, email, amount_aud, base_tokens, bonus_tokens, total_tokens, target_node_id, stripe_session_id, status, created_at_ms)
						 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)`
					)
					.bind(
						voucherId,
						email,
						amount * quantity,
						baseTokens,
						bonusTokens,
						totalTokens,
						targetNodeId,
						stripeSessionId,
						checkoutUrl ? 'PENDING_PAYMENT' : 'ISSUED',
						now
					)
					.run();
			} catch (dbErr) {
				console.warn('D1 token_vouchers insert skipped:', dbErr);
			}
		}

		return json({
			success: true,
			voucherId,
			email,
			amount: amount * quantity,
			tokens: totalTokens,
			bonusTokens,
			durationMins,
			passLabel: passSpec?.label ?? null,
			paymentProvider,
			bossSynced,
			targetNodeId,
			qrData,
			passUrl,
			url: checkoutUrl,
			checkoutUrl
		});
	} catch (err) {
		console.error('Token pack checkout failed:', err);
		return json({ error: 'Failed to process checkout session' }, { status: 500 });
	}
};
