import test from 'node:test';
import assert from 'node:assert/strict';
import {
	generateSessionCode,
	processStripeWebhookEvent,
	verifyStripeSignature
} from '../src/lib/stripe.ts';
import { POST as stripeWebhookPost } from '../src/routes/api/webhooks/stripe/+server.ts';

async function signPayload(rawBody, secret, timestampSec) {
	const encoder = new TextEncoder();
	const key = await crypto.subtle.importKey(
		'raw',
		encoder.encode(secret),
		{ name: 'HMAC', hash: 'SHA-256' },
		false,
		['sign']
	);
	const mac = await crypto.subtle.sign('HMAC', key, encoder.encode(`${timestampSec}.${rawBody}`));
	const hex = Array.from(new Uint8Array(mac))
		.map((b) => b.toString(16).padStart(2, '0'))
		.join('');
	return `t=${timestampSec},v1=${hex}`;
}

function createMockKV() {
	const store = new Map();
	return {
		store,
		async get(key) {
			return store.has(key) ? store.get(key).value : null;
		},
		async put(key, value, options) {
			store.set(key, { value, expirationTtl: options?.expirationTtl });
		}
	};
}

function createMockD1({ vouchers = [], bookings = [] } = {}) {
	const events = new Map();
	const voucherRows = vouchers.map((v) => ({ ...v }));
	const bookingRows = bookings.map((b) => ({ ...b }));
	const queries = [];

	return {
		events,
		voucherRows,
		bookingRows,
		queries,
		prepare(sql) {
			return {
				bind(...args) {
					return {
						async run() {
							queries.push({ sql, args });
							if (sql.includes('INSERT OR IGNORE INTO stripe_webhook_events')) {
								const [eventId, eventType, stripeSessionId, outcome, processedAtMs] = args;
								if (events.has(eventId)) {
									return { meta: { changes: 0 } };
								}
								events.set(eventId, {
									event_id: eventId,
									event_type: eventType,
									stripe_session_id: stripeSessionId,
									outcome,
									processed_at_ms: processedAtMs
								});
								return { meta: { changes: 1 } };
							}
							if (sql.includes("UPDATE token_vouchers") && sql.includes("status = 'ISSUED'")) {
								const [sessionCode, stripeSessionId, voucherId] = args;
								let changes = 0;
								for (const row of voucherRows) {
									const matches =
										(stripeSessionId && row.stripe_session_id === stripeSessionId) ||
										(voucherId && row.voucher_id === voucherId);
									if (matches && row.status === 'PENDING_PAYMENT') {
										row.status = 'ISSUED';
										row.session_code = sessionCode;
										changes++;
									}
								}
								return { meta: { changes } };
							}
							if (sql.includes("UPDATE token_vouchers") && sql.includes("status = 'EXPIRED'")) {
								const [stripeSessionId, voucherId] = args;
								let changes = 0;
								for (const row of voucherRows) {
									const matches =
										(stripeSessionId && row.stripe_session_id === stripeSessionId) ||
										(voucherId && row.voucher_id === voucherId);
									if (matches && row.status === 'PENDING_PAYMENT') {
										row.status = 'EXPIRED';
										changes++;
									}
								}
								return { meta: { changes } };
							}
							if (sql.includes("UPDATE function_bookings") && sql.includes("status = 'CONFIRMED'")) {
								const [stripeSessionId, bookingRef] = args;
								let changes = 0;
								for (const row of bookingRows) {
									const matches =
										(stripeSessionId && row.stripe_session_id === stripeSessionId) ||
										(bookingRef && row.booking_ref === bookingRef);
									if (matches && row.status === 'PENDING_STRIPE') {
										row.status = 'CONFIRMED';
										changes++;
									}
								}
								return { meta: { changes } };
							}
							if (sql.includes("UPDATE function_bookings") && sql.includes("status = 'EXPIRED'")) {
								const [stripeSessionId, bookingRef] = args;
								let changes = 0;
								for (const row of bookingRows) {
									const matches =
										(stripeSessionId && row.stripe_session_id === stripeSessionId) ||
										(bookingRef && row.booking_ref === bookingRef);
									if (matches && row.status === 'PENDING_STRIPE') {
										row.status = 'EXPIRED';
										changes++;
									}
								}
								return { meta: { changes } };
							}
							return { meta: { changes: 0 } };
						}
					};
				}
			};
		}
	};
}

test('verifyStripeSignature rejects missing, forged, and stale (>300s) signatures', async () => {
	const secret = 'whsec_test_beercade_secret_123';
	const body = JSON.stringify({ id: 'evt_sig_1', type: 'checkout.session.completed' });
	const nowSec = 1_760_000_000;

	const validSig = await signPayload(body, secret, nowSec);
	assert.equal(await verifyStripeSignature(body, validSig, secret, nowSec), true);
	assert.equal(await verifyStripeSignature(body, validSig, secret, nowSec + 300), true);

	// Missing / empty header or secret
	assert.equal(await verifyStripeSignature(body, null, secret, nowSec), false);
	assert.equal(await verifyStripeSignature(body, '', secret, nowSec), false);
	assert.equal(await verifyStripeSignature(body, validSig, '', nowSec), false);

	// Forged signature
	const forged = `t=${nowSec},v1=${'a'.repeat(64)}`;
	assert.equal(await verifyStripeSignature(body, forged, secret, nowSec), false);
	assert.equal(await verifyStripeSignature(body + ' ', validSig, secret, nowSec), false);

	// Stale (>300s tolerance)
	const staleSig = await signPayload(body, secret, nowSec - 301);
	assert.equal(await verifyStripeSignature(body, staleSig, secret, nowSec), false);

	// Key rotation style multiple v1 candidates
	const multiSig = `t=${nowSec},v1=${'0'.repeat(64)},${validSig.split(',')[1]}`;
	assert.equal(await verifyStripeSignature(body, multiSig, secret, nowSec), true);
});

test('POST /api/webhooks/stripe enforces secret presence and signature verification (503/400/401)', async () => {
	const prevSecret = process.env.STRIPE_WEBHOOK_SECRET;
	delete process.env.STRIPE_WEBHOOK_SECRET;
	try {
		const missingSecretReq = new Request('https://beercade.test/api/webhooks/stripe', {
			method: 'POST',
			body: '{}'
		});
		const res503 = await stripeWebhookPost({ request: missingSecretReq, platform: { env: {} } });
		assert.equal(res503.status, 503);

		const secret = 'whsec_handler_test_key';
		const noSigReq = new Request('https://beercade.test/api/webhooks/stripe', {
			method: 'POST',
			body: '{}'
		});
		const res400 = await stripeWebhookPost({
			request: noSigReq,
			platform: { env: { STRIPE_WEBHOOK_SECRET: secret } }
		});
		assert.equal(res400.status, 400);

		const badSigReq = new Request('https://beercade.test/api/webhooks/stripe', {
			method: 'POST',
			headers: { 'stripe-signature': `t=${Math.floor(Date.now() / 1000)},v1=${'f'.repeat(64)}` },
			body: '{"id":"evt_1","type":"checkout.session.completed"}'
		});
		const res401 = await stripeWebhookPost({
			request: badSigReq,
			platform: { env: { STRIPE_WEBHOOK_SECRET: secret } }
		});
		assert.equal(res401.status, 401);

		const staleTs = Math.floor(Date.now() / 1000) - 305;
		const staleBody = JSON.stringify({ id: 'evt_stale', type: 'checkout.session.completed' });
		const staleSig = await signPayload(staleBody, secret, staleTs);
		const staleReq = new Request('https://beercade.test/api/webhooks/stripe', {
			method: 'POST',
			headers: { 'stripe-signature': staleSig },
			body: staleBody
		});
		const resStale401 = await stripeWebhookPost({
			request: staleReq,
			platform: { env: { STRIPE_WEBHOOK_SECRET: secret } }
		});
		assert.equal(resStale401.status, 401);
	} finally {
		if (prevSecret !== undefined) process.env.STRIPE_WEBHOOK_SECRET = prevSecret;
	}
});

test('checkout.session.completed mints BRC-XXXX, promotes PENDING_PAYMENT -> ISSUED, writes KV, and calls B.O.S.S. once (deduped on replay)', async (t) => {
	const secret = 'whsec_fulfillment_test';
	const kv = createMockKV();
	const db = createMockD1({
		vouchers: [
			{
				voucher_id: 'ASTRO-SQ-ABCD-1234',
				stripe_session_id: 'cs_test_pass_1',
				status: 'PENDING_PAYMENT',
				session_code: null
			}
		]
	});

	const bossCalls = [];
	const origFetch = globalThis.fetch;
	t.after(() => {
		globalThis.fetch = origFetch;
	});
	globalThis.fetch = async (url, init) => {
		bossCalls.push({
			url: String(url),
			method: init?.method,
			headers: init?.headers,
			body: init?.body ? JSON.parse(String(init.body)) : null
		});
		return new Response(JSON.stringify({ ok: true }), { status: 200 });
	};

	const eventPayload = {
		id: 'evt_pass_completed_1',
		type: 'checkout.session.completed',
		data: {
			object: {
				id: 'cs_test_pass_1',
				customer_email: 'player@beercade.com.au',
				metadata: {
					voucher_id: 'ASTRO-SQ-ABCD-1234',
					duration_mins: '60',
					total_tokens: '25',
					target_node_id: 'ASTRO-02'
				}
			}
		}
	};
	const rawBody = JSON.stringify(eventPayload);
	const sig = await signPayload(rawBody, secret, Math.floor(Date.now() / 1000));

	const platform = {
		env: {
			STRIPE_WEBHOOK_SECRET: secret,
			DB: db,
			TELEMETRY_KV: kv,
			BOSS_URL: 'https://boss.beercade.internal/',
			BOSS_RPC_TOKEN: 'rpc_secret_token_99'
		}
	};

	// First delivery
	const req1 = new Request('https://beercade.test/api/webhooks/stripe', {
		method: 'POST',
		headers: { 'stripe-signature': sig },
		body: rawBody
	});
	const res1 = await stripeWebhookPost({ request: req1, platform });
	assert.equal(res1.status, 200);
	const body1 = await res1.json();

	assert.equal(body1.ok, true);
	assert.equal(body1.deduped, false);
	assert.equal(body1.voucher_activated, true);
	assert.equal(body1.boss_synced, true);
	assert.match(body1.session_code, /^BRC-[ABCDEFGHJKLMNPQRSTUVWXYZ23456789]{4}$/);

	// D1 row promoted and session_code persisted
	assert.equal(db.voucherRows[0].status, 'ISSUED');
	assert.equal(db.voucherRows[0].session_code, body1.session_code);

	// KV session:code:BRC-* cached with 86400s TTL
	const kvEntry = kv.store.get(`session:code:${body1.session_code}`);
	assert.ok(kvEntry, 'Expected session:code:BRC-* entry in KV');
	assert.equal(kvEntry.expirationTtl, 86400);
	const cachedSession = JSON.parse(kvEntry.value);
	assert.equal(cachedSession.session_code, body1.session_code);
	assert.equal(cachedSession.voucher_id, 'ASTRO-SQ-ABCD-1234');
	assert.equal(cachedSession.node_id, 'ASTRO-02');
	assert.equal(cachedSession.duration_mins, 60);
	assert.equal(cachedSession.credits, 25);

	// B.O.S.S. called exactly once with Bearer token & session_code
	assert.equal(bossCalls.length, 1);
	assert.equal(bossCalls[0].url, 'https://boss.beercade.internal/api/v1/plugins/pos-payments/action');
	assert.equal(bossCalls[0].headers.Authorization, 'Bearer rpc_secret_token_99');
	assert.deepEqual(bossCalls[0].body, {
		action: 'activate_game_time_pass',
		payload: {
			node_id: 'ASTRO-02',
			pass_code: 'ASTRO-SQ-ABCD-1234',
			session_code: body1.session_code,
			duration_mins: 60,
			credits: 25,
			provider: 'Stripe Checkout'
		}
	});

	// Replay with same event.id -> deduped, ZERO additional B.O.S.S. calls
	const req2 = new Request('https://beercade.test/api/webhooks/stripe', {
		method: 'POST',
		headers: { 'stripe-signature': sig },
		body: rawBody
	});
	const res2 = await stripeWebhookPost({ request: req2, platform });
	assert.equal(res2.status, 200);
	const body2 = await res2.json();
	assert.deepEqual(body2, {
		ok: true,
		deduped: true,
		event_id: 'evt_pass_completed_1'
	});
	assert.equal(bossCalls.length, 1);
});

test('KV-only idempotency gate deduplicates replayed event.id when DB is absent', async () => {
	const kv = createMockKV();
	const bossCalls = [];
	const mockFetch = async (url, init) => {
		bossCalls.push({ url: String(url), body: JSON.parse(String(init.body)) });
		return new Response('{}', { status: 200 });
	};

	const event = {
		id: 'evt_kv_only_1',
		type: 'checkout.session.completed',
		data: {
			object: {
				id: 'cs_kv_1',
				metadata: {
					voucher_id: 'BK-VCH-KV-1',
					duration_mins: '',
					total_tokens: '55',
					target_node_id: 'ASTRO-04'
				}
			}
		}
	};

	const first = await processStripeWebhookEvent(
		event,
		{ TELEMETRY_KV: kv, BOSS_URL: 'https://boss.local', BOSS_RPC_TOKEN: 'tok' },
		mockFetch
	);
	assert.equal(first.deduped, false);
	assert.equal(first.boss_synced, true);
	assert.equal(bossCalls.length, 1);
	assert.equal(bossCalls[0].url, 'https://boss.local/api/v1/credits/add');
	assert.deepEqual(bossCalls[0].body, { node_id: 'ASTRO-04', credits: 55 });

	const second = await processStripeWebhookEvent(
		event,
		{ TELEMETRY_KV: kv, BOSS_URL: 'https://boss.local', BOSS_RPC_TOKEN: 'tok' },
		mockFetch
	);
	assert.equal(second.deduped, true);
	assert.equal(bossCalls.length, 1);
});

test('booking checkout.session.completed promotes function_bookings PENDING_STRIPE -> CONFIRMED and expired marks EXPIRED', async () => {
	const db = createMockD1({
		vouchers: [
			{
				voucher_id: 'BK-VCH-EXP-1',
				stripe_session_id: 'cs_expired_1',
				status: 'PENDING_PAYMENT',
				session_code: null
			}
		],
		bookings: [
			{
				booking_ref: 'BK-RES-9001',
				stripe_session_id: 'cs_booking_1',
				status: 'PENDING_STRIPE'
			}
		]
	});

	const bookingRes = await processStripeWebhookEvent(
		{
			id: 'evt_booking_1',
			type: 'checkout.session.completed',
			data: {
				object: {
					id: 'cs_booking_1',
					metadata: { booking_ref: 'BK-RES-9001' }
				}
			}
		},
		{ DB: db }
	);
	assert.equal(bookingRes.ok, true);
	assert.equal(bookingRes.booking_confirmed, true);
	assert.equal(db.bookingRows[0].status, 'CONFIRMED');

	await processStripeWebhookEvent(
		{
			id: 'evt_voucher_expired_1',
			type: 'checkout.session.expired',
			data: {
				object: {
					id: 'cs_expired_1',
					metadata: { voucher_id: 'BK-VCH-EXP-1' }
				}
			}
		},
		{ DB: db }
	);
	assert.equal(db.voucherRows[0].status, 'EXPIRED');
});

test('generateSessionCode emits unambiguous BRC-XXXX codes', () => {
	for (let i = 0; i < 20; i++) {
		const code = generateSessionCode();
		assert.match(code, /^BRC-[ABCDEFGHJKLMNPQRSTUVWXYZ23456789]{4}$/);
	}
});
