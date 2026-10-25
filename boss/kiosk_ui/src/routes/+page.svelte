<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import PinKeypadModal from '$lib/components/PinKeypadModal.svelte';
	import {
		fetchPlugins,
		fetchVenueTelemetry,
		getApiBase,
		getStoredOperatorToken,
		invokePluginAction
	} from '$lib/bossApi';
	import type { NodeTelemetry, PluginDescriptor, VenueTelemetryReport } from '$lib/types/telemetry';

	interface FeaturedGame {
		id: string;
		title: string;
		genre: string;
		players: string;
		core: string;
		rom: string;
		accent: string;
	}

	const featuredGames: FeaturedGame[] = [
		{
			id: 'sfiii3',
			title: 'Street Fighter III: 3rd Strike',
			genre: '2D Fighter · CPS-3',
			players: '1–2P Rollback Netplay',
			core: 'fbneo_libretro',
			rom: 'sfiii3.zip',
			accent: '#00f0ff'
		},
		{
			id: 'mvc2',
			title: 'Marvel vs. Capcom 2',
			genre: '3v3 Tag Fighter · NAOMI',
			players: '1–2P Rollback Netplay',
			core: 'flycast_libretro',
			rom: 'mvc2.zip',
			accent: '#ff0055'
		},
		{
			id: 'mslugx',
			title: 'Metal Slug X',
			genre: 'Run & Gun · Neo-Geo MVS',
			players: '1–2P Co-Op',
			core: 'fbneo_libretro',
			rom: 'mslugX.zip',
			accent: '#ffbe0b'
		},
		{
			id: 'tmnt',
			title: 'TMNT: Turtles in Time',
			genre: '4P Beat-Em-Up · Konami',
			players: '1–4P Co-Op',
			core: 'mame2003_plus',
			rom: 'tmnt2.zip',
			accent: '#00f59b'
		},
		{
			id: 'outrun',
			title: 'OutRunners / Super Hang-On',
			genre: 'Arcade Racer · Sega System 32',
			players: '1–2P Twin Link',
			core: 'mame2003_plus',
			rom: 'orunners.zip',
			accent: '#a855f7'
		},
		{
			id: 'windjammers',
			title: 'Windjammers',
			genre: 'Sports Arcade · Neo-Geo MVS',
			players: '1–2P Versus',
			core: 'fbneo_libretro',
			rom: 'wjammers.zip',
			accent: '#38bdf8'
		}
	];

	const kioskMenuItems = [
		{ id: 'c1', name: 'Negroni 8-Bit', category: 'Cocktails', priceCents: 2100 },
		{ id: 'c2', name: 'Cyberpunk Espresso Martini', category: 'Cocktails', priceCents: 2200 },
		{ id: 't1', name: 'Redfern Hazy IPA Pint', category: 'Craft Taps', priceCents: 1400 },
		{ id: 't2', name: 'Regent St Lager Jug', category: 'Craft Taps', priceCents: 2600 },
		{ id: 'b1', name: 'Cheeseburger Sliders (3x)', category: 'Arcade Bites', priceCents: 1800 },
		{ id: 'b2', name: 'Woodfired Pepperoni Pizza', category: 'Arcade Bites', priceCents: 2400 }
	];

	let kioskMode = $state<'stations' | 'tokens' | 'timeslots' | 'food'>('stations');
	let report = $state<VenueTelemetryReport | null>(null);
	let plugins = $state<PluginDescriptor[]>([]);
	let selectedNodeId = $state<string>('ASTRO-01');
	let selectedPackId = $state<string>('pack-5');
	let activeQrSession = $state<any>(null);
	let rfidFeedback = $state<string>('');
	let pinModalOpen = $state(false);
	let operatorUnlocked = $state(false);

	// Time-slot booking & Square A.S.T.R.O. Game-Time Pass state
	let slotDurationMins = $state<number>(60);
	let slotPatronName = $state<string>('');
	let slotConfirmation = $state<string>('');
	let squarePassCodeInput = $state<string>('');

	// Food & beverage cart state
	let cart = $state<Record<string, number>>({});
	let fbOrderConfirmation = $state<string>('');

	// Hidden 5-tap corner gesture tracker
	let tapTimestamps = $state<number[]>([]);

	let nodes = $derived<NodeTelemetry[]>(report?.nodes ?? []);
	let tournamentPlugin = $derived(
		plugins.find((p) => p.manifest.id === 'tournament-engine' && p.manifest.enabled)
	);
	let posPlugin = $derived(plugins.find((p) => p.manifest.id === 'pos-payments'));
	let rfidPlugin = $derived(plugins.find((p) => p.manifest.id === 'rfid-peripherals'));
	let lightingPlugin = $derived(plugins.find((p) => p.manifest.id === 'venue-lighting'));

	let cartTotalCents = $derived(
		kioskMenuItems.reduce((sum, item) => sum + (cart[item.id] ?? 0) * item.priceCents, 0)
	);

	function handleSecretCornerTap() {
		const now = Date.now();
		const recent = [...tapTimestamps.filter((t) => now - t < 3000), now];
		tapTimestamps = recent;
		if (recent.length >= 5) {
			tapTimestamps = [];
			if (getStoredOperatorToken()) {
				goto('/admin');
			} else {
				pinModalOpen = true;
			}
		}
	}

	async function generateQrForNode() {
		rfidFeedback = '';
		const res = await invokePluginAction('pos-payments', 'create_qr_session', {
			node_id: selectedNodeId,
			pack_id: selectedPackId
		});
		if (res.ok) {
			activeQrSession = res.result ?? {
				session_id: `qr_${Date.now().toString(36)}`,
				node_id: selectedNodeId,
				pack_id: selectedPackId,
				credits: selectedPackId === 'pack-15' ? 15 : selectedPackId === 'pack-5' ? 5 : 1,
				amount_cents:
					selectedPackId === 'pack-15' ? 1200 : selectedPackId === 'pack-5' ? 450 : 100,
				qr_payload: `https://beercade.gg/tokens?node=${selectedNodeId}&pack=${selectedPackId}`
			};
		}
	}

	async function simulateWristbandTap(uid: string) {
		rfidFeedback = 'Processing NFC tap...';
		const res = await invokePluginAction('rfid-peripherals', 'tap_card', {
			uid,
			node_id: selectedNodeId
		});
		if (res.ok) {
			rfidFeedback = `✓ Injected credit onto ${selectedNodeId} via wristband ${uid}`;
			report = await fetchVenueTelemetry();
			plugins = await fetchPlugins();
		} else {
			rfidFeedback = `✕ ${res.detail || 'Wristband tap rejected'}`;
		}
	}

	async function handleBookTimeslot() {
		slotConfirmation = '';
		const res = await invokePluginAction('pos-payments', 'book_timeslot', {
			node_id: selectedNodeId,
			duration_mins: slotDurationMins,
			patron_name: slotPatronName.trim() || 'Kiosk Guest'
		});
		if (res.ok) {
			const ref = res.result?.booking_ref ?? `SLOT-${Date.now().toString(36).toUpperCase()}`;
			slotConfirmation = `✓ Square POS: Locked ${slotDurationMins}m A.S.T.R.O. Session on ${selectedNodeId} (Ref: ${ref})`;
			slotPatronName = '';
			plugins = await fetchPlugins();
		}
	}

	async function handleRedeemSquarePass() {
		rfidFeedback = '';
		const code = squarePassCodeInput.trim().toUpperCase() || `ASTRO-SQ-${Date.now().toString(36).slice(-4).toUpperCase()}`;
		const res = await invokePluginAction('pos-payments', 'activate_game_time_pass', {
			node_id: selectedNodeId,
			pass_code: code,
			duration_mins: 60,
			credits: 25,
			amount_cents: 2000,
			patron_name: slotPatronName.trim() || 'Square Online Player',
			provider: 'Square Online Game-Time Pass'
		});
		if (res.ok) {
			rfidFeedback = `✓ Activated Square Game-Time Pass ${code} on ${selectedNodeId} (60m + 25 Credits)!`;
			squarePassCodeInput = '';
			report = await fetchVenueTelemetry();
			plugins = await fetchPlugins();
		} else {
			rfidFeedback = `✕ ${res.detail || 'Could not redeem Square Game-Time Pass'}`;
		}
	}

	function updateCart(itemId: string, delta: number) {
		const next = Math.max(0, (cart[itemId] ?? 0) + delta);
		cart = { ...cart, [itemId]: next };
	}

	async function submitFbOrder() {
		if (cartTotalCents === 0) return;
		fbOrderConfirmation = '';
		const summary = kioskMenuItems
			.filter((i) => (cart[i.id] ?? 0) > 0)
			.map((i) => `${cart[i.id]}x ${i.name}`)
			.join(', ');
		const res = await invokePluginAction('pos-payments', 'submit_fb_order', {
			station: selectedNodeId,
			summary,
			amount_cents: cartTotalCents
		});
		if (res.ok) {
			const ref = res.result?.order_ref ?? `ORD-${Date.now().toString(36).toUpperCase()}`;
			fbOrderConfirmation = `✓ Sent to Bar POS (${ref}) — delivering to station ${selectedNodeId}!`;
			cart = {};
		}
	}

	onMount(() => {
		operatorUnlocked = Boolean(getStoredOperatorToken());

		(async () => {
			report = await fetchVenueTelemetry();
			plugins = await fetchPlugins();
			if (report.nodes.length > 0) {
				selectedNodeId = report.nodes[0].node_id;
			}
		})();

		const handleKey = (e: KeyboardEvent) => {
			if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'o') {
				e.preventDefault();
				pinModalOpen = true;
			}
		};
		window.addEventListener('keydown', handleKey);

		let es: EventSource | null = null;
		try {
			es = new EventSource(
				`${getApiBase()}/api/telemetry/stream?channels=venue,plugins,alerts`
			);
			es.addEventListener('venue', (ev) => {
				try {
					report = JSON.parse((ev as MessageEvent).data);
				} catch {
					// ignore parse error
				}
			});
			es.addEventListener('plugins', (ev) => {
				try {
					const data = JSON.parse((ev as MessageEvent).data);
					if (Array.isArray(data.plugins)) {
						plugins = data.plugins;
					} else {
						fetchPlugins().then((p) => (plugins = p));
					}
				} catch {
					// ignore
				}
			});
		} catch {
			// SSE optional when offline
		}

		return () => {
			window.removeEventListener('keydown', handleKey);
			es?.close();
		};
	});
</script>

<svelte:head>
	<title>B.O.S.S. Public Kiosk — B.E.E.R.C.A.D.E.</title>
</svelte:head>

<div class="kiosk-shell">
	<!-- Top Kiosk Bar with Hidden 5-Tap Operator Gesture -->
	<header class="kiosk-topbar">
		<div class="brand-cluster">
			<span class="pulse-dot"></span>
			<div>
				<div class="venue-tag">
					B.O.S.S. PUBLIC TOUCH KIOSK · {report?.venue ?? 'BEERCADE REDFERN (THE BK)'}
				</div>
				<h1 class="neon-title">B.E.E.R.C.A.D.E. STATION &amp; VENUE KIOSK</h1>
			</div>
		</div>

		<div class="topbar-actions">
			<a href="/tv" class="scene-chip" style="text-decoration: none;">
				<span>📺 TV ATTRACTOR:</span>
				<strong>OPEN /TV</strong>
			</a>

			{#if lightingPlugin}
				<div class="scene-chip">
					<span>LIGHTING:</span>
					<strong>{lightingPlugin.state.active_scene?.replace(/_/g, ' ').toUpperCase()}</strong>
				</div>
			{/if}

			{#if operatorUnlocked}
				<a href="/admin" class="operator-Return-pill">
					⚡ OPERATOR SESSION ACTIVE — OPEN /ADMIN
				</a>
			{/if}

			<!-- Hidden 5-tap operator unlock target -->
			<button
				type="button"
				class="status-corner-badge"
				onclick={handleSecretCornerTap}
				title="B.O.S.S. Fleet Status (5-tap for Operator PIN)"
			>
				<span class="online-count">{report?.nodes_online ?? 4} O.R.I.O.N. NODES ONLINE</span>
				{#if tapTimestamps.length >= 2}
					<span class="tap-counter">{tapTimestamps.length}/5</span>
				{/if}
			</button>
		</div>
	</header>

	<!-- Live Tournament Attractor Banner (controlled by tournament-engine plugin) -->
	{#if tournamentPlugin && tournamentPlugin.state.kiosk_overlay_enabled}
		<section class="tournament-banner">
			<div class="tournament-meta">
				<span class="live-pill">🏆 LIVE VENUE TOURNAMENT</span>
				<h2>{tournamentPlugin.state.title}</h2>
				<p>
					Game: <strong>{tournamentPlugin.state.game_name}</strong> · Prize Pool:
					<span class="prize">{tournamentPlugin.state.prize_pool}</span>
				</p>
			</div>

			<div class="tournament-standings">
				{#each (tournamentPlugin.state.leaderboard ?? []).slice(0, 3) as row, idx}
					<div class="standing-card rank-{idx + 1}">
						<span class="rank-badge">#{idx + 1}</span>
						<div class="standing-info">
							<strong>{row.player_name} ({row.initials})</strong>
							<span>{Number(row.score).toLocaleString()} PTS · {row.node_id}</span>
						</div>
					</div>
				{/each}
			</div>
		</section>
	{/if}

	<!-- 4 Touchscreen Kiosk Mode Tabs (Stations · Tokens · Time-Slots · Food/Drinks) -->
	<nav class="kiosk-mode-bar">
		<button
			type="button"
			class="kiosk-tab {kioskMode === 'stations' ? 'active' : ''}"
			onclick={() => (kioskMode = 'stations')}
		>
			🕹️ 1. ARCADE STATION ACCESS
		</button>
		<button
			type="button"
			class="kiosk-tab {kioskMode === 'tokens' ? 'active' : ''}"
			onclick={() => (kioskMode = 'tokens')}
		>
			🪙 2. TOKEN BALANCE &amp; TOP-UP
		</button>
		<button
			type="button"
			class="kiosk-tab {kioskMode === 'timeslots' ? 'active' : ''}"
			onclick={() => (kioskMode = 'timeslots')}
		>
			⏱️ 3. TIME-SLOT BOOKINGS
		</button>
		<button
			type="button"
			class="kiosk-tab {kioskMode === 'food' ? 'active' : ''}"
			onclick={() => (kioskMode = 'food')}
		>
			🍕 4. FOOD &amp; BEVERAGE ORDERING
		</button>
	</nav>

	<div class="main-columns">
		<!-- Left Column: Active Kiosk Workflow -->
		<div class="catalog-column">
			<!-- Always-visible O.R.I.O.N. Station Picker -->
			<section class="panel-block">
				<div class="section-head">
					<h2>SELECT O.R.I.O.N. ARCADE STATION</h2>
					<span class="sub">
						RK3588 / RK3588S2 Client Compute Nodes · A.S.T.R.O. Direct-to-KMS/DRM Runtime
					</span>
				</div>

				<div class="cabinet-grid">
					{#each nodes as node}
						<button
							type="button"
							class="cabinet-card {selectedNodeId === node.node_id ? 'selected' : ''}"
							onclick={() => {
								selectedNodeId = node.node_id;
								activeQrSession = null;
							}}
						>
							<div class="cab-top">
								<span class="cab-id">{node.node_id}</span>
								<span class="credit-pill">
									{node.credits.credits_available} CREDITS
								</span>
							</div>
							<div class="cab-hw">{node.hardware}</div>
							<div class="cab-rom">
								NOW RUNNING: <strong>{node.active_rom.replace('.zip', '').toUpperCase()}</strong>
							</div>
							<div class="cab-stats">
								<span>{node.fps.toFixed(1)} FPS</span>
								<span>1kHz p99: {node.input_jitter.p99_us.toFixed(0)}μs</span>
								{#if node.netplay}
									<span class="netplay-tag">P2P LINKED ({node.netplay.peer_id})</span>
								{/if}
							</div>
						</button>
					{/each}
				</div>
			</section>

			{#if kioskMode === 'stations' || kioskMode === 'tokens'}
				<section class="panel-block">
					<div class="section-head">
						<h2>A.S.T.R.O. HARDWARE-ACCELERATED GAME ROTATION</h2>
						<span class="sub">Zero X11/Wayland Overhead · 1000Hz Polling · Rollback Netplay</span>
					</div>

					<div class="games-grid">
						{#each featuredGames as game}
							<div class="game-tile" style="--accent: {game.accent}">
								<div class="game-genre">{game.genre}</div>
								<h3>{game.title}</h3>
								<div class="game-footer">
									<span>{game.players}</span>
									<span class="rom-tag">{game.rom}</span>
								</div>
							</div>
						{/each}
					</div>
				</section>
			{:else if kioskMode === 'timeslots'}
				<section class="panel-block">
					<div class="section-head">
						<h2>BOOK A MANAGED STATION TIME-SLOT ({selectedNodeId})</h2>
						<span class="sub">
							Locks {selectedNodeId} into A.S.T.R.O. Managed Session Mode with unlimited play
						</span>
					</div>

					<div class="slot-grid">
						{#each [
							{ mins: 30, label: '30 MIN SPRINT', price: '$10.00' },
							{ mins: 60, label: '60 MIN POWER HOUR', price: '$20.00' },
							{ mins: 120, label: '120 MIN SQUAD LOCK', price: '$35.00' }
						] as tier}
							<button
								type="button"
								class="slot-card {slotDurationMins === tier.mins ? 'active' : ''}"
								onclick={() => (slotDurationMins = tier.mins)}
							>
								<strong>{tier.label}</strong>
								<span class="slot-price">{tier.price}</span>
							</button>
						{/each}
					</div>

					<div class="slot-form">
						<input
							type="text"
							placeholder="Your Name / Handle (e.g. ALEX)"
							bind:value={slotPatronName}
							class="kiosk-input"
						/>
						<button type="button" class="generate-qr-btn" onclick={handleBookTimeslot}>
							LOCK {slotDurationMins}-MINUTE TIME-SLOT ON {selectedNodeId}
						</button>
					</div>

					{#if slotConfirmation}
						<div class="rfid-feedback">{slotConfirmation}</div>
					{/if}
				</section>
			{:else if kioskMode === 'food'}
				<section class="panel-block">
					<div class="section-head">
						<h2>ORDER DRINKS &amp; BITES TO STATION {selectedNodeId}</h2>
						<span class="sub">Prepared at the main bar &amp; delivered straight to your cabinet</span>
					</div>

					<div class="games-grid">
						{#each kioskMenuItems as item}
							<div class="menu-order-tile">
								<div>
									<div class="game-genre">{item.category}</div>
									<h3>{item.name}</h3>
									<strong class="menu-price">${(item.priceCents / 100).toFixed(2)}</strong>
								</div>
								<div class="qty-controls">
									<button type="button" onclick={() => updateCart(item.id, -1)}>-</button>
									<span>{cart[item.id] ?? 0}</span>
									<button type="button" onclick={() => updateCart(item.id, 1)}>+</button>
								</div>
							</div>
						{/each}
					</div>

					<div class="fb-checkout-bar">
						<div>
							<span>DELIVER TO: <strong>{selectedNodeId}</strong></span>
							<strong class="cart-total">TOTAL: ${(cartTotalCents / 100).toFixed(2)}</strong>
						</div>
						<button
							type="button"
							class="generate-qr-btn"
							disabled={cartTotalCents === 0}
							onclick={submitFbOrder}
						>
							SEND ORDER TO BAR POS →
						</button>
					</div>

					{#if fbOrderConfirmation}
						<div class="rfid-feedback">{fbOrderConfirmation}</div>
					{/if}
				</section>
			{/if}
		</div>

		<!-- Right Column: Square POS Checkout, Online Game-Time Pass & NFC Wristband Station -->
		<aside class="payment-column">
			<div class="pay-card">
				<span class="step-tag">STATION SELECTED: {selectedNodeId} · POWERED BY SQUARE POS</span>
				<h2>SQUARE TOKENS &amp; A.S.T.R.O. GAME TIME</h2>
				<p class="pay-sub">
					Buy credits or timed A.S.T.R.O. passes via Square Terminal / QR, or redeem an online Square Pass
				</p>

				<div class="pack-selector">
					{#each posPlugin?.state?.tiers ?? [
						{ id: 'pack-1', label: 'Single Drop', credits: 1, price_cents: 100 },
						{ id: 'pack-5', label: 'Squad 5-Pack', credits: 5, price_cents: 450, badge: 'POPULAR' },
						{ id: 'pack-15', label: 'Arcade Boss 15-Pack', credits: 15, price_cents: 1200, badge: 'BEST VALUE' }
					] as tier}
						<button
							type="button"
							class="pack-btn {selectedPackId === tier.id ? 'active' : ''}"
							onclick={() => {
								selectedPackId = tier.id;
								generateQrForNode();
							}}
						>
							<div>
								<strong>{tier.credits} {tier.credits === 1 ? 'CREDIT' : 'CREDITS'}</strong>
								<span>{tier.label}</span>
							</div>
							<div class="pack-price">
								{#if tier.badge}
									<small class="tier-badge">{tier.badge}</small>
								{/if}
								<strong>${(tier.price_cents / 100).toFixed(2)}</strong>
							</div>
						</button>
					{/each}

					{#each posPlugin?.state?.game_time_tiers ?? [
						{ id: 'astro-30m', label: '30-Min A.S.T.R.O. Session', duration_mins: 30, included_credits: 10, price_cents: 1200, badge: 'QUICK PLAY' },
						{ id: 'astro-60m', label: '60-Min Unlimited A.S.T.R.O. Pass', duration_mins: 60, included_credits: 25, price_cents: 2000, badge: 'SQUARE PASS' }
					] as gt}
						<button
							type="button"
							class="pack-btn {selectedPackId === gt.id ? 'active' : ''}"
							onclick={() => {
								selectedPackId = gt.id;
								generateQrForNode();
							}}
						>
							<div>
								<strong>{gt.duration_mins} MIN A.S.T.R.O. PASS</strong>
								<span>{gt.label} (+{gt.included_credits} CR)</span>
							</div>
							<div class="pack-price">
								{#if gt.badge}
									<small class="tier-badge">{gt.badge}</small>
								{/if}
								<strong>${(gt.price_cents / 100).toFixed(2)}</strong>
							</div>
						</button>
					{/each}
				</div>

				<div class="qr-stage">
					<div class="qr-matrix">
						<div class="qr-inner">
							<span class="qr-title">SQUARE QR</span>
							<strong>{selectedNodeId}</strong>
							<small>{activeQrSession?.session_id ?? 'SELECT TIER ABOVE'}</small>
						</div>
					</div>
					<button type="button" class="generate-qr-btn" onclick={generateQrForNode}>
						GENERATE SQUARE QR / TERMINAL CHECKOUT
					</button>
				</div>

				<!-- Online Square A.S.T.R.O. Game-Time Pass Redemption -->
				<div class="rfid-box" style="margin-bottom: 0.9rem;">
					<div class="rfid-head">
						<span>🎟️ REDEEM ONLINE SQUARE A.S.T.R.O. PASS</span>
					</div>
					<div class="slot-form" style="margin-top: 0.5rem;">
						<input
							type="text"
							placeholder="Pass Code (e.g. ASTRO-SQ-8841)"
							bind:value={squarePassCodeInput}
							class="kiosk-input"
						/>
						<button type="button" class="generate-qr-btn" onclick={handleRedeemSquarePass}>
							UNLOCK {selectedNodeId} WITH SQUARE PASS
						</button>
					</div>
				</div>

				<div class="rfid-box">
					<div class="rfid-head">
						<span>⌚ NFC WRISTBAND / VIP CARD TAP</span>
					</div>
					<div class="rfid-buttons">
						{#each (rfidPlugin?.state?.cards ?? []).slice(0, 3) as card}
							<button
								type="button"
								class="rfid-tap-btn"
								onclick={() => simulateWristbandTap(card.uid)}
							>
								<span>Tap {card.holder_name}</span>
								<small>
									{card.vip_freeplay ? 'VIP FREEPLAY' : `${card.credits_balance} CR`}
								</small>
							</button>
						{/each}
					</div>
					{#if rfidFeedback}
						<div class="rfid-feedback">{rfidFeedback}</div>
					{/if}
				</div>
			</div>
		</aside>
	</div>
</div>

<PinKeypadModal
	open={pinModalOpen}
	onClose={() => (pinModalOpen = false)}
	onSuccess={() => {
		pinModalOpen = false;
		operatorUnlocked = true;
		goto('/admin');
	}}
/>

<style>
	/* Background, text colour and font come from the active theme ($lib/theme/themes.css). */
	:global(body) {
		overflow-x: hidden;
	}

	.kiosk-shell {
		min-height: 100vh;
		padding: 1.5rem 2rem;
		box-sizing: border-box;
		max-width: 1600px;
		margin: 0 auto;
	}

	.kiosk-topbar {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		padding-bottom: 1.25rem;
		border-bottom: 1px solid rgba(var(--accent-rgb), 0.22);
		margin-bottom: 1.25rem;
		flex-wrap: wrap;
	}

	.brand-cluster {
		display: flex;
		align-items: center;
		gap: 1rem;
	}

	.pulse-dot {
		width: 14px;
		height: 14px;
		border-radius: 50%;
		background: var(--good);
		box-shadow: 0 0 14px var(--good);
	}

	.venue-tag {
		font-size: 0.75rem;
		letter-spacing: 0.14em;
		color: var(--accent);
		text-transform: uppercase;
	}

	.neon-title {
		margin: 0.15rem 0 0 0;
		font-size: 1.75rem;
		letter-spacing: 0.06em;
		color: var(--text-strong);
		text-shadow: 0 0 18px rgba(var(--accent-rgb), 0.45);
	}

	.topbar-actions {
		display: flex;
		align-items: center;
		gap: 0.85rem;
		flex-wrap: wrap;
	}

	.scene-chip {
		background: rgba(var(--special-rgb), 0.16);
		border: 1px solid rgba(var(--special-rgb), 0.5);
		padding: 0.45rem 0.85rem;
		border-radius: 999px;
		font-size: 0.78rem;
		color: var(--special);
		display: flex;
		gap: 0.4rem;
	}

	.operator-Return-pill {
		background: rgba(var(--warn-rgb), 0.18);
		border: 1px solid var(--warn);
		color: var(--warn);
		text-decoration: none;
		padding: 0.5rem 0.9rem;
		border-radius: 8px;
		font-size: 0.78rem;
		font-weight: 700;
	}

	.status-corner-badge {
		background: var(--surface-2);
		border: 1px solid rgba(var(--accent-rgb), 0.4);
		color: var(--accent);
		padding: 0.55rem 1rem;
		border-radius: 10px;
		font-family: inherit;
		font-weight: 700;
		font-size: 0.82rem;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 0.5rem;
		user-select: none;
	}

	.tap-counter {
		background: var(--danger);
		color: var(--text-strong);
		padding: 0.1rem 0.4rem;
		border-radius: 4px;
		font-size: 0.7rem;
	}

	.kiosk-mode-bar {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 0.75rem;
		margin-bottom: 1.5rem;
	}

	.kiosk-tab {
		padding: 0.9rem 1rem;
		border-radius: 12px;
		border: 1px solid var(--border);
		background: var(--surface-2);
		color: var(--text-muted);
		font-family: inherit;
		font-weight: 800;
		font-size: 0.82rem;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.kiosk-tab.active {
		border-color: var(--accent);
		background: rgba(var(--accent-rgb), 0.15);
		color: var(--accent);
		box-shadow: 0 0 16px rgba(var(--accent-rgb), 0.18);
	}

	.tournament-banner {
		background: linear-gradient(90deg, rgba(var(--danger-rgb), 0.18), rgba(var(--accent-rgb), 0.12));
		border: 1px solid rgba(var(--danger-rgb), 0.55);
		border-radius: 14px;
		padding: 1.15rem 1.5rem;
		margin-bottom: 1.25rem;
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1.5rem;
		flex-wrap: wrap;
	}

	.live-pill {
		display: inline-block;
		background: var(--danger);
		color: var(--text-strong);
		font-size: 0.7rem;
		font-weight: 700;
		padding: 0.2rem 0.55rem;
		border-radius: 4px;
		letter-spacing: 0.1em;
	}

	.tournament-meta h2 {
		margin: 0.35rem 0 0.2rem 0;
		font-size: 1.25rem;
		color: var(--text-strong);
	}

	.tournament-meta p {
		margin: 0;
		font-size: 0.85rem;
		color: var(--text-soft);
	}

	.prize {
		color: var(--warn);
		font-weight: 700;
	}

	.tournament-standings {
		display: flex;
		gap: 0.75rem;
		flex-wrap: wrap;
	}

	.standing-card {
		background: rgba(var(--surface-rgb), 0.85);
		border: 1px solid var(--border-strong);
		border-radius: 10px;
		padding: 0.6rem 0.9rem;
		display: flex;
		align-items: center;
		gap: 0.65rem;
	}

	.standing-card.rank-1 {
		border-color: var(--warn);
	}

	.rank-badge {
		font-weight: 800;
		color: var(--warn);
		font-size: 1rem;
	}

	.standing-info {
		display: flex;
		flex-direction: column;
		font-size: 0.8rem;
	}

	.standing-info span {
		color: var(--text-muted);
		font-size: 0.72rem;
	}

	.main-columns {
		display: grid;
		grid-template-columns: 1fr 400px;
		gap: 1.5rem;
	}

	.panel-block {
		background: rgba(var(--surface-rgb), 0.78);
		border: 1px solid var(--border);
		border-radius: 16px;
		padding: 1.35rem;
		margin-bottom: 1.5rem;
	}

	.section-head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		margin-bottom: 1rem;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.section-head h2 {
		margin: 0;
		font-size: 1.05rem;
		color: var(--accent);
		letter-spacing: 0.08em;
	}

	.section-head .sub {
		font-size: 0.76rem;
		color: var(--text-muted);
	}

	.cabinet-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
		gap: 0.9rem;
	}

	.cabinet-card {
		background: var(--surface-2);
		border: 2px solid var(--border);
		border-radius: 12px;
		padding: 1rem;
		color: inherit;
		font-family: inherit;
		text-align: left;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.cabinet-card.selected,
	.cabinet-card:hover {
		border-color: var(--accent);
		box-shadow: 0 0 18px rgba(var(--accent-rgb), 0.22);
		background: var(--surface-3);
	}

	.cab-top {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 0.35rem;
	}

	.cab-id {
		font-weight: 800;
		font-size: 1.05rem;
		color: var(--text-strong);
	}

	.cab-hw {
		font-size: 0.68rem;
		color: var(--accent);
		margin-bottom: 0.4rem;
	}

	.credit-pill {
		background: rgba(var(--good-rgb), 0.16);
		color: var(--good);
		border: 1px solid rgba(var(--good-rgb), 0.4);
		padding: 0.18rem 0.5rem;
		border-radius: 6px;
		font-size: 0.74rem;
		font-weight: 700;
	}

	.cab-rom {
		font-size: 0.8rem;
		color: var(--text-soft);
		margin-bottom: 0.6rem;
	}

	.cab-stats {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		font-size: 0.7rem;
		color: var(--text-muted);
	}

	.netplay-tag {
		color: var(--accent);
	}

	.games-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
		gap: 0.9rem;
	}

	.game-tile {
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-left: 4px solid var(--accent);
		border-radius: 10px;
		padding: 1rem;
	}

	.game-genre {
		font-size: 0.72rem;
		color: var(--text-muted);
		text-transform: uppercase;
	}

	.game-tile h3,
	.menu-order-tile h3 {
		margin: 0.35rem 0 0.5rem 0;
		font-size: 0.98rem;
		color: var(--text-strong);
	}

	.game-footer {
		display: flex;
		justify-content: space-between;
		font-size: 0.74rem;
		color: var(--text-soft);
	}

	.rom-tag {
		color: var(--accent);
	}

	.slot-grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 0.85rem;
		margin-bottom: 1rem;
	}

	.slot-card {
		background: var(--surface-2);
		border: 2px solid var(--border);
		border-radius: 12px;
		padding: 1.15rem;
		color: var(--text-strong);
		font-family: inherit;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
		cursor: pointer;
	}

	.slot-card.active {
		border-color: var(--good);
		background: rgba(var(--good-rgb), 0.12);
	}

	.slot-price {
		color: var(--good);
		font-size: 1.15rem;
		font-weight: 800;
	}

	.slot-form {
		display: flex;
		gap: 0.75rem;
	}

	.kiosk-input {
		flex: 1;
		padding: 0.75rem 1rem;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
		background: var(--inset);
		color: var(--text-strong);
		font-family: inherit;
	}

	.menu-order-tile {
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 10px;
		padding: 1rem;
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 0.75rem;
	}

	.menu-price {
		color: var(--good);
	}

	.qty-controls {
		display: flex;
		align-items: center;
		gap: 0.55rem;
	}

	.qty-controls button {
		width: 34px;
		height: 34px;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
		background: var(--surface-3);
		color: var(--text-strong);
		font-family: inherit;
		font-weight: 800;
		cursor: pointer;
	}

	.fb-checkout-bar {
		margin-top: 1.15rem;
		padding-top: 1rem;
		border-top: 1px solid var(--border);
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		flex-wrap: wrap;
	}

	.cart-total {
		margin-left: 1rem;
		color: var(--good);
		font-size: 1.1rem;
	}

	.pay-card {
		background: rgba(var(--surface-rgb), 0.9);
		border: 2px solid var(--accent);
		box-shadow: 0 0 25px rgba(var(--accent-rgb), 0.15);
		border-radius: 16px;
		padding: 1.4rem;
	}

	.step-tag {
		font-size: 0.72rem;
		color: var(--accent);
		letter-spacing: 0.12em;
		font-weight: 700;
	}

	.pay-card h2 {
		margin: 0.3rem 0;
		font-size: 1.35rem;
		color: var(--text-strong);
	}

	.pay-sub {
		margin: 0 0 1rem 0;
		font-size: 0.8rem;
		color: var(--text-muted);
	}

	.pack-selector {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
		margin-bottom: 1.15rem;
	}

	.pack-btn {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 0.75rem 0.95rem;
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		border-radius: 10px;
		color: var(--text-strong);
		font-family: inherit;
		cursor: pointer;
		text-align: left;
	}

	.pack-btn.active {
		border-color: var(--accent);
		background: rgba(var(--accent-rgb), 0.12);
	}

	.pack-btn div {
		display: flex;
		flex-direction: column;
	}

	.pack-btn span {
		font-size: 0.74rem;
		color: var(--text-muted);
	}

	.pack-price {
		text-align: right;
	}

	.tier-badge {
		font-size: 0.65rem;
		color: var(--warn);
		font-weight: 700;
	}

	.qr-stage {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.75rem;
		margin-bottom: 1.2rem;
	}

	.qr-matrix {
		position: relative;
		width: 170px;
		height: 170px;
		background: var(--text-strong);
		color: var(--on-accent);
		border-radius: 12px;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 0.75rem;
		box-sizing: border-box;
	}

	.qr-inner {
		border: 2px dashed var(--on-accent);
		width: 100%;
		height: 100%;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		text-align: center;
	}

	.qr-title {
		font-size: 0.7rem;
		font-weight: 700;
	}

	.generate-qr-btn {
		padding: 0.75rem 1.1rem;
		border-radius: 8px;
		border: 1px solid var(--accent);
		background: rgba(var(--accent-rgb), 0.18);
		color: var(--accent);
		font-family: inherit;
		font-weight: 700;
		cursor: pointer;
	}

	.rfid-box {
		border-top: 1px solid var(--border);
		padding-top: 1rem;
	}

	.rfid-head {
		font-size: 0.76rem;
		color: var(--warn);
		font-weight: 700;
		margin-bottom: 0.6rem;
	}

	.rfid-buttons {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.rfid-tap-btn {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 0.6rem 0.8rem;
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		border-radius: 8px;
		color: var(--text);
		font-family: inherit;
		font-size: 0.78rem;
		cursor: pointer;
	}

	.rfid-tap-btn:hover {
		border-color: var(--warn);
	}

	.rfid-tap-btn small {
		color: var(--good);
		font-weight: 700;
	}

	.rfid-feedback {
		margin-top: 0.6rem;
		font-size: 0.78rem;
		color: var(--good);
	}

	@media (max-width: 1024px) {
		.main-columns,
		.kiosk-mode-bar {
			grid-template-columns: 1fr;
		}
	}
</style>
