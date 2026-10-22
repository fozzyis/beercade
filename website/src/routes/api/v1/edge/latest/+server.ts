import { json, error } from '@sveltejs/kit';
import type { RequestHandler } from '@sveltejs/kit';
import type { NodeTelemetry, VenueLatestSnapshot } from '$lib/types/telemetry';

/**
 * Cloudflare Worker endpoint: GET /api/v1/edge/latest
 *
 * Reads the latest venue/node snapshot cached in KV by the ingest endpoint.
 * Query params:
 *   ?node=ASTRO-01  -> latest telemetry for a single node
 *   ?venue=<name>   -> snapshot for a specific venue (defaults to the primary)
 */

const DEFAULT_VENUE = 'Beercade Redfern (The BK)';

export const GET: RequestHandler = async ({ platform, url }) => {
	const env = platform?.env;
	if (!env?.TELEMETRY_KV) {
		throw error(500, 'TELEMETRY_KV binding is not configured');
	}

	const nodeParam = url.searchParams.get('node');
	if (nodeParam) {
		const raw = await env.TELEMETRY_KV.get(`latest:node:${nodeParam}`);
		if (!raw) throw error(404, `No cached telemetry for node '${nodeParam}'`);
		return json(JSON.parse(raw) as NodeTelemetry);
	}

	const venueParam = url.searchParams.get('venue') ?? DEFAULT_VENUE;
	const raw = await env.TELEMETRY_KV.get(`latest:venue:${venueParam}`);
	if (!raw) throw error(404, `No cached telemetry for venue '${venueParam}'`);
	return json(JSON.parse(raw) as VenueLatestSnapshot);
};
