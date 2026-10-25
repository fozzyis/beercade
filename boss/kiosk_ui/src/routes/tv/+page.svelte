<script lang="ts">
	import { onMount } from 'svelte';
	import { fetchPlugins, fetchVenueTelemetry, subscribeBossEvents } from '$lib/bossApi';
	import type { HighScoreEntry, NodeTelemetry, PluginDescriptor } from '$lib/types/telemetry';

	type SlideId = 'high_scores' | 'tournament_bracket' | 'live_cabinets' | 'taproom_promo';

	let zone = $state<'main' | 'bar' | 'stage'>('main');
	let venueName = $state('Beercade Redfern (The BK)');
	let nodes = $state<NodeTelemetry[]>([]);
	let plugins = $state<PluginDescriptor[]>([]);
	let currentSlide = $state<SlideId>('high_scores');
	let autoRotate = $state(true);
	let slideProgressPct = $state(0);
	let clockTime = $state('');

	// Live fanfare interrupt overlay (triggered by SSE high score or operator fanfare)
	let fanfare = $state<{
		id: string;
		kind: string;
		headline: string;
		subtext: string;
		node_id: string;
	} | null>(null);
	let lastSeenFanfareId = $state<string | null>(null);

	const slidesOrder: { id: SlideId; label: string; badge: string }[] = [
		{ id: 'high_scores', label: 'HIGH SCORE HALL OF FAME', badge: 'LEADERBOARD' },
		{ id: 'tournament_bracket', label: 'LIVE TOURNAMENT BRACKET', badge: 'ESPORTS STAGE' },
		{ id: 'live_cabinets', label: 'O.R.I.O.N. CABINET FLOOR & TIMERS', badge: 'A.S.T.R.O. LIVE' },
		{ id: 'taproom_promo', label: 'SQUARE GAME-TIME PASS & TAP LIST', badge: 'TAPROOM & POS' }
	];

	const tournamentPlugin = $derived(plugins.find((p) => p.manifest.id === 'tournament-engine'));
	const posPlugin = $derived(plugins.find((p) => p.manifest.id === 'pos-payments'));
	const lightingPlugin = $derived(plugins.find((p) => p.manifest.id === 'venue-lighting'));

	const slideIntervalSecs = $derived(
		Number(tournamentPlugin?.state?.tv_slide_interval_secs) || 12
	);
	const announcementBanner = $derived(
		tournamentPlugin?.state?.tv_announcement_banner ||
			'SCAN QR TO BUY A.S.T.R.O. GAME TIME VIA SQUARE · TAP WRISTBAND AT ANY O.R.I.O.N. CABINET'
	);

	const allTimeScores: HighScoreEntry[] = [
		{
			node_id: 'ASTRO-01',
			active_core: 'fbneo_libretro',
			active_rom: 'sfiii3.zip',
			game_name: 'Street Fighter III: 3rd Strike',
			score: 984200,
			initials: 'DGO',
			achieved_at_epoch_ms: Date.now() - 600_000
		},
		{
			node_id: 'ASTRO-02',
			active_core: 'fbneo_libretro',
			active_rom: 'sfiii3.zip',
			game_name: 'Street Fighter III: 3rd Strike',
			score: 912500,
			initials: 'JWG',
			achieved_at_epoch_ms: Date.now() - 420_000
		},
		{
			node_id: 'ASTRO-04',
			active_core: 'flycast_libretro',
			active_rom: 'mvc2.zip',
			game_name: 'Marvel vs. Capcom 2',
			score: 889400,
			initials: 'KRD',
			achieved_at_epoch_ms: Date.now() - 180_000
		},
		{
			node_id: 'ASTRO-03',
			active_core: 'mame2003_plus',
			active_rom: 'mslugX.zip',
			game_name: 'Metal Slug X',
			score: 4512900,
			initials: 'ACE',
			achieved_at_epoch_ms: Date.now() - 1_200_000
		},
		{
			node_id: 'ASTRO-01',
			active_core: 'fbneo_libretro',
			active_rom: 'garou.zip',
			game_name: 'Garou: Mark of the Wolves',
			score: 742100,
			initials: 'MOV',
			achieved_at_epoch_ms: Date.now() - 2_400_000
		},
		{
			node_id: 'ASTRO-02',
			active_core: 'mame2003_plus',
			active_rom: 'pacman.zip',
			game_name: 'Pac-Man (Midway)',
			score: 318900,
			initials: 'MAX',
			achieved_at_epoch_ms: Date.now() - 3_600_000
		}
	];

	let liveScores = $state<HighScoreEntry[]>([...allTimeScores]);

	const activeTimedSessions = $derived(
		(posPlugin?.state?.active_timed_sessions ?? []) as Array<{
			session_id: string;
			pass_code: string;
			patron_name: string;
			node_id: string;
			duration_mins: number;
			started_at_ms: number;
			expires_at_ms: number;
			provider: string;
			status: string;
		}>
	);

	function triggerFanfareBanner(alert: {
		id: string;
		kind: string;
		headline: string;
		subtext: string;
		node_id: string;
	}) {
		if (alert.id === lastSeenFanfareId) return;
		lastSeenFanfareId = alert.id;
		fanfare = alert;
		setTimeout(() => {
			if (fanfare?.id === alert.id) {
				fanfare = null;
			}
		}, 8000);
	}

	async function refreshData() {
		const [rep, plugList] = await Promise.all([fetchVenueTelemetry(), fetchPlugins()]);
		venueName = rep.venue;
		nodes = rep.nodes;
		plugins = plugList;

		const tourney = plugList.find((p) => p.manifest.id === 'tournament-engine');
		if (tourney?.state?.active_fanfare) {
			triggerFanfareBanner(tourney.state.active_fanfare);
		}
	}

	function nextSlide() {
		const idx = slidesOrder.findIndex((s) => s.id === currentSlide);
		const nextIdx = (idx + 1) % slidesOrder.length;
		currentSlide = slidesOrder[nextIdx].id;
		slideProgressPct = 0;
	}

	function toggleFullscreen() {
		if (typeof document === 'undefined') return;
		if (!document.fullscreenElement) {
			document.documentElement.requestFullscreen?.();
		} else {
			document.exitFullscreen?.();
		}
	}

	function remainingMinutes(expiresAtMs: number): number {
		return Math.max(0, Math.ceil((expiresAtMs - Date.now()) / 60_000));
	}

	function romDisplayTitle(rom: string): string {
		const clean = rom.replace(/\.zip$/i, '').toLowerCase();
		const map: Record<string, string> = {
			sfiii3: 'Street Fighter III: 3rd Strike',
			mslugx: 'Metal Slug X',
			mvc2: 'Marvel vs. Capcom 2',
			garou: 'Garou: Mark of the Wolves',
			pacman: 'Pac-Man (Midway)'
		};
		return map[clean] || rom;
	}

	onMount(() => {
		const params = new URLSearchParams(window.location.search);
		const z = params.get('zone');
		if (z === 'bar' || z === 'stage' || z === 'main') {
			zone = z;
			if (z === 'bar') currentSlide = 'taproom_promo';
			if (z === 'stage') currentSlide = 'tournament_bracket';
		}

		refreshData();
		const pollTimer = setInterval(refreshData, 5000);

		const tickMs = 200;
		const rotateTimer = setInterval(() => {
			clockTime = new Date().toLocaleTimeString([], {
				hour: '2-digit',
				minute: '2-digit',
				second: '2-digit'
			});
			if (!autoRotate) return;
			const step = (tickMs / (slideIntervalSecs * 1000)) * 100;
			slideProgressPct += step;
			if (slideProgressPct >= 100) {
				nextSlide();
			}
		}, tickMs);

		const unsub = subscribeBossEvents(['venue', 'alerts', 'plugins'], (channel, data) => {
			if (channel === 'alerts' && data?.score && data?.initials) {
				const entry: HighScoreEntry = data;
				liveScores = [entry, ...liveScores].sort((a, b) => b.score - a.score).slice(0, 10);
				triggerFanfareBanner({
					id: `score_${Date.now()}`,
					kind: 'high_score',
					headline: `NEW HIGH SCORE BY ${entry.initials}!`,
					subtext: `${entry.score.toLocaleString()} PTS ON ${entry.game_name} (${entry.node_id})`,
					node_id: entry.node_id
				});
			} else if (channel === 'plugins' && data?.plugins) {
				plugins = data.plugins;
				const tourney = data.plugins.find((p: any) => p.manifest?.id === 'tournament-engine');
				if (tourney?.state?.active_fanfare) {
					triggerFanfareBanner(tourney.state.active_fanfare);
				}
			}
		});

		return () => {
			clearInterval(pollTimer);
			clearInterval(rotateTimer);
			unsub();
		};
	});
</script>

<svelte:head>
	<title>B.E.E.R.C.A.D.E. // Venue TV Attractor & High Score Signage ({zone.toUpperCase()})</title>
</svelte:head>

<div class="tv-shell">
	<!-- Real-Time High Score / Match Winner Fanfare Interrupt Overlay -->
	{#if fanfare}
		<div class="fanfare-scrim">
			<div class="fanfare-card">
				<div class="fanfare-tag">
					<span class="ping-dot"></span>
					LIVE A.S.T.R.O. CABINET BROADCAST // {fanfare.node_id}
				</div>
				<h1 class="fanfare-headline">{fanfare.headline}</h1>
				<p class="fanfare-sub">{fanfare.subtext}</p>
				<div class="fanfare-foot">
					Venue Lighting Scene Synced: {lightingPlugin?.state?.active_scene ?? 'high_score_strobe'}
				</div>
			</div>
		</div>
	{/if}

	<!-- Top Broadcast Header -->
	<header class="tv-header">
		<div class="header-row">
			<div class="brand">
				<div class="brand-mark">TV</div>
				<div>
					<div class="brand-meta">
						<span class="zone-tag">B.O.S.S. SIGNAGE STREAM · ZONE: {zone.toUpperCase()}</span>
						<span class="live-tag"><span class="live-dot"></span>60FPS LIVE SSE</span>
					</div>
					<h1 class="venue-title">
						{venueName} — <span class="accent">A.S.T.R.O.</span> Arcade &amp; Taproom
					</h1>
				</div>
			</div>

			<!-- Slide Indicator Pills + Controls -->
			<div class="controls">
				{#each slidesOrder as s}
					<button
						type="button"
						class="pill {currentSlide === s.id ? 'active' : ''}"
						onclick={() => {
							currentSlide = s.id;
							slideProgressPct = 0;
						}}
					>
						{s.badge}
					</button>
				{/each}
				<button type="button" class="pill ghost" onclick={() => (autoRotate = !autoRotate)}>
					{autoRotate ? 'PAUSE LOOP' : 'RESUME LOOP'}
				</button>
				<button type="button" class="pill outline" onclick={toggleFullscreen}>FULLSCREEN</button>
				<a href="/" class="pill ghost">KIOSK</a>
			</div>
		</div>

		<!-- Slide Progress Bar -->
		<div class="progress"><div class="progress-fill" style="width: {slideProgressPct}%"></div></div>
	</header>

	<!-- Main Attractor Slide Stage -->
	<main class="stage">
		{#if currentSlide === 'high_scores'}
			<!-- SLIDE 1: HIGH SCORE HALL OF FAME -->
			<div class="split">
				<div class="card wide warm">
					<div class="card-head">
						<div>
							<span class="eyebrow warn">REAL-TIME O.R.I.O.N. TELEMETRY CAPTURE</span>
							<h2 class="card-title">ALL-TIME HIGH SCORE HALL OF FAME</h2>
						</div>
						<span class="count-chip">TOP {liveScores.length} RECORDS</span>
					</div>

					<div class="score-list">
						{#each liveScores.slice(0, 6) as entry, idx}
							<div class="score-row rank-{idx + 1}">
								<div class="score-left">
									<div class="rank-badge">#{idx + 1}</div>
									<div>
										<div class="score-who">
											<span class="initials">{entry.initials}</span>
											<span class="node-chip">{entry.node_id}</span>
										</div>
										<div class="score-game">{entry.game_name}</div>
									</div>
								</div>
								<div class="score-right">
									<div class="score-value">{entry.score.toLocaleString()}</div>
									<div class="score-note">PTS · VERIFIED BY B.O.S.S.</div>
								</div>
							</div>
						{/each}
					</div>
				</div>

				<!-- Right column: Current Tournament Spotlight -->
				<div class="card narrow cool spread">
					<div>
						<span class="tag special">FEATURED CHALLENGE</span>
						<h3 class="side-title">{tournamentPlugin?.state?.title ?? 'FRIDAY NIGHT FIGHTS'}</h3>
						<p class="side-copy">
							Prize Pool: <span class="warn-text">{tournamentPlugin?.state?.prize_pool ?? '$150 Bar Tab + 50 Tokens'}</span>
						</p>

						<div class="leader-list">
							<div class="eyebrow muted">Current Challenge Leaders</div>
							{#each tournamentPlugin?.state?.leaderboard ?? [] as standing, i}
								<div class="leader-row">
									<div class="leader-who">
										<span class="warn-text strong">#{i + 1}</span>
										<span class="leader-name">{standing.player_name}</span>
										<span class="muted-text">({standing.initials})</span>
									</div>
									<span class="leader-score">{Number(standing.score).toLocaleString()}</span>
								</div>
							{/each}
						</div>
					</div>

					<div class="callout good">
						<div class="eyebrow good">HOW TO ENTER</div>
						<p>
							Tap your RFID wristband or activate a Square A.S.T.R.O. Game-Time Pass on any cabinet running
							<span class="data strong">{tournamentPlugin?.state?.game_name ?? 'Street Fighter III'}</span>. Scores post automatically!
						</p>
					</div>
				</div>
			</div>
		{:else if currentSlide === 'tournament_bracket'}
			<!-- SLIDE 2: LIVE TOURNAMENT BRACKET -->
			<div class="card special-border">
				<div class="card-head">
					<div>
						<span class="eyebrow special">LIVE VENUE TOURNAMENT BRACKET // B.O.S.S. STAGE FEED</span>
						<h2 class="card-title">{tournamentPlugin?.state?.title ?? 'FRIDAY NIGHT FIGHTS'}</h2>
					</div>
					<div class="prize-box">
						<div class="eyebrow warn">PRIZE POOL</div>
						<div class="prize-value">{tournamentPlugin?.state?.prize_pool ?? '$150 Bar Tab + Champion Belt'}</div>
					</div>
				</div>

				<div class="bracket-grid">
					{#each tournamentPlugin?.state?.bracket_matches ?? [] as m}
						<div class="match {m.status === 'in_progress' ? 'live' : ''}">
							<div class="match-head">
								<span class="node-chip">ROUND {m.round} · MATCH {m.id.toUpperCase()}</span>
								<span class="status-chip {m.status === 'in_progress' ? 'live' : 'done'}">
									{m.status === 'in_progress' ? `LIVE ON ${m.cabinet_node_id}` : 'COMPLETED'}
								</span>
							</div>
							<div class="player {m.winner === m.player_a ? 'winner' : ''}">
								<span>{m.player_a}</span>
								{#if m.winner === m.player_a}<span class="winner-tag">WINNER</span>{/if}
							</div>
							<div class="vs">— VS —</div>
							<div class="player {m.winner === m.player_b ? 'winner' : ''}">
								<span>{m.player_b}</span>
								{#if m.winner === m.player_b}<span class="winner-tag">WINNER</span>{/if}
							</div>
						</div>
					{/each}
				</div>
			</div>
		{:else if currentSlide === 'live_cabinets'}
			<!-- SLIDE 3: O.R.I.O.N. CABINET FLOOR STATUS & SQUARE GAME-TIME TIMERS -->
			<div class="split">
				<div class="wide">
					<div class="floor-head">
						<h2 class="card-title">LIVE O.R.I.O.N. CABINET FLOOR ({nodes.length} ONLINE)</h2>
						<span class="eyebrow accent">A.S.T.R.O. DIRECT-TO-KMS/DRM · ZERO-LATENCY</span>
					</div>
					<div class="cab-grid">
						{#each nodes as n}
							{@const activePass = activeTimedSessions.find(
								(s) => s.node_id === n.node_id && s.status === 'active'
							)}
							<div class="card cab">
								<div class="cab-head">
									<span class="cab-id">{n.node_id}</span>
									{#if activePass}
										<span class="status-chip warn">TIMED PASS: {remainingMinutes(activePass.expires_at_ms)}M LEFT</span>
									{:else if n.credits.credits_available > 0}
										<span class="status-chip accent">IN PLAY ({n.credits.credits_available} CREDITS)</span>
									{:else}
										<span class="status-chip good">OPEN — READY TO PLAY</span>
									{/if}
								</div>
								<div class="cab-game">{romDisplayTitle(n.active_rom)}</div>
								<div class="cab-meta">
									{n.hardware} · {n.fps.toFixed(1)} FPS · CPU {n.cpu_temp_c.toFixed(0)}°C
								</div>
								{#if activePass}
									<div class="cab-pass">
										Player: <span class="strong">{activePass.patron_name}</span> ({activePass.pass_code})
									</div>
								{/if}
							</div>
						{/each}
					</div>
				</div>

				<!-- Active Square Game-Time Pass Countdown Board -->
				<div class="card narrow warm">
					<span class="eyebrow warn">SQUARE POS + ONLINE PASSES</span>
					<h3 class="side-title">ACTIVE A.S.T.R.O. TIMED SESSIONS</h3>
					<p class="side-copy small">
						Buy online or at the B.O.S.S. Kiosk via Square to unlock timed free-play on any O.R.I.O.N. cabinet.
					</p>

					<div class="pass-list">
						{#each activeTimedSessions as sess}
							<div class="pass-row">
								<div class="pass-head">
									<span class="accent-text strong">{sess.node_id}</span>
									<span class="status-chip warn">{remainingMinutes(sess.expires_at_ms)} MIN REMAINING</span>
								</div>
								<div class="leader-name">{sess.patron_name}</div>
								<div class="pass-meta">
									<span>Pass: {sess.pass_code}</span>
									<span>{sess.provider}</span>
								</div>
							</div>
						{:else}
							<div class="empty">No timed passes currently active. Scan the QR code or visit the B.O.S.S. Kiosk!</div>
						{/each}
					</div>
				</div>
			</div>
		{:else}
			<!-- SLIDE 4: TAPROOM CRAFT BEER & SQUARE ONLINE A.S.T.R.O. GAME-TIME PASS PROMO -->
			<div class="split">
				<div class="card wide warm">
					<span class="eyebrow warn">ON TAP NOW AT THE BEERCADE BAR</span>
					<h2 class="card-title">CRAFT TAP LIST &amp; ARCADE FUEL</h2>
					<div class="tap-grid">
						{#each [
							{ tag: 'TAP #01 · HAZY IPA', tone: 'warn', price: '$11', name: 'HADOUKEN HAZE', desc: '6.4% ABV · Tropical Citra & Mosaic hops' },
							{ tag: 'TAP #02 · JAPANESE LAGER', tone: 'accent', price: '$10', name: '1-UP CRISP RICE LAGER', desc: '4.8% ABV · Ultra-clean arcade session lager' },
							{ tag: 'TAP #03 · SOUR ALE', tone: 'special', price: '$12', name: 'PIXEL PUNCH GUAVA SOUR', desc: '5.2% ABV · Pink guava, passionfruit & lime' },
							{ tag: 'KITCHEN · COMBO', tone: 'good', price: '$22', name: 'DOUBLE DRAGON SMASH BURGER', desc: 'Double wagyu, secret boss sauce & loaded fries' }
						] as tap}
							<div class="tap">
								<div class="tap-head">
									<span class="data tone-{tap.tone}">{tap.tag}</span>
									<span class="tap-price">{tap.price}</span>
								</div>
								<div class="tap-name">{tap.name}</div>
								<div class="tap-desc">{tap.desc}</div>
							</div>
						{/each}
					</div>
				</div>

				<!-- Square Online Game-Time Pass QR Callout -->
				<div class="card narrow cool spread">
					<div>
						<span class="tag accent">POWERED BY SQUARE POS</span>
						<h3 class="side-title big">BUY A.S.T.R.O. GAME TIME FROM YOUR PHONE</h3>
						<p class="side-copy">
							Skip the queue! Purchase a 30-Min, 60-Min, or 2-Hour A.S.T.R.O. Game-Time Pass online via Square and unlock any O.R.I.O.N. cabinet immediately.
						</p>
						<div class="pass-tiers">
							<div class="tier"><span>30-Min A.S.T.R.O. Session</span><span class="tier-price tone-accent">$12</span></div>
							<div class="tier hot"><span>60-Min Unlimited Pass (Popular)</span><span class="tier-price tone-warn">$20</span></div>
							<div class="tier"><span>2-Hour Marathon Pass</span><span class="tier-price tone-good">$35</span></div>
						</div>
					</div>

					<div class="qr-callout">
						<div>
							<div class="eyebrow accent">SCAN OR VISIT</div>
							<div class="qr-url">beercade.gg/tokens</div>
							<div class="muted-text small">Apple Pay · Google Pay · Square Tap</div>
						</div>
						<div class="qr-box">SQUARE<br />QR PASS<br />[SCAN]</div>
					</div>
				</div>
			</div>
		{/if}
	</main>

	<!-- Bottom Live Ticker Bar -->
	<footer class="ticker">
		<div class="ticker-left">
			<span class="ticker-tag">LIVE VENUE TICKER</span>
			<span class="ticker-text">{announcementBanner}</span>
		</div>
		<div class="ticker-right">
			<span>LIGHTING SCENE: <strong class="accent-text">{lightingPlugin?.state?.active_scene ?? 'arcade_attract'}</strong></span>
			<span>·</span>
			<span class="clock">{clockTime}</span>
		</div>
	</footer>
</div>

<style>
	.tv-shell {
		position: relative;
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		min-height: 100vh;
		overflow: hidden;
		color: var(--text);
		user-select: none;
	}

	.data,
	.initials,
	.score-value,
	.leader-score,
	.prize-value,
	.tier-price,
	.tap-price,
	.qr-url,
	.clock,
	.count-chip,
	.node-chip,
	.status-chip,
	.eyebrow,
	.zone-tag,
	.live-tag,
	.pill,
	.ticker {
		font-family: var(--font-data);
	}

	.strong {
		font-weight: 800;
		color: var(--text-strong);
	}
	.accent-text,
	.tone-accent {
		color: var(--accent);
	}
	.warn-text,
	.tone-warn {
		color: var(--warn);
	}
	.tone-good {
		color: var(--good);
	}
	.tone-special {
		color: var(--special);
	}
	.muted-text {
		color: var(--text-muted);
	}
	.small {
		font-size: 0.8rem;
	}

	/* ---- fanfare ---- */
	.fanfare-scrim {
		position: fixed;
		inset: 0;
		z-index: 50;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 2rem;
		background: rgba(var(--scrim-rgb), 0.92);
	}
	.fanfare-card {
		max-width: 56rem;
		padding: 3rem;
		text-align: center;
		border: 4px solid var(--warn);
		border-radius: 1.5rem;
		background: var(--surface-2);
		box-shadow: 0 0 120px rgba(var(--warn-rgb), 0.5);
	}
	.fanfare-tag {
		display: inline-flex;
		align-items: center;
		gap: 0.75rem;
		margin-bottom: 1rem;
		padding: 0.5rem 1.5rem;
		border-radius: 999px;
		border: 1px solid rgba(var(--warn-rgb), 0.6);
		background: rgba(var(--warn-rgb), 0.18);
		color: var(--warn);
		font-family: var(--font-data);
		font-weight: 800;
		letter-spacing: 0.3em;
	}
	.ping-dot {
		width: 0.75rem;
		height: 0.75rem;
		border-radius: 50%;
		background: var(--warn);
		animation: ping 1s infinite;
	}
	.fanfare-headline {
		margin: 0;
		font-family: var(--font-display);
		font-size: 4.5rem;
		line-height: 1.05;
		color: var(--text-strong);
	}
	.fanfare-sub {
		margin: 1.5rem 0 0;
		font-family: var(--font-data);
		font-size: 2rem;
		font-weight: 700;
		color: var(--warn);
	}
	.fanfare-foot {
		margin-top: 2rem;
		font-size: 0.75rem;
		letter-spacing: 0.15em;
		text-transform: uppercase;
		color: var(--text-soft);
	}

	/* ---- header ---- */
	.tv-header {
		position: relative;
		z-index: 10;
		padding: 1.25rem 2rem;
		border-bottom: 1px solid var(--border);
		background: rgba(var(--surface-rgb), 0.92);
	}
	.header-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 1rem;
	}
	.brand-mark {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 3.5rem;
		height: 3.5rem;
		border-radius: 1rem;
		border: 1px solid rgba(var(--accent-rgb), 0.5);
		background: rgba(var(--accent-rgb), 0.15);
		color: var(--accent);
		font-family: var(--font-display);
		font-size: 1.5rem;
		font-weight: 900;
	}
	.brand-meta {
		display: flex;
		align-items: center;
		gap: 0.6rem;
	}
	.zone-tag {
		padding: 0.1rem 0.6rem;
		border-radius: 4px;
		background: rgba(var(--accent-rgb), 0.18);
		color: var(--accent);
		font-size: 0.7rem;
		font-weight: 700;
		letter-spacing: 0.1em;
	}
	.live-tag {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		font-size: 0.75rem;
		color: var(--good);
	}
	.live-dot {
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 50%;
		background: var(--good);
		animation: pulse 2s infinite;
	}
	.venue-title {
		margin: 0.15rem 0 0;
		font-family: var(--font-display);
		font-size: 1.85rem;
		font-weight: 900;
		color: var(--text-strong);
	}
	.accent {
		color: var(--warn);
	}
	.controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
	}
	.pill {
		min-height: 2.75rem;
		display: inline-flex;
		align-items: center;
		padding: 0 0.9rem;
		border-radius: 0.75rem;
		border: 1px solid var(--border);
		background: var(--surface-2);
		color: var(--text-muted);
		font-size: 0.75rem;
		font-weight: 700;
		text-decoration: none;
		cursor: pointer;
	}
	.pill.active {
		border-color: var(--warn);
		background: rgba(var(--warn-rgb), 0.2);
		color: var(--warn);
	}
	.pill.outline {
		border-color: rgba(var(--accent-rgb), 0.45);
		color: var(--accent);
	}
	.progress {
		height: 4px;
		margin-top: 1rem;
		overflow: hidden;
		border-radius: 999px;
		background: var(--inset);
	}
	.progress-fill {
		height: 100%;
		background: var(--accent);
		transition: width 0.2s linear;
	}

	/* ---- stage & cards ---- */
	.stage {
		position: relative;
		z-index: 10;
		flex: 1;
		display: flex;
		flex-direction: column;
		justify-content: center;
		width: 100%;
		max-width: 1600px;
		margin: 0 auto;
		padding: 2rem;
		box-sizing: border-box;
	}
	.split {
		display: grid;
		grid-template-columns: minmax(0, 2fr) minmax(0, 1fr);
		gap: 2rem;
	}
	.card {
		padding: 2rem;
		border-radius: 1.5rem;
		border: 1px solid var(--border);
		background: rgba(var(--surface-rgb), 0.85);
	}
	.card.warm {
		border-color: rgba(var(--warn-rgb), 0.35);
	}
	.card.cool {
		border-color: rgba(var(--accent-rgb), 0.35);
	}
	.card.special-border {
		border-color: rgba(var(--special-rgb), 0.45);
	}
	.spread {
		display: flex;
		flex-direction: column;
		justify-content: space-between;
	}
	.card-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
		margin-bottom: 1.5rem;
		padding-bottom: 1rem;
		border-bottom: 1px solid var(--border);
	}
	.eyebrow {
		font-size: 0.75rem;
		font-weight: 700;
		letter-spacing: 0.2em;
		text-transform: uppercase;
	}
	.eyebrow.warn {
		color: var(--warn);
	}
	.eyebrow.accent {
		color: var(--accent);
	}
	.eyebrow.special {
		color: var(--special);
	}
	.eyebrow.good {
		color: var(--good);
	}
	.eyebrow.muted {
		color: var(--text-muted);
	}
	.card-title {
		margin: 0.25rem 0 0;
		font-family: var(--font-display);
		font-size: 2.25rem;
		font-weight: 900;
		color: var(--text-strong);
	}
	.count-chip {
		padding: 0.5rem 1rem;
		border-radius: 0.75rem;
		border: 1px solid rgba(var(--warn-rgb), 0.4);
		background: rgba(var(--warn-rgb), 0.1);
		color: var(--warn);
		font-weight: 800;
	}

	/* ---- high scores ---- */
	.score-list {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}
	.score-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1rem 1.5rem;
		border-radius: 1rem;
		border: 1px solid var(--border);
		background: var(--surface-2);
	}
	.score-row.rank-1 {
		border-color: rgba(var(--warn-rgb), 0.6);
		background: rgba(var(--warn-rgb), 0.14);
	}
	.score-row.rank-2 {
		border-color: rgba(var(--accent-rgb), 0.4);
		background: rgba(var(--accent-rgb), 0.08);
	}
	.score-left {
		display: flex;
		align-items: center;
		gap: 1.25rem;
	}
	.rank-badge {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 3rem;
		height: 3rem;
		border-radius: 0.75rem;
		background: var(--surface-3);
		color: var(--text-soft);
		font-family: var(--font-display);
		font-size: 1.25rem;
		font-weight: 900;
	}
	.rank-1 .rank-badge {
		background: var(--warn);
		color: var(--on-accent);
	}
	.rank-2 .rank-badge {
		background: var(--accent);
		color: var(--on-accent);
	}
	.rank-3 .rank-badge {
		background: rgb(var(--special-rgb));
		color: var(--text-strong);
	}
	.score-who {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}
	.initials {
		font-size: 1.5rem;
		font-weight: 900;
		letter-spacing: 0.08em;
		color: var(--text-strong);
	}
	.node-chip {
		padding: 0.1rem 0.6rem;
		border-radius: 4px;
		background: var(--surface-3);
		color: var(--accent);
		font-size: 0.75rem;
		font-weight: 700;
	}
	.score-game {
		margin-top: 0.15rem;
		font-size: 0.9rem;
		font-weight: 600;
		color: var(--text-soft);
	}
	.score-right {
		text-align: right;
	}
	.score-value {
		font-size: 1.9rem;
		font-weight: 900;
		color: var(--warn);
	}
	.score-note {
		font-family: var(--font-data);
		font-size: 0.75rem;
		color: var(--text-muted);
	}

	/* ---- side cards ---- */
	.tag {
		display: inline-block;
		padding: 0.25rem 0.75rem;
		border-radius: 999px;
		font-family: var(--font-data);
		font-size: 0.75rem;
		font-weight: 700;
	}
	.tag.special {
		background: rgba(var(--special-rgb), 0.2);
		color: var(--special);
	}
	.tag.accent {
		background: rgba(var(--accent-rgb), 0.2);
		color: var(--accent);
	}
	.side-title {
		margin: 0.75rem 0 0;
		font-family: var(--font-display);
		font-size: 1.5rem;
		font-weight: 900;
		color: var(--text-strong);
	}
	.side-title.big {
		font-size: 1.9rem;
	}
	.side-copy {
		margin: 0.5rem 0 0;
		font-size: 0.9rem;
		color: var(--text-soft);
	}
	.leader-list,
	.pass-list {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		margin-top: 1.5rem;
	}
	.leader-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.75rem 1rem;
		border-radius: 0.75rem;
		border: 1px solid var(--border);
		background: var(--surface-2);
	}
	.leader-who {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		font-family: var(--font-data);
	}
	.leader-name {
		font-weight: 700;
		color: var(--text-strong);
	}
	.leader-score {
		font-size: 1.1rem;
		font-weight: 900;
		color: var(--accent);
	}
	.callout {
		margin-top: 2rem;
		padding: 1.25rem;
		border-radius: 1rem;
	}
	.callout.good {
		border: 1px solid rgba(var(--good-rgb), 0.3);
		background: rgba(var(--good-rgb), 0.1);
	}
	.callout p {
		margin: 0.25rem 0 0;
		font-size: 0.9rem;
		color: var(--text);
	}

	/* ---- bracket ---- */
	.prize-box {
		padding: 0.75rem 1.25rem;
		border-radius: 1rem;
		border: 1px solid rgba(var(--warn-rgb), 0.4);
		background: rgba(var(--warn-rgb), 0.1);
		text-align: right;
	}
	.prize-value {
		font-size: 1.1rem;
		font-weight: 900;
		color: var(--warn);
	}
	.bracket-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 1.5rem;
	}
	.match {
		padding: 1.5rem;
		border-radius: 1rem;
		border: 1px solid var(--border);
		background: var(--surface-2);
	}
	.match.live {
		border-color: var(--accent);
		background: rgba(var(--accent-rgb), 0.1);
	}
	.match-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 1rem;
	}
	.status-chip {
		padding: 0.25rem 0.75rem;
		border-radius: 999px;
		font-size: 0.75rem;
		font-weight: 900;
		text-transform: uppercase;
	}
	.status-chip.live {
		background: var(--accent);
		color: var(--on-accent);
	}
	.status-chip.done,
	.status-chip.good {
		background: rgba(var(--good-rgb), 0.2);
		color: var(--good);
	}
	.status-chip.warn {
		border: 1px solid rgba(var(--warn-rgb), 0.5);
		background: rgba(var(--warn-rgb), 0.2);
		color: var(--warn);
	}
	.status-chip.accent {
		background: rgba(var(--accent-rgb), 0.2);
		color: var(--accent);
	}
	.player {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1rem 1.25rem;
		border-radius: 0.75rem;
		border: 1px solid var(--border);
		background: var(--surface-3);
		color: var(--text);
		font-family: var(--font-display);
		font-size: 1.5rem;
		font-weight: 900;
	}
	.player.winner {
		border-color: var(--good);
		background: rgba(var(--good-rgb), 0.2);
		color: var(--text-strong);
	}
	.winner-tag {
		padding: 0.25rem 0.6rem;
		border-radius: 4px;
		background: var(--good);
		color: var(--on-accent);
		font-family: var(--font-data);
		font-size: 0.75rem;
	}
	.vs {
		margin: 0.75rem 0;
		text-align: center;
		font-family: var(--font-data);
		font-size: 0.75rem;
		font-weight: 700;
		letter-spacing: 0.2em;
		color: var(--text-faint);
	}

	/* ---- cabinet floor ---- */
	.floor-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 1rem;
	}
	.cab-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 1rem;
	}
	.card.cab {
		padding: 1.5rem;
		border-radius: 1rem;
	}
	.cab-head,
	.pass-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.cab-id {
		font-family: var(--font-display);
		font-size: 1.5rem;
		font-weight: 900;
		color: var(--text-strong);
	}
	.cab-game {
		margin-top: 0.75rem;
		font-size: 1.1rem;
		font-weight: 700;
		color: var(--warn);
	}
	.cab-meta,
	.pass-meta {
		margin-top: 0.25rem;
		font-family: var(--font-data);
		font-size: 0.75rem;
		color: var(--text-muted);
	}
	.pass-meta {
		display: flex;
		justify-content: space-between;
	}
	.cab-pass {
		margin-top: 1rem;
		padding: 0.6rem 1rem;
		border-radius: 0.75rem;
		border: 1px solid rgba(var(--warn-rgb), 0.3);
		background: var(--surface-2);
		font-family: var(--font-data);
		font-size: 0.75rem;
	}
	.pass-row {
		padding: 1rem;
		border-radius: 1rem;
		border: 1px solid var(--border);
		background: var(--surface-2);
	}
	.empty {
		padding: 1.5rem;
		border-radius: 1rem;
		border: 1px solid var(--border);
		text-align: center;
		font-size: 0.9rem;
		color: var(--text-muted);
	}

	/* ---- taproom promo ---- */
	.tap-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 1rem;
		margin-top: 1.5rem;
	}
	.tap {
		padding: 1.25rem;
		border-radius: 1rem;
		border: 1px solid var(--border);
		background: var(--surface-2);
	}
	.tap-head {
		display: flex;
		justify-content: space-between;
		font-size: 0.75rem;
	}
	.tap-price {
		font-size: 1.1rem;
		font-weight: 900;
		color: var(--text-strong);
	}
	.tap-name {
		margin-top: 0.25rem;
		font-family: var(--font-display);
		font-size: 1.25rem;
		font-weight: 900;
		color: var(--text-strong);
	}
	.tap-desc {
		margin-top: 0.25rem;
		font-size: 0.75rem;
		color: var(--text-muted);
	}
	.pass-tiers {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
		margin-top: 1.5rem;
	}
	.tier {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.75rem 1rem;
		border-radius: 0.75rem;
		border: 1px solid var(--border);
		background: var(--surface-2);
		font-family: var(--font-data);
		font-weight: 700;
		color: var(--text-strong);
	}
	.tier.hot {
		border-color: rgba(var(--warn-rgb), 0.5);
		background: rgba(var(--warn-rgb), 0.1);
		color: var(--warn);
	}
	.tier-price {
		font-size: 1.1rem;
		font-weight: 900;
	}
	.qr-callout {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-top: 1.5rem;
		padding: 1rem;
		border-radius: 1rem;
		border: 1px solid rgba(var(--accent-rgb), 0.4);
		background: var(--inset);
	}
	.qr-url {
		font-size: 1.1rem;
		font-weight: 900;
		color: var(--text-strong);
	}
	.qr-box {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 5rem;
		height: 5rem;
		padding: 0.5rem;
		box-sizing: border-box;
		border-radius: 0.75rem;
		border: 2px solid var(--accent);
		background: #ffffff;
		color: #000000;
		font-family: var(--font-data);
		font-size: 0.6rem;
		font-weight: 900;
		line-height: 1.2;
		text-align: center;
	}

	/* ---- ticker ---- */
	.ticker {
		position: relative;
		z-index: 10;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
		padding: 1rem 2rem;
		border-top: 1px solid var(--border);
		background: rgba(var(--surface-rgb), 0.95);
		font-size: 0.75rem;
	}
	.ticker-left,
	.ticker-right {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}
	.ticker-right {
		color: var(--text-muted);
	}
	.ticker-tag {
		padding: 0.25rem 0.6rem;
		border-radius: 4px;
		background: var(--warn);
		color: var(--on-accent);
		font-weight: 900;
	}
	.ticker-text {
		font-weight: 700;
		letter-spacing: 0.05em;
		color: var(--text);
	}
	.clock {
		color: var(--text);
	}

	@keyframes ping {
		75%,
		100% {
			transform: scale(2);
			opacity: 0;
		}
	}
	@keyframes pulse {
		50% {
			opacity: 0.4;
		}
	}

	@media (max-width: 1024px) {
		.split,
		.bracket-grid,
		.cab-grid,
		.tap-grid {
			grid-template-columns: 1fr;
		}
	}

	/* ---- High Score Taproom signature: stripes, bevels, CRT readouts ---- */
	:global([data-theme='taproom']) .tv-header {
		border-bottom: 0;
		padding-bottom: calc(1.25rem + 18px);
	}
	:global([data-theme='taproom']) .tv-header::after {
		content: '';
		position: absolute;
		left: 0;
		right: 0;
		bottom: 0;
		height: 18px;
		background: var(--stripes);
	}
	:global([data-theme='taproom']) :is(.card, .score-row, .leader-row, .match, .tap, .tier, .pass-row, .pill, .brand-mark, .rank-badge, .player, .fanfare-card) {
		border-radius: 0;
	}
	:global([data-theme='taproom']) .card {
		border: 3px solid;
		border-color: var(--bevel-light) var(--bevel-dark) var(--bevel-dark) var(--bevel-light);
		background: var(--surface-2);
	}
	:global([data-theme='taproom']) .score-list {
		padding: 1rem;
		background-color: var(--inset);
		background-image: var(--crt-scanlines);
	}
	:global([data-theme='taproom']) .score-row {
		border: 0;
		background: transparent;
	}
	:global([data-theme='taproom']) :is(.score-value, .leader-score, .initials) {
		font-size: 2.2rem;
	}
	:global([data-theme='taproom']) .tier {
		background: var(--text);
		color: var(--on-accent);
	}
	:global([data-theme='taproom']) .tier-price {
		padding-left: 0.75rem;
		border-left: 3px dashed var(--on-accent);
		color: var(--danger);
	}
	:global([data-theme='taproom']) .progress-fill {
		background: var(--stripes);
	}
	:global([data-theme='taproom']) .ticker {
		border-top: 4px solid var(--accent);
		font-size: 1.15rem;
	}
</style>
