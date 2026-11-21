import type { RequestHandler } from '@sveltejs/kit';
import {
	processStripeWebhookEvent,
	verifyStripeSignature,
	type StripeWebhookEnv,
	type StripeWebhookEvent
} from '../../../../lib/stripe.ts';

function readEnvString(
	platformVal: string | undefined,
	processKey: string
): string | undefined {
	if (platformVal && platformVal.trim()) return platformVal.trim();
	if (typeof process !== 'undefined' && process.env?.[processKey]?.trim()) {
		return process.env[processKey]!.trim();
	}
	return undefined;
}

export const POST: RequestHandler = async ({ request, platform }) => {
	const webhookSecret = (
		readEnvString(platform?.env?.STRIPE_WEBHOOK_SECRET, 'STRIPE_WEBHOOK_SECRET') || ''
	).trim();
	if (!webhookSecret) {
		return Response.json(
			{ error: 'Stripe webhook secret is not configured' },
			{ status: 503 }
		);
	}

	const signatureHeader = request.headers.get('stripe-signature');
	if (!signatureHeader || !signatureHeader.trim()) {
		return Response.json({ error: 'Missing stripe-signature header' }, { status: 400 });
	}

	const rawBody = await request.text();
	const verified = await verifyStripeSignature(rawBody, signatureHeader, webhookSecret);
	if (!verified) {
		return Response.json({ error: 'Invalid Stripe webhook signature' }, { status: 401 });
	}

	let event: StripeWebhookEvent;
	try {
		event = JSON.parse(rawBody) as StripeWebhookEvent;
	} catch {
		return Response.json({ error: 'Invalid JSON payload' }, { status: 400 });
	}

	if (
		!event ||
		typeof event.id !== 'string' ||
		!event.id.trim() ||
		typeof event.type !== 'string' ||
		!event.type.trim()
	) {
		return Response.json({ error: 'Missing required Stripe event id or type' }, { status: 400 });
	}

	const env: StripeWebhookEnv = {
		DB: platform?.env?.DB,
		TELEMETRY_KV: platform?.env?.TELEMETRY_KV,
		BOSS_URL: readEnvString(platform?.env?.BOSS_URL, 'BOSS_URL'),
		BOSS_RPC_TOKEN: readEnvString(platform?.env?.BOSS_RPC_TOKEN, 'BOSS_RPC_TOKEN')
	};

	const result = await processStripeWebhookEvent(event, env);
	return Response.json(result, { status: 200 });
};
