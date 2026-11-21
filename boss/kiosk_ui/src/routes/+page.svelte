<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import PinKeypadModal from '$lib/components/PinKeypadModal.svelte';
	import {
		fetchGames,
		fetchPlugins,
		fetchVenueTelemetry,
		getStoredOperatorToken,
		invokePluginAction,
		lookupSession,
		subscribeBossEvents,
		updateSession
	} from '$lib/bossApi';
	import type {
		CabinetAlertEvent,
		CatalogueGame,
		GameRef,
		NodeTelemetry,
		PluginDescriptor,
		SessionLookupResponse,
		VenueTelemetryReport
	} from '$lib/types/telemetry';

	const MAX_SESSION_GAMES = 8;

	const kioskMenuItems = [
		{ id: 'c1', name: 'Negroni 8-Bit', category: 'Cocktails', priceCents: 2100 },
		{ id: 'c2', name: 'Cyberpunk Espresso Martini', category: 'Cocktails', priceCents: 2200 },
		{ id: 't1', name: 'Redfern Hazy IPA Pint', category: 'Craft Taps', priceCents: 1400 },
		{ id: 't2', name: 'Regent St Lager Jug', category: 'Craft Taps', priceCents: 2600 },
		{ id: 'b1', name: 'Cheeseburger Sliders (3x)', category: 'Arcade Bites', priceCents: 1800 },
		{ id: 'b2', name: 'Woodfired Pepperoni Pizza', category: 'Arcade Bites', priceCents: 2400 }
	];

	let kioskMode = $state<'stations' | 'tokens' | 'timeslots' | 'food' | 'playing'>('stations');
	let report = $state<VenueTelemetryReport | null>(null);
	let plugins = $state<PluginDescriptor[]>([]);
	let catalogueGames = $state<CatalogueGame[]>([]);
	let selectedSystem = $state<string>('ALL');
	let selectedGameIds = $state<string[]>([]);
	let gamePickerNotice = $state<string>('');
	let selectedNodeId = $state<string>('ASTRO-01');
	let selectedPackId = $state<string>('pack-5');
	let activeQrSession = $state<any>(null);
	let rfidFeedback = $state<string>('');
	let pinModalOpen = $state(false);
	let operatorUnlocked = $state(false);
	let nowMs = $state<number>(Date.now());

	// Issued BRC-XXXX session & staging status
	let issuedSession = $state<{
		session_id: string;
		session_code: string;
		node_id: string;
		games: GameRef[];
		staging_status: 'staging' | 'ready' | 'partial' | 'failed';
		ready_game_ids: string[];
		failed_game_ids: string[];
		topup_grace_secs?: number;
	} | null>(null);

	// Expired cabinet alerts from SSE (`session_expired` / `session_ended`)
	let expiredAlerts = $state<
		Array<{
			node_id: string;
			session_code: string;
			grace_until_epoch_ms: number | null;
			at_epoch_ms: number;
		}>
	>([]);

	// "I'm already playing (BRC-XXXX)" patron flow state
	let sessionCodeInput = $state<string>('');
	let sessionLookupBusy = $state<boolean>(false);
	let sessionLookupError = $state<string>('');
	let lookedUpSession = $state<SessionLookupResponse | null>(null);
	let editGameIds = $state<string[]>([]);
	let editSystemFilter = $state<string>('ALL');
	let topupAddSecs = $state<number>(0);
	let sessionUpdateBusy = $state<boolean>(false);
	let sessionUpdateFeedback = $state<string>('');
	let sessionUpdateError = $state<string>('');

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
	let systems = $derived<string[]>([
		'ALL',
		...Array.from(new Set(catalogueGames.map((g) => g.system).filter(Boolean)))
	]);
	let filteredGames = $derived<CatalogueGame[]>(
		selectedSystem === 'ALL'
			? catalogueGames
			: catalogueGames.filter((g) => g.system === selectedSystem)
	);
	let selectedGames = $derived<CatalogueGame[]>(
		catalogueGames.filter((g) => selectedGameIds.includes(g.game_id))
	);
	let editFilteredGames = $derived<CatalogueGame[]>(
		editSystemFilter === 'ALL'
			? catalogueGames
			: catalogueGames.filter((g) => g.system === editSystemFilter)
	);
	let tournamentPlugin = $derived(
		plugins.find((p) => p.manifest.id === 'tournament-engine' && p.manifest.enabled)
	);
	let posPlugin = $derived(plugins.find((p) => p.manifest.id === 'pos-payments'));
	let rfidPlugin = $derived(plugins.find((p) => p.manifest.id === 'rfid-peripherals'));
	let lightingPlugin = $derived(plugins.find((p) => p.manifest.id === 'venue-lighting'));

	let cartTotalCents = $derived(
		kioskMenuItems.reduce((sum, item) => sum + (cart[item.id] ?? 0) * item.priceCents, 0)
	);

	let activeExpiredAlerts = $derived(
		expiredAlerts.filter(
			(a) => a.grace_until_epoch_ms == null || a.grace_until_epoch_ms > nowMs
		)
	);

	function formatSizeMiB(bytes: number): string {
		if (!Number.isFinite(bytes) || bytes <= 0) return '—';
		const mib = bytes / (1024 * 1024);
		return mib >= 1 ? `${mib.toFixed(1)} MiB` : `${Math.round(bytes / 1024)} KiB`;
	}

	function formatCountdownSecs(secs: number): string {
		const s = Math.max(0, Math.floor(secs));
		const mins = Math.floor(s / 60);
		const rem = s % 60;
		return `${String(mins).padStart(2, '0')}:${String(rem).padStart(2, '0')}`;
	}

	function toggleGameSelection(gameId: string) {
		gamePickerNotice = '';
		if (selectedGameIds.includes(gameId)) {
			selectedGameIds = selectedGameIds.filter((id) => id !== gameId);
			return;
		}
		if (selectedGameIds.length >= MAX_SESSION_GAMES) {
			gamePickerNotice = `Limit reached (${MAX_SESSION_GAMES} games max). Deselect a title to swap.`;
			return;
		}
		selectedGameIds = [...selectedGameIds, gameId];
	}

	function toggleEditGameSelection(gameId: string) {
		sessionUpdateError = '';
		const maxAllowed = lookedUpSession?.max_games ?? MAX_SESSION_GAMES;
		if (editGameIds.includes(gameId)) {
			editGameIds = editGameIds.filter((id) => id !== gameId);
			return;
		}
		if (editGameIds.length >= maxAllowed) {
			sessionUpdateError = `Maximum ${maxAllowed} games allowed for this session.`;
			return;
		}
		editGameIds = [...editGameIds, gameId];
	}

	function recordIssuedSessionFromResult(
		result: any,
		fallbackNodeId: string,
		fallbackGames: GameRef[]
	) {
		const code =
			result?.session_code ||
			(result?.simulated ? `BRC-${Math.random().toString(36).slice(2, 6).toUpperCase()}` : null);
		const sid = result?.session_id || (code ? `sess_${code.toLowerCase()}` : '');
		if (!code) return;
		const games: GameRef[] = Array.isArray(result?.games) ? result.games : fallbackGames;
		issuedSession = {
			session_id: sid,
			session_code: code,
			node_id: result?.node_id ?? fallbackNodeId,
			games,
			staging_status: games.length > 0 ? 'staging' : 'ready',
			ready_game_ids: [],
			failed_game_ids: [],
			topup_grace_secs: result?.topup_grace_secs
		};
	}

	function applyPosPluginStateIfPresent(stateObj: any) {
		if (!stateObj || typeof stateObj !== 'object') return;
		const idx = plugins.findIndex((p) => p.manifest.id === 'pos-payments');
		if (idx >= 0) {
			plugins[idx] = { ...plugins[idx], state: stateObj };
		}
	}

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
		const gameTimeTier = (posPlugin?.state?.game_time_tiers ?? []).find(
			(t: any) => t.id === selectedPackId
		);
		const res = await invokePluginAction('pos-payments', 'create_qr_session', {
			node_id: selectedNodeId,
			pack_id: selectedPackId,
			...(gameTimeTier ? { game_ids: selectedGameIds, games: selectedGames } : {})
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
		} else {
			rfidFeedback = `✕ ${res.detail || 'Wristband tap rejected'}`;
		}
	}

	async function handleBookTimeslot() {
		slotConfirmation = '';
		const picked = [...selectedGames];
		const pickedIds = [...selectedGameIds];
		const res = await invokePluginAction('pos-payments', 'book_timeslot', {
			node_id: selectedNodeId,
			duration_mins: slotDurationMins,
			patron_name: slotPatronName.trim() || 'Kiosk Guest',
			game_ids: pickedIds,
			games: picked
		});
		if (res.ok) {
			const ref = res.result?.booking_ref ?? `SLOT-${Date.now().toString(36).toUpperCase()}`;
			recordIssuedSessionFromResult(res.result, selectedNodeId, picked);
			applyPosPluginStateIfPresent(res.state);
			const codeLabel = res.result?.session_code ? ` · Code: ${res.result.session_code}` : '';
			slotConfirmation = `✓ Square POS: Locked ${slotDurationMins}m A.S.T.R.O. Session on ${selectedNodeId} (Ref: ${ref}${codeLabel})`;
			slotPatronName = '';
		}
	}

	async function handleActivateSelectedTimeTier(tier: any) {
		rfidFeedback = '';
		const picked = [...selectedGames];
		const pickedIds = [...selectedGameIds];
		const code = `ASTRO-SQ-${Date.now().toString(36).slice(-4).toUpperCase()}`;
		const res = await invokePluginAction('pos-payments', 'activate_game_time_pass', {
			node_id: selectedNodeId,
			pass_code: code,
			duration_mins: Number(tier.duration_mins ?? 60),
			credits: Number(tier.included_credits ?? 25),
			amount_cents: Number(tier.price_cents ?? 2000),
			patron_name: slotPatronName.trim() || 'Kiosk Player',
			provider: 'Square Kiosk Game-Time Pass',
			game_ids: pickedIds,
			games: picked
		});
		if (res.ok) {
			recordIssuedSessionFromResult(res.result, selectedNodeId, picked);
			applyPosPluginStateIfPresent(res.state);
			const sessionCode = res.result?.session_code ?? code;
			rfidFeedback = `✓ Activated ${tier.duration_mins}m pass on ${selectedNodeId} — Session Code: ${sessionCode}`;
		} else {
			rfidFeedback = `✕ ${res.detail || 'Could not activate Game-Time Pass'}`;
		}
	}

	async function handleRedeemSquarePass() {
		rfidFeedback = '';
		const picked = [...selectedGames];
		const pickedIds = [...selectedGameIds];
		const code =
			squarePassCodeInput.trim().toUpperCase() ||
			`ASTRO-SQ-${Date.now().toString(36).slice(-4).toUpperCase()}`;
		const res = await invokePluginAction('pos-payments', 'activate_game_time_pass', {
			node_id: selectedNodeId,
			pass_code: code,
			duration_mins: 60,
			credits: 25,
			amount_cents: 2000,
			patron_name: slotPatronName.trim() || 'Square Online Player',
			provider: 'Square Online Game-Time Pass',
			game_ids: pickedIds,
			games: picked
		});
		if (res.ok) {
			recordIssuedSessionFromResult(res.result, selectedNodeId, picked);
			applyPosPluginStateIfPresent(res.state);
			const sessionCode = res.result?.session_code ?? code;
			rfidFeedback = `✓ Activated Square Game-Time Pass ${code} on ${selectedNodeId} (Session: ${sessionCode})!`;
			squarePassCodeInput = '';
		} else {
			rfidFeedback = `✕ ${res.detail || 'Could not redeem Square Game-Time Pass'}`;
		}
	}

	async function handleLookupSession(prefillCode?: string) {
		const targetCode = (prefillCode ?? sessionCodeInput).trim().toUpperCase();
		sessionCodeInput = targetCode;
		sessionLookupError = '';
		sessionUpdateFeedback = '';
		sessionUpdateError = '';
		if (!targetCode) {
			sessionLookupError = 'Enter your BRC-XXXX session code.';
			return;
		}
		sessionLookupBusy = true;
		const res = await lookupSession(targetCode);
		sessionLookupBusy = false;
		if (res.ok) {
			lookedUpSession = res;
			editGameIds = (res.games ?? []).map((g) => g.game_id);
			topupAddSecs = 0;
		} else {
			lookedUpSession = null;
			sessionLookupError = res.error || `Session ${targetCode} not found.`;
		}
	}

	function jumpToPlayingFlow(code: string) {
		kioskMode = 'playing';
		handleLookupSession(code);
	}

	async function handleUpdateSession() {
		if (!lookedUpSession?.session_code) return;
		sessionUpdateFeedback = '';
		sessionUpdateError = '';
		sessionUpdateBusy = true;
		const replacementGames = catalogueGames.filter((g) => editGameIds.includes(g.game_id));
		const payload: { games?: GameRef[]; game_ids?: string[]; add_secs?: number } = {
			game_ids: editGameIds,
			games: replacementGames
		};
		if (topupAddSecs > 0) {
			payload.add_secs = topupAddSecs;
		}
		const res = await updateSession(lookedUpSession.session_code, payload);
		sessionUpdateBusy = false;
		if (res.ok && res.session) {
			const s = res.session;
			lookedUpSession = {
				...lookedUpSession,
				session_id: s.session_id,
				session_code: s.session_code,
				node_id: s.node_id,
				games: s.games,
				max_games: s.max_games ?? lookedUpSession.max_games,
				in_grace: s.in_grace ?? false,
				remaining_secs:
					s.remaining_secs ?? Math.max(0, Math.ceil((s.expires_at_epoch_ms - Date.now()) / 1000)),
				grace_remaining_secs: s.grace_remaining_secs ?? 0,
				expires_at_epoch_ms: s.expires_at_epoch_ms,
				grace_until_epoch_ms: s.grace_until_epoch_ms ?? null,
				started_at_epoch_ms: s.started_at_epoch_ms
			};
			editGameIds = s.games.map((g) => g.game_id);
			if (topupAddSecs > 0) {
				expiredAlerts = expiredAlerts.filter((a) => a.session_code !== s.session_code);
			}
			const minsAdded = topupAddSecs > 0 ? ` (+${Math.round(topupAddSecs / 60)}m added)` : '';
			topupAddSecs = 0;
			sessionUpdateFeedback = `✓ Updated ${s.session_code} on ${s.node_id}${minsAdded} — staging games now!`;
		} else {
			sessionUpdateError = res.error || 'Could not update session.';
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
			const [rep, plugList, games] = await Promise.all([
				fetchVenueTelemetry(),
				fetchPlugins(),
				fetchGames()
			]);
			report = rep;
			plugins = plugList;
			catalogueGames = games;
			if (rep.nodes.length > 0) {
				selectedNodeId = rep.nodes[0].node_id;
			}
		})();

		const clockTick = setInterval(() => {
			nowMs = Date.now();
		}, 1000);

		const handleKey = (e: KeyboardEvent) => {
			if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'o') {
				e.preventDefault();
				pinModalOpen = true;
			}
		};
		window.addEventListener('keydown', handleKey);

		const unsub = subscribeBossEvents(['venue', 'plugins', 'alerts'], (channel, data) => {
			if (channel === 'venue' && data && Array.isArray(data.nodes)) {
				report = data as VenueTelemetryReport;
			} else if (channel === 'plugins') {
				if (Array.isArray(data?.plugins)) {
					plugins = data.plugins;
				} else if (data?.manifest?.id) {
					const idx = plugins.findIndex((p) => p.manifest.id === data.manifest.id);
					if (idx >= 0) {
						plugins[idx] = data as PluginDescriptor;
					} else {
						plugins = [...plugins, data as PluginDescriptor];
					}
				}
			} else if (channel === 'alerts' && data?.kind === 'cabinet_event') {
				const alert = data as CabinetAlertEvent;
				const ev = alert.event;
				if (ev.type === 'session_staged') {
					if (
						issuedSession &&
						(issuedSession.session_id === ev.session_id ||
							(alert.session_code && issuedSession.session_code === alert.session_code))
					) {
						issuedSession = {
							...issuedSession,
							ready_game_ids: ev.ready_game_ids,
							failed_game_ids: ev.failed_game_ids,
							staging_status:
								ev.failed_game_ids.length > 0
									? ev.ready_game_ids.length > 0
										? 'partial'
										: 'failed'
									: 'ready'
						};
					}
				} else if (ev.type === 'session_expired') {
					const nextAlert = {
						node_id: alert.node_id,
						session_code: alert.session_code || '',
						grace_until_epoch_ms: alert.grace_until_epoch_ms ?? null,
						at_epoch_ms: alert.at_epoch_ms ?? Date.now()
					};
					expiredAlerts = [
						nextAlert,
						...expiredAlerts.filter((a) => a.node_id !== alert.node_id)
					];
					if (
						lookedUpSession &&
						(lookedUpSession.session_id === ev.session_id ||
							(alert.session_code && lookedUpSession.session_code === alert.session_code))
					) {
						lookedUpSession = {
							...lookedUpSession,
							in_grace: true,
							remaining_secs: 0,
							grace_until_epoch_ms: alert.grace_until_epoch_ms ?? null
						};
					}
				} else if (ev.type === 'session_ended') {
					expiredAlerts = expiredAlerts.filter(
						(a) =>
							a.node_id !== alert.node_id &&
							(!alert.session_code || a.session_code !== alert.session_code)
					);
					if (
						lookedUpSession &&
						(lookedUpSession.session_id === ev.session_id ||
							(alert.session_code && lookedUpSession.session_code === alert.session_code))
					) {
						lookedUpSession = null;
						sessionUpdateFeedback = 'This session has ended.';
					}
				}
			}
		});

		return () => {
			clearInterval(clockTick);
			window.removeEventListener('keydown', handleKey);
			unsub();
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

	<!-- Grace Window / Session Expired Alert Banners (live via SSE alerts) -->
	{#if activeExpiredAlerts.length > 0}
		<section class="grace-alert-stack">
			{#each activeExpiredAlerts as alert}
				{@const graceLeft = alert.grace_until_epoch_ms
					? Math.max(0, Math.ceil((alert.grace_until_epoch_ms - nowMs) / 1000))
					: 0}
				<div class="grace-alert-banner">
					<div>
						<span class="grace-badge">⏱️ SESSION TIME EXPIRED · {alert.node_id}</span>
						<strong>
							{alert.session_code ? `Code ${alert.session_code}` : 'Cabinet Session'} is in top-up grace
							{#if graceLeft > 0}
								({formatCountdownSecs(graceLeft)} left)
							{/if}
						</strong>
					</div>
					{#if alert.session_code}
						<button
							type="button"
							class="grace-cta-btn"
							onclick={() => jumpToPlayingFlow(alert.session_code)}
						>
							TOP UP {alert.session_code} →
						</button>
					{/if}
				</div>
			{/each}
		</section>
	{/if}

	<!-- Prominently Displayed Issued BRC-XXXX Session Code & Live Staging Status -->
	{#if issuedSession}
		<section class="issued-session-banner">
			<div class="issued-main">
				<div>
					<span class="issued-eyebrow">A.S.T.R.O. SESSION ISSUED · {issuedSession.node_id}</span>
					<div class="issued-code-row">
						<span class="issued-code">{issuedSession.session_code}</span>
						<span class="staging-pill stage-{issuedSession.staging_status}">
							{#if issuedSession.staging_status === 'staging'}
								⏳ STAGING GAMES...
							{:else if issuedSession.staging_status === 'ready'}
								✓ READY TO PLAY
							{:else if issuedSession.staging_status === 'partial'}
								⚠️ PARTIAL ({issuedSession.ready_game_ids.length} READY / {issuedSession.failed_game_ids.length} FAILED)
							{:else}
								✕ STAGING FAILED
							{/if}
						</span>
					</div>
					<p class="issued-hint">
						Save your <strong>{issuedSession.session_code}</strong> code to swap games or top up time later.
						{#if issuedSession.games.length > 0}
							Selected: {issuedSession.games.map((g) => g.title).join(', ')}
						{/if}
					</p>
				</div>
				<div class="issued-actions">
					<button
						type="button"
						class="generate-qr-btn"
						onclick={() => jumpToPlayingFlow(issuedSession?.session_code ?? '')}
					>
						MANAGE / TOP-UP {issuedSession.session_code}
					</button>
					<button
						type="button"
						class="dismiss-btn"
						onclick={() => (issuedSession = null)}
						aria-label="Dismiss session banner"
					>
						✕
					</button>
				</div>
			</div>
		</section>
	{/if}

	<!-- 5 Touchscreen Kiosk Mode Tabs (Stations · Tokens · Time-Slots · Food/Drinks · Already Playing) -->
	<nav class="kiosk-mode-bar">
		<button
			type="button"
			class="kiosk-tab {kioskMode === 'stations' ? 'active' : ''}"
			onclick={() => (kioskMode = 'stations')}
		>
			🕹️ 1. ARCADE CATALOGUE
		</button>
		<button
			type="button"
			class="kiosk-tab {kioskMode === 'tokens' ? 'active' : ''}"
			onclick={() => (kioskMode = 'tokens')}
		>
			🪙 2. TOKENS &amp; PASSES
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
			🍕 4. FOOD &amp; BEVERAGE
		</button>
		<button
			type="button"
			class="kiosk-tab {kioskMode === 'playing' ? 'active' : ''}"
			onclick={() => (kioskMode = 'playing')}
		>
			🎟️ 5. ALREADY PLAYING (BRC-XXXX)
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

			{#if kioskMode === 'stations' || kioskMode === 'tokens' || kioskMode === 'timeslots'}
				<!-- Live Multi-System Selectable Game Catalogue Picker -->
				<section class="panel-block">
					<div class="section-head">
						<h2>LIVE A.S.T.R.O. GAME CATALOGUE PICKER</h2>
						<div class="picker-meta">
							<span class="selection-counter">
								SELECTED: <strong>{selectedGameIds.length}</strong> / {MAX_SESSION_GAMES} GAMES
							</span>
							{#if selectedGameIds.length > 0}
								<button
									type="button"
									class="clear-pick-btn"
									onclick={() => {
										selectedGameIds = [];
										gamePickerNotice = '';
									}}
								>
									CLEAR
								</button>
							{/if}
						</div>
					</div>

					<div class="system-filter-bar">
						{#each systems as sys}
							<button
								type="button"
								class="sys-tab {selectedSystem === sys ? 'active' : ''}"
								onclick={() => (selectedSystem = sys)}
							>
								{sys === 'ALL' ? `ALL SYSTEMS (${catalogueGames.length})` : sys}
							</button>
						{/each}
					</div>

					{#if gamePickerNotice}
						<div class="picker-warn">{gamePickerNotice}</div>
					{/if}

					{#if filteredGames.length === 0}
						<div class="empty-catalogue">
							No games available in this system filter. Ask an operator to import ROMs in <code>/admin</code>.
						</div>
					{:else}
						<div class="games-grid">
							{#each filteredGames as game}
								{@const isSelected = selectedGameIds.includes(game.game_id)}
								<button
									type="button"
									class="game-tile selectable {isSelected ? 'selected' : ''}"
									aria-pressed={isSelected}
									onclick={() => toggleGameSelection(game.game_id)}
								>
									<div class="game-top-row">
										<span class="game-genre">{game.system} · {game.core}</span>
										<span class="pick-badge {isSelected ? 'picked' : ''}">
											{isSelected ? '✓ SELECTED' : '+ PICK'}
										</span>
									</div>
									<div class="game-title-row">
										{#if game.artwork_url}
											<img src={game.artwork_url} alt={game.title} class="game-thumb" />
										{/if}
										<h3>{game.title}</h3>
									</div>
									<div class="game-footer">
										<span class="rom-tag">{game.file_name}</span>
										<span>{formatSizeMiB(game.size_bytes)}</span>
									</div>
								</button>
							{/each}
						</div>
					{/if}
				</section>
			{/if}

			{#if kioskMode === 'timeslots'}
				<section class="panel-block">
					<div class="section-head">
						<h2>BOOK A MANAGED STATION TIME-SLOT ({selectedNodeId})</h2>
						<span class="sub">
							Locks {selectedNodeId} into A.S.T.R.O. Managed Session Mode with {selectedGameIds.length} selected game{selectedGameIds.length === 1 ? '' : 's'}
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
			{:else if kioskMode === 'playing'}
				<section class="panel-block">
					<div class="section-head">
						<h2>I'M ALREADY PLAYING — SWAP GAMES OR TOP UP TIME</h2>
						<span class="sub">Enter your 4-character BRC-XXXX session code from your cabinet or receipt</span>
					</div>

					<form
						class="slot-form"
						onsubmit={(e) => {
							e.preventDefault();
							handleLookupSession();
						}}
					>
						<input
							type="text"
							placeholder="Enter Session Code (e.g. BRC-7K2M)"
							bind:value={sessionCodeInput}
							class="kiosk-input code-input"
						/>
						<button type="submit" class="generate-qr-btn" disabled={sessionLookupBusy}>
							{sessionLookupBusy ? 'LOOKING UP...' : 'FIND MY SESSION'}
						</button>
					</form>

					{#if sessionLookupError}
						<div class="picker-warn" style="margin-top: 0.75rem;">✕ {sessionLookupError}</div>
					{/if}

					{#if lookedUpSession && lookedUpSession.ok}
						{@const liveRemainingSecs = lookedUpSession.expires_at_epoch_ms
							? Math.max(0, Math.ceil((lookedUpSession.expires_at_epoch_ms - nowMs) / 1000))
							: (lookedUpSession.remaining_secs ?? 0)}
						{@const liveGraceSecs = lookedUpSession.grace_until_epoch_ms
							? Math.max(0, Math.ceil((lookedUpSession.grace_until_epoch_ms - nowMs) / 1000))
							: (lookedUpSession.grace_remaining_secs ?? 0)}
						{@const inGrace = Boolean(lookedUpSession.in_grace || (liveRemainingSecs <= 0 && liveGraceSecs > 0))}

						<div class="session-active-card">
							<div class="session-card-head">
								<div>
									<span class="issued-eyebrow">CABINET {lookedUpSession.node_id}</span>
									<h3 class="session-code-heading">{lookedUpSession.session_code}</h3>
								</div>
								<div class="session-timers">
									{#if inGrace}
										<span class="staging-pill stage-failed">
											⏱️ GRACE WINDOW: {formatCountdownSecs(liveGraceSecs)} LEFT
										</span>
									{:else}
										<span class="staging-pill stage-ready">
											⏱️ TIME LEFT: {formatCountdownSecs(liveRemainingSecs)}
										</span>
									{/if}
								</div>
							</div>

							{#if inGrace}
								<div class="picker-warn">
									Your session time has expired! Pick a quick top-up below before the grace window closes to keep playing without losing your cabinet.
								</div>
							{/if}

							<div class="current-games-strip">
								<span class="strip-label">CURRENTLY LOADED ({(lookedUpSession.games ?? []).length}):</span>
								<div class="strip-chips">
									{#each lookedUpSession.games ?? [] as g}
										<span class="game-chip">{g.title}</span>
									{:else}
										<span class="sub">No custom game filter set</span>
									{/each}
								</div>
							</div>

							<div class="topup-section">
								<div class="strip-label">1. QUICK TIME TOP-UP:</div>
								<div class="topup-grid">
									{#each [
										{ secs: 0, label: 'SWAP GAMES ONLY (+0m)' },
										{ secs: 900, label: '+15 MIN TOP-UP' },
										{ secs: 1800, label: '+30 MIN TOP-UP' },
										{ secs: 3600, label: '+60 MIN TOP-UP' }
									] as opt}
										<button
											type="button"
											class="topup-btn {topupAddSecs === opt.secs ? 'active' : ''}"
											onclick={() => (topupAddSecs = opt.secs)}
										>
											{opt.label}
										</button>
									{/each}
								</div>
							</div>

							<div class="topup-section">
								<div class="section-head" style="margin-bottom: 0.6rem;">
									<span class="strip-label">
										2. SWAP / PICK GAMES ({editGameIds.length} / {lookedUpSession.max_games ?? MAX_SESSION_GAMES} MAX)
									</span>
									<div class="system-filter-bar" style="margin-bottom: 0;">
										{#each systems as sys}
											<button
												type="button"
												class="sys-tab small {editSystemFilter === sys ? 'active' : ''}"
												onclick={() => (editSystemFilter = sys)}
											>
												{sys}
											</button>
										{/each}
									</div>
								</div>

								<div class="games-grid">
									{#each editFilteredGames as game}
										{@const picked = editGameIds.includes(game.game_id)}
										<button
											type="button"
											class="game-tile selectable {picked ? 'selected' : ''}"
											aria-pressed={picked}
											onclick={() => toggleEditGameSelection(game.game_id)}
										>
											<div class="game-top-row">
												<span class="game-genre">{game.system} · {game.core}</span>
												<span class="pick-badge {picked ? 'picked' : ''}">
													{picked ? '✓ LOADED' : '+ ADD'}
												</span>
											</div>
											<h3>{game.title}</h3>
											<div class="game-footer">
												<span class="rom-tag">{game.file_name}</span>
												<span>{formatSizeMiB(game.size_bytes)}</span>
											</div>
										</button>
									{/each}
								</div>
							</div>

							<div class="session-submit-bar">
								<button
									type="button"
									class="generate-qr-btn"
									disabled={sessionUpdateBusy}
									onclick={handleUpdateSession}
								>
									{sessionUpdateBusy
										? 'UPDATING CABINET...'
										: `UPDATE ${lookedUpSession.session_code} (${editGameIds.length} GAMES${topupAddSecs > 0 ? ` + ${Math.round(topupAddSecs / 60)}M` : ''})`}
								</button>
							</div>

							{#if sessionUpdateFeedback}
								<div class="rfid-feedback">{sessionUpdateFeedback}</div>
							{/if}
							{#if sessionUpdateError}
								<div class="picker-warn" style="margin-top: 0.6rem;">✕ {sessionUpdateError}</div>
							{/if}
						</div>
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
						<div class="tier-row-wrap">
							<button
								type="button"
								class="pack-btn {selectedPackId === gt.id ? 'active' : ''}"
								style="width: 100%;"
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
							{#if selectedPackId === gt.id}
								<button
									type="button"
									class="activate-pass-btn"
									onclick={() => handleActivateSelectedTimeTier(gt)}
								>
									⚡ ACTIVATE {gt.duration_mins}M PASS ON {selectedNodeId} ({selectedGameIds.length} GAMES)
								</button>
							{/if}
						</div>
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
		grid-template-columns: repeat(5, 1fr);
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
		border-left: 4px solid var(--game-accent);
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

	/* Wave 3C: Grace alerts, Issued BRC-XXXX banner, Live Catalogue Picker & Already Playing flow */
	.grace-alert-stack {
		display: flex;
		flex-direction: column;
		gap: 0.65rem;
		margin-bottom: 1.15rem;
	}

	.grace-alert-banner {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		flex-wrap: wrap;
		padding: 0.85rem 1.25rem;
		border-radius: 12px;
		border: 2px solid var(--warn);
		background: rgba(var(--warn-rgb), 0.16);
	}

	.grace-alert-banner > div {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	.grace-badge {
		font-size: 0.7rem;
		font-weight: 800;
		letter-spacing: 0.1em;
		color: var(--warn);
	}

	.grace-cta-btn {
		padding: 0.6rem 1rem;
		border-radius: 8px;
		border: 1px solid var(--warn);
		background: var(--warn);
		color: var(--on-accent);
		font-family: inherit;
		font-weight: 800;
		cursor: pointer;
	}

	.issued-session-banner {
		background: linear-gradient(90deg, rgba(var(--good-rgb), 0.14), rgba(var(--accent-rgb), 0.12));
		border: 2px solid var(--good);
		border-radius: 14px;
		padding: 1.1rem 1.4rem;
		margin-bottom: 1.25rem;
	}

	.issued-main {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		flex-wrap: wrap;
	}

	.issued-eyebrow {
		font-size: 0.72rem;
		font-weight: 800;
		letter-spacing: 0.14em;
		color: var(--good);
		text-transform: uppercase;
	}

	.issued-code-row {
		display: flex;
		align-items: center;
		gap: 0.85rem;
		margin: 0.3rem 0;
		flex-wrap: wrap;
	}

	.issued-code {
		font-family: var(--font-data);
		font-size: 2rem;
		font-weight: 900;
		letter-spacing: 0.12em;
		color: var(--text-strong);
		background: var(--inset);
		border: 1px solid var(--border-strong);
		padding: 0.25rem 0.75rem;
		border-radius: 8px;
	}

	.staging-pill {
		font-size: 0.74rem;
		font-weight: 800;
		padding: 0.3rem 0.75rem;
		border-radius: 999px;
		border: 1px solid transparent;
	}

	.staging-pill.stage-staging {
		background: rgba(var(--warn-rgb), 0.18);
		border-color: var(--warn);
		color: var(--warn);
	}

	.staging-pill.stage-ready {
		background: rgba(var(--good-rgb), 0.18);
		border-color: var(--good);
		color: var(--good);
	}

	.staging-pill.stage-partial {
		background: rgba(var(--warn-rgb), 0.22);
		border-color: var(--warn);
		color: var(--warn);
	}

	.staging-pill.stage-failed {
		background: rgba(var(--danger-rgb), 0.2);
		border-color: var(--danger);
		color: var(--danger-soft);
	}

	.issued-hint {
		margin: 0.2rem 0 0;
		font-size: 0.82rem;
		color: var(--text-soft);
	}

	.issued-actions {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.dismiss-btn {
		padding: 0.5rem 0.75rem;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
		background: var(--surface-2);
		color: var(--text-muted);
		font-family: inherit;
		font-weight: 700;
		cursor: pointer;
	}

	.picker-meta {
		display: flex;
		align-items: center;
		gap: 0.65rem;
	}

	.selection-counter {
		font-size: 0.78rem;
		color: var(--text-soft);
		background: var(--inset);
		border: 1px solid var(--border);
		padding: 0.3rem 0.65rem;
		border-radius: 999px;
	}

	.selection-counter strong {
		color: var(--accent);
	}

	.clear-pick-btn {
		padding: 0.28rem 0.6rem;
		border-radius: 6px;
		border: 1px solid var(--border-strong);
		background: var(--surface-2);
		color: var(--text-muted);
		font-family: inherit;
		font-size: 0.7rem;
		font-weight: 700;
		cursor: pointer;
	}

	.system-filter-bar {
		display: flex;
		flex-wrap: wrap;
		gap: 0.45rem;
		margin-bottom: 0.9rem;
	}

	.sys-tab {
		padding: 0.45rem 0.85rem;
		border-radius: 999px;
		border: 1px solid var(--border);
		background: var(--surface-2);
		color: var(--text-muted);
		font-family: inherit;
		font-size: 0.74rem;
		font-weight: 700;
		cursor: pointer;
	}

	.sys-tab.small {
		padding: 0.3rem 0.65rem;
		font-size: 0.7rem;
	}

	.sys-tab.active {
		border-color: var(--accent);
		background: rgba(var(--accent-rgb), 0.16);
		color: var(--accent);
	}

	.picker-warn {
		margin-bottom: 0.75rem;
		padding: 0.6rem 0.9rem;
		border-radius: 8px;
		border: 1px solid var(--warn);
		background: rgba(var(--warn-rgb), 0.14);
		color: var(--warn);
		font-size: 0.8rem;
		font-weight: 700;
	}

	.empty-catalogue {
		padding: 1.5rem;
		text-align: center;
		color: var(--text-muted);
		background: var(--inset);
		border: 1px dashed var(--border);
		border-radius: 10px;
		font-size: 0.84rem;
	}

	.game-tile.selectable {
		text-align: left;
		color: inherit;
		font-family: inherit;
		cursor: pointer;
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		gap: 0.45rem;
		border-left: 4px solid var(--border-strong);
		transition: all 0.14s ease;
	}

	.game-tile.selectable:hover {
		border-color: var(--accent);
		background: var(--surface-3);
	}

	.game-tile.selectable.selected {
		border-color: var(--good);
		border-left-color: var(--good);
		background: rgba(var(--good-rgb), 0.1);
		box-shadow: 0 0 14px rgba(var(--good-rgb), 0.16);
	}

	.game-top-row {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 0.4rem;
		width: 100%;
	}

	.pick-badge {
		font-size: 0.68rem;
		font-weight: 800;
		padding: 0.16rem 0.45rem;
		border-radius: 4px;
		background: var(--inset);
		border: 1px solid var(--border-strong);
		color: var(--text-muted);
	}

	.pick-badge.picked {
		background: var(--good);
		border-color: var(--good);
		color: var(--on-accent);
	}

	.game-title-row {
		display: flex;
		align-items: center;
		gap: 0.65rem;
	}

	.game-thumb {
		width: 40px;
		height: 40px;
		border-radius: 6px;
		object-fit: cover;
		border: 1px solid var(--border);
	}

	.tier-row-wrap {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	.activate-pass-btn {
		padding: 0.55rem 0.85rem;
		border-radius: 8px;
		border: 1px solid var(--good);
		background: rgba(var(--good-rgb), 0.18);
		color: var(--good);
		font-family: inherit;
		font-size: 0.76rem;
		font-weight: 800;
		cursor: pointer;
	}

	.code-input {
		text-transform: uppercase;
		font-family: var(--font-data);
		font-size: 1.05rem;
		letter-spacing: 0.12em;
	}

	.session-active-card {
		margin-top: 1.25rem;
		padding: 1.2rem;
		border-radius: 12px;
		border: 1px solid var(--accent);
		background: var(--surface-2);
	}

	.session-card-head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.75rem;
		margin-bottom: 0.9rem;
	}

	.session-code-heading {
		margin: 0.15rem 0 0;
		font-family: var(--font-data);
		font-size: 1.75rem;
		letter-spacing: 0.12em;
		color: var(--text-strong);
	}

	.current-games-strip {
		margin-bottom: 1rem;
	}

	.strip-label {
		display: block;
		font-size: 0.74rem;
		font-weight: 800;
		letter-spacing: 0.08em;
		color: var(--accent);
		margin-bottom: 0.45rem;
	}

	.strip-chips {
		display: flex;
		flex-wrap: wrap;
		gap: 0.4rem;
	}

	.game-chip {
		padding: 0.25rem 0.6rem;
		border-radius: 6px;
		background: var(--inset);
		border: 1px solid var(--border-strong);
		font-size: 0.76rem;
		color: var(--text-strong);
	}

	.topup-section {
		margin-top: 1rem;
	}

	.topup-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
		gap: 0.6rem;
	}

	.topup-btn {
		padding: 0.7rem 0.8rem;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
		background: var(--inset);
		color: var(--text);
		font-family: inherit;
		font-size: 0.78rem;
		font-weight: 800;
		cursor: pointer;
	}

	.topup-btn.active {
		border-color: var(--good);
		background: rgba(var(--good-rgb), 0.18);
		color: var(--good);
	}

	.session-submit-bar {
		margin-top: 1.1rem;
		display: flex;
		justify-content: flex-end;
	}

	@media (max-width: 1024px) {
		.main-columns,
		.kiosk-mode-bar {
			grid-template-columns: 1fr;
		}
	}
</style>
