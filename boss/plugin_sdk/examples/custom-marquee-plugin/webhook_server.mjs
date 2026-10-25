#!/usr/bin/env node
/**
 * B.O.S.S. Plugin SDK — Reference Webhook Receiver (`custom-marquee-sync`)
 *
 * Run with: `node webhook_server.mjs`
 * Listens on http://127.0.0.1:9100/boss-webhook for B.O.S.S. event hooks.
 */

import http from 'node:http';

const PORT = Number(process.env.PORT || 9100);

const server = http.createServer((req, res) => {
	if (req.method === 'POST' && req.url === '/boss-webhook') {
		let raw = '';
		req.on('data', (chunk) => {
			raw += chunk;
		});
		req.on('end', () => {
			try {
				const delivery = JSON.parse(raw);
				const { plugin_id, event, dispatched_at_epoch_ms } = delivery;
				console.log(
					`[${new Date(dispatched_at_epoch_ms).toISOString()}] Plugin=${plugin_id} Event=${event?.kind}`,
					JSON.stringify(event?.data)
				);
				res.writeHead(200, { 'Content-Type': 'application/json' });
				res.end(JSON.stringify({ ok: true, received_kind: event?.kind }));
			} catch (err) {
				res.writeHead(400, { 'Content-Type': 'application/json' });
				res.end(JSON.stringify({ ok: false, error: String(err) }));
			}
		});
		return;
	}

	res.writeHead(200, { 'Content-Type': 'application/json' });
	res.end(JSON.stringify({ service: 'custom-marquee-sync', status: 'listening' }));
});

server.listen(PORT, '0.0.0.0', () => {
	console.log(`B.O.S.S. Reference Webhook Plugin listening on http://127.0.0.1:${PORT}/boss-webhook`);
});
