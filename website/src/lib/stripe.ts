export interface D1PreparedLike {
	bind(...values: unknown[]): D1PreparedLike;
	run(): Promise<{ meta?: { changes?: number } }>;
}

export interface D1DatabaseLike {
	prepare(query: string): D1PreparedLike;
}

export interface KVNamespaceLike {
	get(key: string): Promise<string | null>;
	put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
}

export interface StripeWebhookEnv {
	DB?: D1DatabaseLike;
	TELEMETRY_KV?: KVNamespaceLike;
	BOSS_URL?: string;
	BOSS_RPC_TOKEN?: string;
}

export interface StripeSessionObject {
	id?: string;
	metadata?: Record<string, string | undefined> | null;
	customer_details?: { email?: string | null } | null;
	customer_email?: string | null;
}

export interface StripeWebhookEvent {
	id: string;
	type: string;
	data?: {
		object?: StripeSessionObject;
	};
}

export interface StripeWebhookProcessResult {
	ok: true;
	deduped: boolean;
	event_id: string;
	event_type?: string;
	session_code?: string | null;
	voucher_activated?: boolean;
	booking_confirmed?: boolean;
	boss_synced?: boolean;
}

const SESSION_CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
const EVENT_DEDUPE_TTL_SECS = 86_400 * 7;
const SESSION_CODE_TTL_SECS = 86_400;

function hexToBytes(hex: string): Uint8Array | null {
	if (hex.length % 2 !== 0 || !/^[0-9a-fA-F]+$/.test(hex)) {
		return null;
	}
	const out = new Uint8Array(hex.length / 2);
	for (let i = 0; i < out.length; i++) {
		out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
	}
	return out;
}

function timingSafeEqual(a: Uint8Array, b: Uint8Array): boolean {
	if (a.length !== b.length) return false;
	let diff = 0;
	for (let i = 0; i < a.length; i++) {
		diff |= a[i] ^ b[i];
	}
	return diff === 0;
}

export async function verifyStripeSignature(
	rawBody: string,
	signatureHeader: string | null,
	secret: string,
	nowEpochSec = Math.floor(Date.now() / 1000),
	toleranceSecs = 300
): Promise<boolean> {
	if (!signatureHeader || !signatureHeader.trim() || !secret || !secret.trim()) {
		return false;
	}

	let timestamp: number | null = null;
	const v1Candidates: string[] = [];

	for (const part of signatureHeader.split(',')) {
		const eqIdx = part.indexOf('=');
		if (eqIdx <= 0) continue;
		const key = part.slice(0, eqIdx).trim();
		const value = part.slice(eqIdx + 1).trim();
		if (key === 't' && /^\d+$/.test(value)) {
			timestamp = Number(value);
		} else if (key === 'v1' && /^[0-9a-fA-F]{64}$/.test(value)) {
			v1Candidates.push(value.toLowerCase());
		}
	}

	if (timestamp === null || !Number.isSafeInteger(timestamp) || v1Candidates.length === 0) {
		return false;
	}
	if (Math.abs(nowEpochSec - timestamp) > toleranceSecs) {
		return false;
	}

	const encoder = new TextEncoder();
	const key = await crypto.subtle.importKey(
		'raw',
		encoder.encode(secret),
		{ name: 'HMAC', hash: 'SHA-256' },
		false,
		['sign']
	);
	const mac = await crypto.subtle.sign('HMAC', key, encoder.encode(`${timestamp}.${rawBody}`));
	const expectedBytes = new Uint8Array(mac);

	let matched = false;
	for (const candidateHex of v1Candidates) {
		const candidateBytes = hexToBytes(candidateHex);
		if (candidateBytes && timingSafeEqual(expectedBytes, candidateBytes)) {
			matched = true;
		}
	}
	return matched;
}

export function generateSessionCode(): string {
	const bytes = new Uint8Array(4);
	crypto.getRandomValues(bytes);
	let suffix = '';
	for (let i = 0; i < bytes.length; i++) {
		suffix += SESSION_CODE_ALPHABET[bytes[i] & 31];
	}
	return `BRC-${suffix}`;
}

export async function processStripeWebhookEvent(
	event: StripeWebhookEvent,
	env: StripeWebhookEnv,
	fetchImpl: typeof fetch = fetch
): Promise<StripeWebhookProcessResult> {
	const eventId = typeof event?.id === 'string' ? event.id.trim() : '';
	const eventType = typeof event?.type === 'string' ? event.type.trim() : '';
	if (!eventId || !eventType) {
		throw new Error('Stripe webhook event requires non-empty id and type');
	}

	const session = (event.data?.object ?? {}) as StripeSessionObject;
	const stripeSessionId =
		typeof session.id === 'string' && session.id.trim() ? session.id.trim() : null;
	const metadata = session.metadata ?? {};
	const now = Date.now();
	const outcome =
		eventType === 'checkout.session.completed' || eventType === 'checkout.session.expired'
			? 'PROCESSED'
			: 'IGNORED';

	if (env.DB) {
		const gateRes = await env.DB.prepare(
			`INSERT OR IGNORE INTO stripe_webhook_events
			 (event_id, event_type, stripe_session_id, outcome, processed_at_ms)
			 VALUES (?1, ?2, ?3, ?4, ?5)`
		)
			.bind(eventId, eventType, stripeSessionId, outcome, now)
			.run();
		if (gateRes?.meta?.changes === 0) {
			return { ok: true, deduped: true, event_id: eventId };
		}
		if (env.TELEMETRY_KV) {
			await env.TELEMETRY_KV.put(`stripe:evt:${eventId}`, '1', {
				expirationTtl: EVENT_DEDUPE_TTL_SECS
			});
		}
	} else if (env.TELEMETRY_KV) {
		const kvKey = `stripe:evt:${eventId}`;
		const existing = await env.TELEMETRY_KV.get(kvKey);
		if (existing !== null) {
			return { ok: true, deduped: true, event_id: eventId };
		}
		await env.TELEMETRY_KV.put(kvKey, '1', { expirationTtl: EVENT_DEDUPE_TTL_SECS });
	}

	let sessionCode: string | null = null;
	let activated = false;
	let bookingConfirmed = false;
	let bossSynced = false;

	if (eventType === 'checkout.session.completed') {
		const voucherId = typeof metadata?.voucher_id === 'string' ? metadata.voucher_id.trim() : '';
		if (voucherId) {
			sessionCode = generateSessionCode();
			const durationMins = Number(metadata?.duration_mins || 0);
			const totalTokens = Number(metadata?.total_tokens || 0);
			const rawTargetNode =
				typeof metadata?.target_node_id === 'string' ? metadata.target_node_id.trim() : '';
			const targetNodeId = rawTargetNode || 'ASTRO-01';

			if (env.DB) {
				const voucherRes = await env.DB.prepare(
					`UPDATE token_vouchers
					 SET status = 'ISSUED', session_code = ?1
					 WHERE (stripe_session_id = ?2 OR voucher_id = ?3) AND status = 'PENDING_PAYMENT'`
				)
					.bind(sessionCode, stripeSessionId, voucherId)
					.run();
				activated = (voucherRes?.meta?.changes ?? 0) > 0;
			}

			if (env.TELEMETRY_KV && (activated || !env.DB)) {
				await env.TELEMETRY_KV.put(
					`session:code:${sessionCode}`,
					JSON.stringify({
						session_code: sessionCode,
						voucher_id: voucherId,
						node_id: targetNodeId,
						duration_mins: durationMins,
						credits: totalTokens,
						issued_at_ms: now
					}),
					{ expirationTtl: SESSION_CODE_TTL_SECS }
				);
			}

			if ((activated || !env.DB) && env.BOSS_URL?.trim()) {
				const bossBase = env.BOSS_URL.trim().replace(/\/$/, '');
				const headers: Record<string, string> = {
					'Content-Type': 'application/json'
				};
				if (env.BOSS_RPC_TOKEN?.trim()) {
					headers['Authorization'] = `Bearer ${env.BOSS_RPC_TOKEN.trim()}`;
				}

				try {
					if (durationMins > 0) {
						const bossRes = await fetchImpl(
							`${bossBase}/api/v1/plugins/pos-payments/action`,
							{
								method: 'POST',
								headers,
								body: JSON.stringify({
									action: 'activate_game_time_pass',
									payload: {
										node_id: targetNodeId,
										pass_code: voucherId,
										session_code: sessionCode,
										duration_mins: durationMins,
										credits: totalTokens,
										provider: 'Stripe Checkout'
									}
								})
							}
						);
						bossSynced = bossRes.ok;
					} else if (totalTokens > 0 && rawTargetNode) {
						const bossRes = await fetchImpl(`${bossBase}/api/v1/credits/add`, {
							method: 'POST',
							headers,
							body: JSON.stringify({
								node_id: targetNodeId,
								credits: totalTokens
							})
						});
						bossSynced = bossRes.ok;
					}
				} catch (err) {
					console.warn('Stripe webhook B.O.S.S. fulfillment failed:', err);
				}
			}
		}

		const bookingRef = typeof metadata?.booking_ref === 'string' ? metadata.booking_ref.trim() : '';
		if (bookingRef && env.DB) {
			const bookingRes = await env.DB.prepare(
				`UPDATE function_bookings
				 SET status = 'CONFIRMED'
				 WHERE (stripe_session_id = ?1 OR booking_ref = ?2) AND status = 'PENDING_STRIPE'`
			)
				.bind(stripeSessionId, bookingRef)
				.run();
			bookingConfirmed = (bookingRes?.meta?.changes ?? 0) > 0;
		}
	} else if (eventType === 'checkout.session.expired' && env.DB) {
		const voucherId = typeof metadata?.voucher_id === 'string' ? metadata.voucher_id.trim() : '';
		if (voucherId || stripeSessionId) {
			await env.DB.prepare(
				`UPDATE token_vouchers
				 SET status = 'EXPIRED'
				 WHERE (stripe_session_id = ?1 OR voucher_id = ?2) AND status = 'PENDING_PAYMENT'`
			)
				.bind(stripeSessionId, voucherId || null)
				.run();
		}

		const bookingRef = typeof metadata?.booking_ref === 'string' ? metadata.booking_ref.trim() : '';
		if (bookingRef || stripeSessionId) {
			await env.DB.prepare(
				`UPDATE function_bookings
				 SET status = 'EXPIRED'
				 WHERE (stripe_session_id = ?1 OR booking_ref = ?2) AND status = 'PENDING_STRIPE'`
			)
				.bind(stripeSessionId, bookingRef || null)
				.run();
		}
	}

	return {
		ok: true,
		deduped: false,
		event_id: eventId,
		event_type: eventType,
		session_code: sessionCode,
		voucher_activated: activated,
		booking_confirmed: bookingConfirmed,
		boss_synced: bossSynced
	};
}
