import { json, error } from '@sveltejs/kit';
import type { RequestHandler } from '@sveltejs/kit';
import type { EdgeBatchEnvelope, EdgeIngestResult } from '$lib/types/telemetry';

/**
 * Cloudflare Worker endpoint: POST /api/v1/edge/ingest
 *
 * Ingests batched telemetry from B.O.S.S. (pushed over the Cloudflare Tunnel)
 * into D1 (historical analytics) and KV (latest-state snapshots).
 *
 * Auth: `Authorization: Bearer <EDGE_INGEST_TOKEN>` (or `X-BOSS-Token` header).
 */

const KV_TTL_SECONDS = 300;
const MAX_NODES_PER_BATCH = 500;
const MAX_AUX_ITEMS_PER_BATCH = 500;
/** D1 batch has a bound on statements per call — stay comfortably under it. */
const D1_BATCH_CHUNK = 64;

/** Constant-time string comparison to avoid trivial timing oracles. */
function tokensMatch(a: string, b: string): boolean {
	if (a.length !== b.length) return false;
	let diff = 0;
	for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
	return diff === 0;
}

function extractToken(request: Request, env: Env): string | null {
	const authHeader = request.headers.get('authorization');
	const bearer = authHeader?.startsWith('Bearer ') ? authHeader.slice(7).trim() : null;
	const headerToken = request.headers.get('x-boss-token')?.trim() ?? null;
	const expected = env.EDGE_INGEST_TOKEN?.trim() ?? '';
	if (!expected) return null; // fail closed when unset
	if (bearer && tokensMatch(bearer, expected)) return bearer;
	if (headerToken && tokensMatch(headerToken, expected)) return headerToken;
	return null;
}

/** Structural validation without pulling a schema library into the bundle. */
function validateEnvelope(body: unknown): body is EdgeBatchEnvelope {
	if (typeof body !== 'object' || body === null) return false;
	const env = body as Record<string, unknown>;
	return (
		typeof env.batch_id === 'string' &&
		env.batch_id.length > 0 &&
		typeof env.venue === 'string' &&
		typeof env.generated_at_epoch_ms === 'number' &&
		Array.isArray(env.nodes) &&
		env.nodes.length <= MAX_NODES_PER_BATCH &&
		Array.isArray(env.errors) &&
		env.errors.length <= MAX_AUX_ITEMS_PER_BATCH &&
		Array.isArray(env.transactions) &&
		env.transactions.length <= MAX_AUX_ITEMS_PER_BATCH &&
		Array.isArray(env.scores) &&
		env.scores.length <= MAX_AUX_ITEMS_PER_BATCH
	);
}

/** Split statements into chunks D1 accepts and run them sequentially. */
async function runChunked(db: D1Database, statements: D1PreparedStatement[]): Promise<void> {
	for (let i = 0; i < statements.length; i += D1_BATCH_CHUNK) {
		const chunk = statements.slice(i, i + D1_BATCH_CHUNK);
		if (chunk.length > 0) await db.batch(chunk);
	}
}

export const POST: RequestHandler = async ({ request, platform }) => {
	const env = platform?.env;
	if (!env?.DB || !env?.TELEMETRY_KV) {
		throw error(500, 'Edge store bindings (DB / TELEMETRY_KV) are not configured');
	}

	if (!extractToken(request, env)) {
		throw error(401, 'Invalid or missing ingest token');
	}

	let body: unknown;
	try {
		body = await request.json();
	} catch {
		throw error(400, 'Malformed JSON body');
	}
	if (!validateEnvelope(body)) {
		throw error(422, 'Envelope failed validation (check batch_id, venue and array limits)');
	}
	const batch = body;

	// Idempotency: BOSS retries failed flushes, so skip already-seen batches.
	const dedupeKey = `batch:seen:${batch.batch_id}`;
	const alreadySeen = await env.TELEMETRY_KV.get(dedupeKey);
	if (alreadySeen === '1') {
		const result: EdgeIngestResult = {
			ok: true,
			batch_id: batch.batch_id,
			deduped: true,
			accepted: { nodes: 0, errors: 0, transactions: 0, scores: 0 }
		};
		return json(result);
	}

	const now = Date.now();
	const statements: D1PreparedStatement[] = [];

	for (const n of batch.nodes) {
		if (!n?.node_id) continue;
		statements.push(
			env.DB.prepare(
				`INSERT INTO machine_analytics
					(venue_id, node_id, hardware, soc, cpu_temp_c, gpu_temp_c, npu_load_pct,
					 active_core, active_rom, frame_time_ms, fps,
					 jitter_poll_hz, jitter_p50_us, jitter_p95_us, jitter_p99_us, jitter_max_us, jitter_dropped,
					 netplay_peer_id, netplay_rtt_ms, netplay_jitter_ms, netplay_loss_pct, netplay_rollback,
					 credits_available, credits_total, credits_today, captured_at_ms, ingested_at_ms)
				 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
			).bind(
				batch.venue,
				n.node_id,
				n.hardware ?? null,
				n.soc ?? null,
				n.cpu_temp_c ?? null,
				n.gpu_temp_c ?? null,
				n.npu_load_pct ?? null,
				n.active_core ?? null,
				n.active_rom ?? null,
				n.frame_time_ms ?? null,
				n.fps ?? null,
				n.input_jitter?.poll_rate_hz ?? null,
				n.input_jitter?.p50_us ?? null,
				n.input_jitter?.p95_us ?? null,
				n.input_jitter?.p99_us ?? null,
				n.input_jitter?.max_us ?? null,
				n.input_jitter?.dropped_events ?? null,
				n.netplay?.peer_id ?? null,
				n.netplay?.rtt_ms ?? null,
				n.netplay?.jitter_ms ?? null,
				n.netplay?.packet_loss_pct ?? null,
				n.netplay?.rollback_frames ?? null,
				n.credits?.credits_available ?? null,
				n.credits?.credits_inserted_total ?? null,
				n.credits?.credits_dispensed_today ?? null,
				n.timestamp_epoch_ms,
				now
			)
		);
	}

	for (const e of batch.errors) {
		if (!e?.node_id || !e?.message) continue;
		statements.push(
			env.DB.prepare(
				`INSERT INTO error_logs
					(venue_id, node_id, level, source, message, detail, occurred_at_ms, ingested_at_ms)
				 VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
			).bind(
				batch.venue,
				e.node_id,
				e.level ?? 'error',
				e.source ?? null,
				e.message,
				e.detail ? JSON.stringify(e.detail) : null,
				e.occurred_at_epoch_ms,
				now
			)
		);
	}

	for (const t of batch.transactions) {
		if (!t?.node_id) continue;
		statements.push(
			env.DB.prepare(
				`INSERT INTO token_transactions
					(venue_id, node_id, kind, credits, balance_after, command_id, created_at_ms, ingested_at_ms)
				 VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
			).bind(
				batch.venue,
				t.node_id,
				t.kind ?? 'add',
				t.credits ?? 0,
				t.balance_after ?? null,
				t.command_id ?? null,
				t.created_at_epoch_ms,
				now
			)
		);
	}

	for (const s of batch.scores) {
		if (!s?.node_id || !s?.active_rom) continue;
		statements.push(
			env.DB.prepare(
				`INSERT OR IGNORE INTO high_scores
					(node_id, active_core, active_rom, game_name, score, initials, achieved_at_ms, ingested_at_ms)
				 VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
			).bind(
				s.node_id,
				s.active_core ?? 'unknown',
				s.active_rom,
				s.game_name ?? s.active_rom,
				s.score ?? 0,
				(s.initials ?? '???').slice(0, 3).toUpperCase(),
				s.achieved_at_epoch_ms,
				now
			)
		);
	}

	await runChunked(env.DB, statements);

	// Latest-state KV snapshots (with TTL so stale nodes age out gracefully).
	const snapshot = {
		venue: batch.venue,
		generated_at_epoch_ms: batch.generated_at_epoch_ms,
		nodes: batch.nodes
	};
	const kvWrites: Promise<unknown>[] = [
		env.TELEMETRY_KV.put(`latest:venue:${batch.venue}`, JSON.stringify(snapshot), {
			expirationTtl: KV_TTL_SECONDS
		}),
		env.TELEMETRY_KV.put(dedupeKey, '1', { expirationTtl: 86_400 })
	];
	for (const n of batch.nodes) {
		if (!n?.node_id) continue;
		kvWrites.push(
			env.TELEMETRY_KV.put(`latest:node:${n.node_id}`, JSON.stringify(n), {
				expirationTtl: KV_TTL_SECONDS
			})
		);
	}
	await Promise.all(kvWrites);

	const result: EdgeIngestResult = {
		ok: true,
		batch_id: batch.batch_id,
		deduped: false,
		accepted: {
			nodes: batch.nodes.length,
			errors: batch.errors.length,
			transactions: batch.transactions.length,
			scores: batch.scores.length
		}
	};
	return json(result);
};
