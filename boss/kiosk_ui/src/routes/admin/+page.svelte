<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import PinKeypadModal from '$lib/components/PinKeypadModal.svelte';
	import {
		cabinetAddCredits,
		cabinetLaunchGame,
		cabinetPing,
		cabinetReboot,
		discoverFleetNodes,
		fetchAdminOverview,
		fetchNetworkInterfaces,
		fetchNetplaySessions,
		fetchOtaPayloads,
		forceConnectNode,
		getApiBase,
		getStoredOperatorToken,
		invokePluginAction,
		logoutOperator,
		registerWebhookPlugin,
		togglePlugin,
		type DiscoveredNode,
		type DiscoveryReport,
		type NetworkInterfacesReport
	} from '$lib/bossApi';
	import type {
		AdminOverview,
		NetplaySessionSummary,
		OtaPayloadEntry,
		PluginDescriptor
	} from '$lib/types/telemetry';
	import { THEMES, currentTheme, setVenueTheme } from '$lib/theme/themes';

	const IDLE_LOCK_SECONDS = 300; // 5 minutes inactivity lock

	let authenticated = $state(false);
	let unlockModalOpen = $state(false);
	let overview = $state<AdminOverview | null>(null);
	let simulatedMode = $state(false);
	let netplaySessions = $state<NetplaySessionSummary[]>([]);
	let otaPayloads = $state<OtaPayloadEntry[]>([]);
	let activeTab = $state<'fleet' | 'plugins' | 'netplay_ota' | 'audit'>('fleet');
	let selectedPluginId = $state<string>('pos-payments');
	let idleSecondsRemaining = $state<number>(IDLE_LOCK_SECONDS);
	let toastMessage = $state<string>('');
	let liveSseEvents = $state<Array<{ channel: string; summary: string; at: string }>>([]);
	let venueTheme = $state<string>(currentTheme());

	async function handleThemeChange(id: string) {
		const ok = await setVenueTheme(id);
		if (ok) {
			venueTheme = id;
			const label = THEMES.find((t) => t.id === id)?.label ?? id;
			showToast(`Venue theme switched to ${label} on every screen`);
		} else {
			venueTheme = currentTheme();
			showToast('Could not change the venue theme');
		}
	}

	// Launch Game RPC modal state
	let launchNodeTarget = $state<string>('ASTRO-01');
	let launchCore = $state<string>('fbneo_libretro');
	let launchRom = $state<string>('sfiii3.zip');
	let launchNetplay = $state<boolean>(true);

	// Plugin-specific form inputs
	let posTargetNode = $state<string>('ASTRO-01');
	let posTapCredits = $state<number>(5);
	let posTapProvider = $state<string>('Stripe WisePOS E');

	let newFixtureName = $state<string>('VIP Booth WLED Strip');
	let newFixtureEndpoint = $state<string>('http://10.10.20.65/json/state');

	let newPlayerName = $state<string>('');
	let tourneyTitle = $state<string>('FRIDAY NIGHT FIGHTS: STREET FIGHTER III 3RD STRIKE');
	let tourneyGame = $state<string>('Street Fighter III: 3rd Strike');
	let tourneyRom = $state<string>('sfiii3');
	let tourneyPrize = $state<string>('$150 Bar Tab + Champion Belt + 50 Tokens');

	let rfidTapNode = $state<string>('ASTRO-01');
	let newCardUid = $state<string>('04:B8:55:19:C1:09:80');
	let newCardHolder = $state<string>('Patron Wristband #210');
	let newCardInitials = $state<string>('PLY');
	let newCardCredits = $state<number>(15);
	let newCardVip = $state<boolean>(false);

	let thermalThresholdInput = $state<number>(78);
	let newSinkName = $state<string>('Telegram #ops-bot');
	let newSinkKind = $state<string>('telegram');
	let newSinkUrl = $state<string>('https://api.telegram.org/bot000/sendMessage');

	// Custom webhook plugin registration form
	let webhookModalOpen = $state(false);
	let whId = $state('custom-bar-pos-sync');
	let whName = $state('Lightspeed Bar POS Webhook');
	let whUrl = $state('http://10.10.20.90:4000/boss-events');
	let whDesc = $state('Streams high scores and credit transactions to bar TV ticker.');

	// Fleet Discovery & Force Connect state
	let isScanning = $state(false);
	let isForceConnecting = $state(false);
	let forceConnectTarget = $state('');
	let forceConnectBossUrl = $state('');
	let customSubnetInput = $state('');
	let discoveryReport = $state<DiscoveryReport | null>(null);
	let detectedBossUrl = $state('http://boss:8989');
	let detectedInterfaces = $state<NetworkInterfacesReport['interfaces']>([]);

	let selectedPlugin = $derived<PluginDescriptor | undefined>(
		(overview?.plugins ?? []).find((p) => p.manifest.id === selectedPluginId) ??
			overview?.plugins?.[0]
	);

	function showToast(msg: string) {
		toastMessage = msg;
		setTimeout(() => {
			if (toastMessage === msg) toastMessage = '';
		}, 3500);
	}

	function resetIdleTimer() {
		idleSecondsRemaining = IDLE_LOCK_SECONDS;
	}

	async function refreshAll() {
		const res = await fetchAdminOverview();
		overview = res.overview;
		simulatedMode = res.simulated;
		netplaySessions = await fetchNetplaySessions();
		otaPayloads = await fetchOtaPayloads();
		const alertPlugin = res.overview.plugins.find((p) => p.manifest.id === 'alerting-sinks');
		if (alertPlugin?.state?.thermal_warn_temp_c) {
			thermalThresholdInput = alertPlugin.state.thermal_warn_temp_c;
		}

		fetchNetworkInterfaces().then((rep) => {
			detectedBossUrl = rep.default_boss_url;
			detectedInterfaces = rep.interfaces;
			if (!forceConnectBossUrl) {
				forceConnectBossUrl = rep.default_boss_url;
			}
		});
	}

	async function handleForceConnect(e?: SubmitEvent) {
		if (e) e.preventDefault();
		if (!forceConnectTarget.trim()) {
			showToast('Please enter an IP address or hostname to connect');
			return;
		}
		isForceConnecting = true;
		showToast(`Attempting connection to ${forceConnectTarget}...`);
		const res = await forceConnectNode(forceConnectTarget, forceConnectBossUrl || detectedBossUrl);
		isForceConnecting = false;
		if (res.status === 'ok') {
			showToast(`✓ Connected to ASTRO node ${res.node?.node_id || forceConnectTarget}`);
			forceConnectTarget = '';
			await refreshAll();
		} else {
			showToast(`⚠️ Connection failed: ${res.error || res.message || 'Unknown error'}`);
		}
	}

	async function handleSubnetArpScan() {
		isScanning = true;
		showToast('Scanning ARP cache and local subnets for ASTRO nodes...');
		const report = await discoverFleetNodes(customSubnetInput, forceConnectBossUrl || detectedBossUrl);
		isScanning = false;
		discoveryReport = report;
		showToast(report.message);
		await refreshAll();
	}

	async function handleConnectDiscovered(node: DiscoveredNode) {
		showToast(`Connecting to ${node.node_id} at ${node.ip}:${node.port}...`);
		const res = await forceConnectNode(`${node.ip}:${node.port}`, forceConnectBossUrl || detectedBossUrl);
		if (res.status === 'ok') {
			showToast(`✓ Linked ${node.node_id}`);
			await refreshAll();
			if (discoveryReport) {
				const n = discoveryReport.nodes.find((item) => item.node_id === node.node_id);
				if (n) n.connected = true;
			}
		} else {
			showToast(`Failed: ${res.error || res.message}`);
		}
	}

	async function handleLockKiosk() {
		await logoutOperator();
		authenticated = false;
		goto('/');
	}

	// Cabinet RPC Actions
	async function handleAddCredits(nodeId: string, credits: number) {
		const res = await cabinetAddCredits(nodeId, credits);
		showToast(`Queued +${credits} credits for ${nodeId} (${res.command_id ?? 'ok'})`);
		await refreshAll();
	}

	async function handleReboot(nodeId: string) {
		const res = await cabinetReboot(nodeId, 5);
		showToast(`Queued 5s controlled reboot for ${nodeId} (${res.command_id ?? 'ok'})`);
		await refreshAll();
	}

	async function handlePing(nodeId: string) {
		const res = await cabinetPing(nodeId);
		showToast(`Dispatched RPC Ping to ${nodeId} (${res.command_id ?? 'ok'})`);
		await refreshAll();
	}

	async function handleLaunchGame(e: SubmitEvent) {
		e.preventDefault();
		const res = await cabinetLaunchGame(launchNodeTarget, launchCore, launchRom, launchNetplay);
		showToast(
			`Queued LaunchGame (${launchRom}) on ${launchNodeTarget} (${res.command_id ?? 'ok'})`
		);
		await refreshAll();
	}

	// Plugin Actions
	async function handleTogglePlugin(plugin: PluginDescriptor) {
		const next = !plugin.manifest.enabled;
		await togglePlugin(plugin.manifest.id, next);
		plugin.manifest.enabled = next;
		showToast(`${plugin.manifest.name} ${next ? 'ENABLED' : 'DISABLED'}`);
		await refreshAll();
	}

	async function runPluginAction(pluginId: string, action: string, payload: Record<string, any>) {
		const res = await invokePluginAction(pluginId, action, payload);
		if (res.ok) {
			showToast(`Plugin [${pluginId}] → ${action} executed`);
			await refreshAll();
		} else {
			showToast(`Error: ${res.detail || 'Plugin action failed'}`);
		}
	}

	async function handleRegisterWebhook(e: SubmitEvent) {
		e.preventDefault();
		await registerWebhookPlugin({
			id: whId.trim(),
			name: whName.trim(),
			version: '1.0.0',
			category: 'webhook_custom',
			description: whDesc.trim(),
			author: 'Venue Operator',
			enabled: true,
			builtin: false,
			subscribed_events: ['high_score', 'credit_transaction', 'error_reported', 'node_offline'],
			capabilities: ['webhook_post'],
			ui_panel: {
				tab_label: whName.trim().slice(0, 18),
				icon: 'webhook',
				summary_metric_key: 'dispatched_count'
			},
			webhook_url: whUrl.trim()
		});
		webhookModalOpen = false;
		showToast(`Registered webhook plugin: ${whName}`);
		await refreshAll();
	}

	function formatIdleTime(sec: number): string {
		const m = Math.floor(sec / 60);
		const s = sec % 60;
		return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
	}

	onMount(() => {
		const existingToken = getStoredOperatorToken();
		if (existingToken) {
			authenticated = true;
			refreshAll();
		} else {
			unlockModalOpen = true;
		}

		const activityEvents = ['pointerdown', 'keydown', 'touchstart'];
		activityEvents.forEach((ev) => window.addEventListener(ev, resetIdleTimer));

		const timer = setInterval(() => {
			if (!authenticated) return;
			idleSecondsRemaining -= 1;
			if (idleSecondsRemaining <= 0) {
				handleLockKiosk();
			}
		}, 1000);

		let es: EventSource | null = null;
		try {
			es = new EventSource(
				`${getApiBase()}/api/telemetry/stream?channels=venue,alerts,commands,plugins`
			);
			for (const ch of ['venue', 'alerts', 'commands', 'plugins']) {
				es.addEventListener(ch, (ev) => {
					const raw = (ev as MessageEvent).data;
					if (ch !== 'venue') {
						liveSseEvents = [
							{
								channel: ch.toUpperCase(),
								summary: raw.slice(0, 140),
								at: new Date().toLocaleTimeString()
							},
							...liveSseEvents.slice(0, 29)
						];
						refreshAll();
					}
				});
			}
		} catch {
			// SSE optional when offline
		}

		return () => {
			activityEvents.forEach((ev) => window.removeEventListener(ev, resetIdleTimer));
			clearInterval(timer);
			es?.close();
		};
	});
</script>

<svelte:head>
	<title>B.O.S.S. Supervisory & Plugin Console</title>
</svelte:head>

<div class="admin-shell">
	<!-- Top Supervisory Header -->
	<header class="admin-topbar">
		<div class="brand-left">
			<span class="boss-badge">B.O.S.S. v1.0</span>
			<div>
				<h1>OPERATIONS & SUPERVISORY CONSOLE</h1>
				<p>{overview?.venue ?? 'Beercade Redfern (The BK)'} · {overview?.address ?? ''}</p>
			</div>
		</div>

		<div class="topbar-controls">
			<span class="stream-indicator {simulatedMode ? 'sim' : 'live'}">
				{simulatedMode ? '● PREVIEW SIMULATION' : '● LIVE TELEMETRY & SSE'}
			</span>
			<span class="idle-timer" title="Auto-locks kiosk after 5 minutes of inactivity">
				AUTO-LOCK: <strong>{formatIdleTime(idleSecondsRemaining)}</strong>
			</span>
			{#if authenticated}
				<label class="theme-picker">
					<span>VENUE THEME</span>
					<select
						value={venueTheme}
						onchange={(e) => handleThemeChange((e.currentTarget as HTMLSelectElement).value)}
					>
						{#each THEMES as t}
							<option value={t.id}>{t.label}</option>
						{/each}
					</select>
				</label>
			{/if}
			<a href="/" class="nav-btn">← PATRON KIOSK</a>
			<button type="button" class="lock-btn" onclick={handleLockKiosk}>
				🔒 LOCK SESSION
			</button>
		</div>
	</header>

	{#if toastMessage}
		<div class="toast-banner" role="status">{toastMessage}</div>
	{/if}

	{#if !authenticated}
		<div class="locked-placeholder">
			<h2>OPERATOR AUTHENTICATION REQUIRED</h2>
			<p>Unlock with your 6-digit physical kiosk PIN or LAN <code>BOSS_RPC_TOKEN</code>.</p>
			<button type="button" class="action-btn primary" onclick={() => (unlockModalOpen = true)}>
				OPEN OPERATOR UNLOCK PAD
			</button>
		</div>
	{:else if overview}
		<!-- KPI Summary Strip -->
		<section class="kpi-strip">
			<div class="kpi-card">
				<span class="kpi-label">CABINET FLEET</span>
				<strong class="kpi-val cyan">{overview.nodes_online} / {overview.nodes.length} ONLINE</strong>
				<small>15s Staleness Watchdog</small>
			</div>
			<div class="kpi-card">
				<span class="kpi-label">ACTIVE PLUGINS</span>
				<strong class="kpi-val green">
					{overview.plugins.filter((p) => p.manifest.enabled).length} / {overview.plugins.length} ENABLED
				</strong>
				<small>Hybrid Event-Hook Bus</small>
			</div>
			<div class="kpi-card">
				<span class="kpi-label">NETPLAY RELAY (UDP:9750)</span>
				<strong class="kpi-val amber">{netplaySessions.length} SESSIONS</strong>
				<small>OR Protocol P2P Arbiter</small>
			</div>
			<div class="kpi-card">
				<span class="kpi-label">CLOUDFLARE D1 EDGE</span>
				<strong class="kpi-val">
					{overview.edge_sync.enabled ? 'CONNECTED' : 'LOCAL ONLY'}
				</strong>
				<small>{overview.edge_sync.buffered_items} items buffered</small>
			</div>
		</section>

		<!-- Primary Navigation Tabs -->
		<nav class="console-tabs">
			<button
				type="button"
				class="tab-btn {activeTab === 'fleet' ? 'active' : ''}"
				onclick={() => (activeTab = 'fleet')}
			>
				1. FLEET & CABINET RPC ({overview.nodes.length})
			</button>
			<button
				type="button"
				class="tab-btn {activeTab === 'plugins' ? 'active' : ''}"
				onclick={() => (activeTab = 'plugins')}
			>
				2. PLUGIN HUB ({overview.plugins.length})
			</button>
			<button
				type="button"
				class="tab-btn {activeTab === 'netplay_ota' ? 'active' : ''}"
				onclick={() => (activeTab = 'netplay_ota')}
			>
				3. NETPLAY RELAY & OTA SYNC
			</button>
			<button
				type="button"
				class="tab-btn {activeTab === 'audit' ? 'active' : ''}"
				onclick={() => (activeTab = 'audit')}
			>
				4. EVENT BUS & ERROR LOG
			</button>
		</nav>

		<!-- =================================================================== -->
		<!-- TAB 1: FLEET & CABINET RPC CONTROL                                  -->
		<!-- =================================================================== -->
		{#if activeTab === 'fleet'}
			<!-- Empty Fleet Notice -->
			{#if overview.nodes.length === 0}
				<div class="empty-fleet-alert">
					<div class="alert-icon">⚠️</div>
					<div class="alert-body">
						<h3>NO ASTRO STATIONS CURRENTLY LINKED</h3>
						<p>
							Nodes running on a virtual network (e.g. <code>BEERCADE</code>) or unconfigured subnet may not have discovered B.O.S.S.
							Use the <strong>Fleet Discovery & Rescue Console</strong> below to search ARP tables and subnets, or force a direct connection.
						</p>
					</div>
				</div>
			{/if}

			<!-- ASTRO Fleet Discovery & Rescue Console -->
			<div class="discovery-card">
				<div class="discovery-head">
					<div class="dh-title">
						<span class="dh-icon">🛰️</span>
						<div>
							<h3>ASTRO NODE DISCOVERY & FLEET RESCUE</h3>
							<p>Search ARP tables & subnets or force direct connection to lost or unlinked ASTRO cabinets</p>
						</div>
					</div>
					{#if detectedInterfaces.length > 0}
						<div class="iface-badges">
							{#each detectedInterfaces as iface}
								<span class="iface-badge {iface.is_default ? 'default' : ''}" title="{iface.name}: {iface.ip}">
									{iface.subnet} ({iface.ip})
								</span>
							{/each}
						</div>
					{/if}
				</div>

				<div class="discovery-grid">
					<!-- Box 1: Subnet & ARP Table Sweep -->
					<div class="disc-box">
						<div class="disc-box-title">
							<h4>1. SUB-NET & ARP TABLE SWEEP</h4>
							<span class="tag-auto">AUTO-DISCOVER</span>
						</div>
						<p class="disc-box-desc">
							Queries kernel ARP cache (<code>/proc/net/arp</code>) and broadcasts discovery probes on UDP:8990 across candidate subnets.
						</p>
						<div class="disc-input-row">
							<input
								type="text"
								placeholder="Subnet CIDR (e.g. 172.28.0.0/24 or blank for auto)"
								bind:value={customSubnetInput}
								class="disc-text-input"
							/>
							<button
								type="button"
								class="action-btn cyan {isScanning ? 'busy' : ''}"
								onclick={handleSubnetArpScan}
								disabled={isScanning}
							>
								{isScanning ? '⏳ SCANNING...' : '🔍 SCAN SUBNETS & ARP'}
							</button>
						</div>
					</div>

					<!-- Box 2: Force Direct Connection -->
					<form class="disc-box" onsubmit={handleForceConnect}>
						<div class="disc-box-title">
							<h4>2. FORCE DIRECT NODE CONNECTION</h4>
							<span class="tag-manual">DIRECT LINK</span>
						</div>
						<p class="disc-box-desc">
							Instructs the ASTRO node on inbound port 8990 to link to this B.O.S.S. instance immediately.
						</p>
						<div class="disc-input-row">
							<input
								type="text"
								placeholder="Target IP / Host (e.g. 172.28.0.3, astro, 192.168.1.50:8990)"
								bind:value={forceConnectTarget}
								class="disc-text-input"
							/>
							<button
								type="submit"
								class="action-btn primary {isForceConnecting ? 'busy' : ''}"
								disabled={isForceConnecting}
							>
								{isForceConnecting ? '⏳ CONNECTING...' : '⚡ FORCE CONNECT'}
							</button>
						</div>
						<div class="disc-sub-field">
							<span>Callback BOSS URL:</span>
							<input
								type="text"
								bind:value={forceConnectBossUrl}
								placeholder={detectedBossUrl}
								class="disc-small-input"
							/>
						</div>
					</form>
				</div>

				<!-- Discovery Results Strip -->
				{#if discoveryReport}
					<div class="discovery-results">
						<div class="d-results-banner">
							<strong>SCAN RESULT:</strong> {discoveryReport.message}
							<small>({discoveryReport.scanned_ips} IPs checked · {discoveryReport.arp_entries_found} ARP entries · {discoveryReport.duration_ms}ms)</small>
						</div>
						{#if discoveryReport.nodes.length > 0}
							<div class="discovered-nodes-row">
								{#each discoveryReport.nodes as node}
									<div class="disc-node-card {node.connected ? 'linked' : 'unlinked'}">
										<div class="dnc-meta">
											<strong class="dnc-id">{node.node_id}</strong>
											<span class="dnc-ip">{node.ip}:{node.port}</span>
											<span class="dnc-hw">{node.hardware || 'Generic'} ({node.soc || ''})</span>
										</div>
										<div class="dnc-action">
											{#if node.connected}
												<span class="linked-tag">✓ LINKED</span>
											{:else}
												<button
													type="button"
													class="action-btn green small"
													onclick={() => handleConnectDiscovered(node)}
												>
													CONNECT
												</button>
											{/if}
										</div>
									</div>
								{/each}
							</div>
						{/if}
					</div>
				{/if}
			</div>

			<div class="fleet-layout">
				<div class="fleet-cards">
					{#each overview.nodes as entry}
						{@const t = entry.telemetry}
						<div class="node-card {entry.online ? 'online' : 'offline'}">
							<div class="node-head">
								<div>
									<span class="node-status-dot {entry.online ? 'on' : 'off'}"></span>
									<strong class="node-title">{t.node_id}</strong>
									<span class="node-hw">{t.hardware} ({t.soc})</span>
								</div>
								<span class="credits-badge">
									{t.credits.credits_available} CR AVAIL ({t.credits.credits_dispensed_today} TODAY)
								</span>
							</div>

							<div class="telemetry-metrics">
								<div class="metric-box">
									<span>CPU / GPU TEMP</span>
									<strong class={t.cpu_temp_c >= 75 ? 'warn-text' : ''}>
										{t.cpu_temp_c.toFixed(1)}°C / {t.gpu_temp_c.toFixed(1)}°C
									</strong>
								</div>
								<div class="metric-box">
									<span>ACTIVE CORE / ROM</span>
									<strong>{t.active_core} · {t.active_rom}</strong>
								</div>
								<div class="metric-box">
									<span>FPS / FRAME TIME</span>
									<strong>{t.fps.toFixed(1)} fps ({t.frame_time_ms.toFixed(2)} ms)</strong>
								</div>
								<div class="metric-box">
									<span>1000HZ JITTER (p50/p99)</span>
									<strong>
										{t.input_jitter.p50_us.toFixed(0)}μs / {t.input_jitter.p99_us.toFixed(0)}μs
									</strong>
								</div>
							</div>

							<div class="rpc-toolbar">
								<button
									type="button"
									class="action-btn green"
									onclick={() => handleAddCredits(t.node_id, 1)}
								>
									+1 CREDIT
								</button>
								<button
									type="button"
									class="action-btn green"
									onclick={() => handleAddCredits(t.node_id, 5)}
								>
									+5 CREDITS
								</button>
								<button
									type="button"
									class="action-btn primary"
									onclick={() => {
										launchNodeTarget = t.node_id;
										launchCore = t.active_core;
										launchRom = t.active_rom;
									}}
								>
									SELECT FOR LAUNCH
								</button>
								<button
									type="button"
									class="action-btn"
									onclick={() => handlePing(t.node_id)}
								>
									PING
								</button>
								<button
									type="button"
									class="action-btn danger"
									onclick={() => handleReboot(t.node_id)}
								>
									REBOOT (5s)
								</button>
							</div>
						</div>
					{/each}
				</div>

				<!-- Right Sidebar: Remote Game Launcher & Command Queue -->
				<aside class="fleet-sidebar">
					<form class="side-panel" onsubmit={handleLaunchGame}>
						<h3>REMOTE GAME LAUNCHER (RPC)</h3>
						<label class="field">
							<span>Target Cabinet Node</span>
							<select bind:value={launchNodeTarget}>
								{#each overview.nodes as n}
									<option value={n.telemetry.node_id}>{n.telemetry.node_id}</option>
								{/each}
							</select>
						</label>
						<label class="field">
							<span>Libretro Core</span>
							<input type="text" bind:value={launchCore} />
						</label>
						<label class="field">
							<span>ROM Archive Filename</span>
							<input type="text" bind:value={launchRom} />
						</label>
						<label class="checkbox-field">
							<input type="checkbox" bind:checked={launchNetplay} />
							<span>Enable A.S.T.R.O. P2P Rollback Netplay</span>
						</label>
						<button type="submit" class="action-btn primary full">
							🚀 DISPATCH LAUNCH_GAME RPC
						</button>
					</form>

					<div class="side-panel">
						<h3>COMMAND LIFECYCLE QUEUE</h3>
						<div class="cmd-list">
							{#each overview.commands.slice(0, 8) as cmd}
								<div class="cmd-row">
									<div>
										<strong>{cmd.node_id}</strong> ·
										<code>{cmd.command.type}</code>
									</div>
									<span class="cmd-status status-{cmd.status}">{cmd.status}</span>
								</div>
							{/each}
						</div>
					</div>
				</aside>
			</div>

		<!-- =================================================================== -->
		<!-- TAB 2: HYBRID PLUGIN SYSTEM HUB (5 FIRST-PARTY PANELS + WEBHOOKS)   -->
		<!-- =================================================================== -->
		{:else if activeTab === 'plugins'}
			<div class="plugin-hub-layout">
				<!-- Left Plugin Selector List -->
				<aside class="plugin-sidebar">
					<div class="plugin-sidebar-head">
						<h3>INSTALLED PLUGINS</h3>
						<button
							type="button"
							class="action-btn primary small"
							onclick={() => (webhookModalOpen = true)}
						>
							+ WEBHOOK
						</button>
					</div>

					{#each overview.plugins as p}
						<div
							class="plugin-nav-item {selectedPlugin?.manifest.id === p.manifest.id ? 'active' : ''}"
						>
							<button
								type="button"
								class="plugin-select-btn"
								onclick={() => (selectedPluginId = p.manifest.id)}
							>
								<div class="plugin-nav-title">
									<strong>{p.manifest.ui_panel.tab_label}</strong>
									<small>v{p.manifest.version}</small>
								</div>
								<span class="plugin-cat-tag">{p.manifest.id}</span>
							</button>
							<button
								type="button"
								class="toggle-pill {p.manifest.enabled ? 'on' : 'off'}"
								onclick={() => handleTogglePlugin(p)}
							>
								{p.manifest.enabled ? 'ON' : 'OFF'}
							</button>
						</div>
					{/each}
				</aside>

				<!-- Right Active Plugin Interactive Panel -->
				{#if selectedPlugin}
					<section class="plugin-detail-pane">
						<div class="plugin-pane-header">
							<div>
								<span class="plugin-id-badge">{selectedPlugin.manifest.id}</span>
								<h2>{selectedPlugin.manifest.name}</h2>
								<p>{selectedPlugin.manifest.description}</p>
							</div>
							<div class="plugin-meta-pills">
								{#each selectedPlugin.manifest.subscribed_events as ev}
									<span class="event-pill">⚡ {ev}</span>
								{/each}
							</div>
						</div>

						<!-- PANEL 1: POS & PAYMENT TERMINALS (SQUARE + A.S.T.R.O. GAME-TIME PASSES) -->
						{#if selectedPlugin.manifest.id === 'pos-payments'}
							<div class="panel-grid-2">
								<div class="sub-box">
									<h3>SQUARE POS & CONNECTED TERMINALS</h3>
									{#if selectedPlugin.state.square_config}
										<div class="list-item-row" style="border-color: rgba(var(--accent-rgb), 0.4);">
											<div>
												<strong>Square Location: {selectedPlugin.state.square_config.location_id}</strong>
												<small>
													Device: {selectedPlugin.state.square_config.terminal_device_id} · Mode: {selectedPlugin.state.square_config.environment.toUpperCase()}
												</small>
											</div>
											<span class="status-pill online">SQUARE LINKED</span>
										</div>
									{/if}
									{#each selectedPlugin.state.terminals ?? [] as term}
										<div class="list-item-row">
											<div>
												<strong>{term.label}</strong>
												<small>{term.provider} · {term.location}</small>
											</div>
											<span class="status-pill online">{term.status.toUpperCase()}</span>
										</div>
									{/each}

									<h3 style="margin-top: 1.25rem;">ISSUE SQUARE A.S.T.R.O. GAME-TIME PASS</h3>
									<div class="inline-controls">
										<select bind:value={posTargetNode}>
											{#each overview.nodes as n}
												<option value={n.telemetry.node_id}>{n.telemetry.node_id}</option>
											{/each}
										</select>
										<button
											type="button"
											class="action-btn primary"
											onclick={() =>
												runPluginAction('pos-payments', 'activate_game_time_pass', {
													node_id: posTargetNode,
													duration_mins: 60,
													credits: 25,
													amount_cents: 2000,
													patron_name: 'Square Walk-In Pass',
													provider: 'Square POS Terminal'
												})}
										>
											⏱️ ACTIVATE 60M A.S.T.R.O. PASS ($20)
										</button>
									</div>

									<h3 style="margin-top: 1.25rem;">SIMULATE TERMINAL / TAP-TO-PAY CREDITS</h3>
									<div class="inline-controls">
										<select bind:value={posTargetNode}>
											{#each overview.nodes as n}
												<option value={n.telemetry.node_id}>{n.telemetry.node_id}</option>
											{/each}
										</select>
										<input
											type="number"
											min="1"
											max="50"
											bind:value={posTapCredits}
											style="width: 80px;"
										/>
										<button
											type="button"
											class="action-btn green"
											onclick={() =>
												runPluginAction('pos-payments', 'simulate_tap_pay', {
													node_id: posTargetNode,
													credits: Number(posTapCredits),
													amount_cents: Number(posTapCredits) * 100,
													provider: 'Square Terminal NFC'
												})}
										>
											💳 SETTLE & INJECT CREDITS
										</button>
									</div>
								</div>

								<div class="sub-box">
									<h3>ACTIVE SQUARE A.S.T.R.O. TIMED SESSIONS</h3>
									{#each selectedPlugin.state.active_timed_sessions ?? [] as sess}
										<div class="list-item-row">
											<div>
												<strong>{sess.node_id} · {sess.patron_name}</strong>
												<small>{sess.pass_code} · {sess.duration_mins}m ({sess.provider})</small>
											</div>
											<span class="status-pill online">{sess.status.toUpperCase()}</span>
										</div>
									{/each}

									<h3 style="margin-top: 1.25rem;">DAILY RECONCILIATION & POS LEDGER</h3>
									<div class="mini-kpis">
										<div>
											<span>REVENUE</span>
											<strong>
												${((selectedPlugin.state.total_revenue_cents ?? 0) / 100).toFixed(2)}
											</strong>
										</div>
										<div>
											<span>CREDITS SOLD</span>
											<strong>{selectedPlugin.state.total_credits_sold ?? 0}</strong>
										</div>
										<div>
											<span>TRANSACTIONS</span>
											<strong>{selectedPlugin.state.transactions_count ?? 0}</strong>
										</div>
									</div>

									<div class="scroll-list">
										{#each selectedPlugin.state.ledger ?? [] as entry}
											<div class="list-item-row">
												<div>
													<strong>{entry.node_id} (+{entry.credits} CR)</strong>
													<small>{entry.provider} · Ref: {entry.reference}</small>
												</div>
												<strong>${(entry.amount_cents / 100).toFixed(2)}</strong>
											</div>
										{/each}
									</div>
								</div>
							</div>

						<!-- PANEL 2: MULTI-ZONE / MULTI-SYSTEM VENUE LIGHTING -->
						{:else if selectedPlugin.manifest.id === 'venue-lighting'}
							<div class="panel-grid-2">
								<div class="sub-box">
									<h3>GLOBAL SCENE OVERRIDE (ALL SYSTEMS)</h3>
									<div class="scene-grid">
										{#each selectedPlugin.state.available_scenes ?? [] as sc}
											<button
												type="button"
												class="scene-btn {selectedPlugin.state.active_scene === sc.id
													? 'active'
													: ''}"
												style="--scene-color: {sc.color}"
												onclick={() =>
													runPluginAction('venue-lighting', 'set_scene', {
														scene: sc.id,
														brightness: selectedPlugin.state.master_brightness_pct
													})}
											>
												<span class="color-swatch" style="background:{sc.color}"></span>
												<strong>{sc.label}</strong>
											</button>
										{/each}
									</div>

									<h3 style="margin-top: 1.25rem;">MULTI-ZONE INDEPENDENT LIGHTING CONTROL</h3>
									{#each selectedPlugin.state.zones ?? [] as z}
										<div class="list-item-row">
											<div>
												<strong>{z.name}</strong>
												<small>Zone: {z.id} · Scene: {z.active_scene} ({z.brightness_pct}%)</small>
											</div>
											<div class="inline-controls">
												<button
													type="button"
													class="action-btn small"
													onclick={() =>
														runPluginAction('venue-lighting', 'set_zone_scene', {
															zone_id: z.id,
															scene: 'arcade_attract'
														})}
												>
													ATTRACT
												</button>
												<button
													type="button"
													class="action-btn small"
													onclick={() =>
														runPluginAction('venue-lighting', 'set_zone_scene', {
															zone_id: z.id,
															scene: 'cyberpunk_neon'
														})}
												>
													NEON
												</button>
												<button
													type="button"
													class="action-btn small"
													onclick={() =>
														runPluginAction('venue-lighting', 'set_zone_scene', {
															zone_id: z.id,
															scene: 'chill_lounge'
														})}
												>
													LOUNGE
												</button>
											</div>
										</div>
									{/each}

									<h3 style="margin-top: 1.25rem;">REACTIVE EVENT RULES</h3>
									<div class="rule-toggles">
										{#each [
											['strobe_on_high_score', 'High Score Celebration Strobe'],
											['pulse_on_credit_insert', 'Coin Drop WLED Ripple'],
											['red_alert_on_node_offline', 'Red Alert on Node Offline / Fatal']
										] as [ruleKey, ruleLabel]}
											{@const active = selectedPlugin.state.reactive_rules?.[ruleKey]}
											<div class="list-item-row">
												<span>{ruleLabel}</span>
												<button
													type="button"
													class="toggle-pill {active ? 'on' : 'off'}"
													onclick={() =>
														runPluginAction('venue-lighting', 'toggle_rule', {
															rule: ruleKey,
															enabled: !active
														})}
												>
													{active ? 'ENABLED' : 'OFF'}
												</button>
											</div>
										{/each}
									</div>
								</div>

								<div class="sub-box">
									<h3>MULTI-PROTOCOL CONTROLLERS (WLED / ART-NET / sACN / HUE)</h3>
									{#each selectedPlugin.state.fixtures ?? [] as fx}
										<div class="list-item-row">
											<div>
												<strong>{fx.name} (U{fx.universe})</strong>
												<small>
													[{fx.zone ?? 'arcade_floor'}] {fx.protocol} · {fx.endpoint}
												</small>
											</div>
											<span class="status-pill online">{fx.online ? 'ONLINE' : 'STANDBY'}</span>
										</div>
									{/each}

									<div class="inline-controls" style="margin-top:0.85rem;">
										<input
											type="text"
											placeholder="Controller Name"
											bind:value={newFixtureName}
										/>
										<input
											type="text"
											placeholder="WLED / Art-Net / sACN / Hue Endpoint"
											bind:value={newFixtureEndpoint}
										/>
										<button
											type="button"
											class="action-btn primary"
											onclick={() =>
												runPluginAction('venue-lighting', 'add_fixture', {
													name: newFixtureName,
													endpoint: newFixtureEndpoint,
													protocol: 'WLED JSON API',
													zone: 'arcade_floor',
													universe: 7
												})}
										>
											+ ADD CONTROLLER
										</button>
									</div>

									<h3 style="margin-top: 1.25rem;">RECENT LIGHTING CUE LOG</h3>
									<div class="scroll-list">
										{#each selectedPlugin.state.cue_history ?? [] as cue}
											<div class="list-item-row">
												<div>
													<strong>{cue.scene} [{cue.zone ?? 'ALL'}]</strong>
													<small>{cue.reason}</small>
												</div>
											</div>
										{/each}
									</div>
								</div>
							</div>

						<!-- PANEL 3: TOURNAMENT, HIGH-SCORE & VENUE TV SIGNAGE ENGINE -->
						{:else if selectedPlugin.manifest.id === 'tournament-engine'}
							<div class="panel-grid-2">
								<div class="sub-box">
									<div class="sub-head-flex">
										<h3>TOURNAMENT CONFIGURATION</h3>
										<button
											type="button"
											class="toggle-pill {selectedPlugin.state.kiosk_overlay_enabled
												? 'on'
												: 'off'}"
											onclick={() =>
												runPluginAction('tournament-engine', 'toggle_kiosk_overlay', {
													enabled: !selectedPlugin.state.kiosk_overlay_enabled
												})}
										>
											KIOSK OVERLAY: {selectedPlugin.state.kiosk_overlay_enabled
												? 'SHOWN'
												: 'HIDDEN'}
										</button>
									</div>

									<label class="field">
										<span>Event Title</span>
										<input type="text" bind:value={tourneyTitle} />
									</label>
									<div class="inline-controls">
										<input type="text" placeholder="Game Name" bind:value={tourneyGame} />
										<input
											type="text"
											placeholder="ROM Filter (e.g. sfiii3)"
											bind:value={tourneyRom}
										/>
									</div>
									<label class="field" style="margin-top:0.5rem;">
										<span>Prize Pool Banner</span>
										<input type="text" bind:value={tourneyPrize} />
									</label>
									<button
										type="button"
										class="action-btn primary full"
										onclick={() =>
											runPluginAction('tournament-engine', 'configure_tournament', {
												title: tourneyTitle,
												game_name: tourneyGame,
												target_rom: tourneyRom,
												prize_pool: tourneyPrize,
												status: 'live'
											})}
									>
										SAVE TOURNAMENT SETTINGS
									</button>

									<h3 style="margin-top: 1.25rem;">ON-SITE VENUE TV SIGNAGE STREAMS (/TV)</h3>
									{#each selectedPlugin.state.tv_displays ?? [] as tv}
										<div class="list-item-row">
											<div>
												<strong>{tv.name}</strong>
												<small>{tv.output_mode} · {tv.slide_interval_secs}s loop</small>
											</div>
											<a
												href={tv.stream_url}
												target="_blank"
												class="action-btn primary small"
												style="text-decoration: none;"
											>
												📺 OPEN {tv.zone.toUpperCase()} TV
											</a>
										</div>
									{/each}
									<button
										type="button"
										class="action-btn green full"
										style="margin-top: 0.65rem;"
										onclick={() =>
											runPluginAction('tournament-engine', 'trigger_tv_fanfare', {
												headline: 'NEW HIGH SCORE RECORD ON ASTRO-01!',
												subtext: 'DAIGO (DGO) — 999,900 PTS ON STREET FIGHTER III',
												node_id: 'ASTRO-01'
											})}
									>
										🚨 BROADCAST HIGH-SCORE FANFARE TO ALL VENUE TVs
									</button>
								</div>

								<div class="sub-box">
									<h3>HEAD-TO-HEAD BRACKET MATCHES</h3>
									{#each selectedPlugin.state.bracket_matches ?? [] as m}
										<div class="bracket-card">
											<div class="bracket-meta">
												<span>ROUND {m.round} · {m.cabinet_node_id}</span>
												<span class="status-pill">{m.status}</span>
											</div>
											<div class="bracket-players">
												<button
													type="button"
													class="player-pick {m.winner === m.player_a ? 'winner' : ''}"
													onclick={() =>
														runPluginAction('tournament-engine', 'report_match_winner', {
															match_id: m.id,
															winner: m.player_a
														})}
												>
													{m.player_a} {m.winner === m.player_a ? '👑' : ''}
												</button>
												<span class="vs">VS</span>
												<button
													type="button"
													class="player-pick {m.winner === m.player_b ? 'winner' : ''}"
													onclick={() =>
														runPluginAction('tournament-engine', 'report_match_winner', {
															match_id: m.id,
															winner: m.player_b
														})}
												>
													{m.player_b} {m.winner === m.player_b ? '👑' : ''}
												</button>
											</div>
										</div>
									{/each}

									<h3 style="margin-top: 1.25rem;">CONTENDER REGISTRATION & BRACKET</h3>
									<div class="inline-controls">
										<input
											type="text"
											placeholder="Add Player (e.g. TOKIDO (TKD))"
											bind:value={newPlayerName}
										/>
										<button
											type="button"
											class="action-btn green"
											onclick={() => {
												if (!newPlayerName.trim()) return;
												runPluginAction('tournament-engine', 'register_player', {
													player: newPlayerName.trim()
												});
												newPlayerName = '';
											}}
										>
											+ ENROLL
										</button>
										<button
											type="button"
											class="action-btn primary"
											onclick={() =>
												runPluginAction('tournament-engine', 'generate_bracket', {})}
										>
											SEED BRACKET
										</button>
									</div>

									<h3 style="margin-top: 1.25rem;">LIVE SCORE-ATTACK STANDINGS</h3>
									{#each selectedPlugin.state.leaderboard ?? [] as row, idx}
										<div class="list-item-row">
											<span>#{idx + 1} <strong>{row.player_name} ({row.initials})</strong></span>
											<strong>{Number(row.score).toLocaleString()} PTS ({row.node_id})</strong>
										</div>
									{/each}
								</div>
							</div>

						<!-- PANEL 4: RFID / NFC WRISTBAND & CARD PERIPHERALS -->
						{:else if selectedPlugin.manifest.id === 'rfid-peripherals'}
							<div class="panel-grid-2">
								<div class="sub-box">
									<h3>REGISTERED NFC WRISTBANDS & VIP CARDS</h3>
									<div class=" inline-controls" style="margin-bottom: 0.75rem;">
										<span>Tap Target Cabinet:</span>
										<select bind:value={rfidTapNode}>
											{#each overview.nodes as n}
												<option value={n.telemetry.node_id}>{n.telemetry.node_id}</option>
											{/each}
										</select>
									</div>

									{#each selectedPlugin.state.cards ?? [] as card}
										<div class="list-item-row">
											<div>
												<strong>{card.holder_name} [{card.initials}]</strong>
												<small>
													UID: <code>{card.uid}</code> · Taps: {card.total_taps}
												</small>
											</div>
											<div class="inline-controls">
												<button
													type="button"
													class="toggle-pill {card.vip_freeplay ? 'on' : 'off'}"
													onclick={() =>
														runPluginAction('rfid-peripherals', 'toggle_vip', {
															uid: card.uid
														})}
												>
													{card.vip_freeplay ? 'VIP FREE' : `${card.credits_balance} CR`}
												</button>
												<button
													type="button"
													class="action-btn small"
													onclick={() =>
														runPluginAction('rfid-peripherals', 'topup_card', {
															uid: card.uid,
															credits: 5
														})}
												>
													+5 CR
												</button>
												<button
													type="button"
													class="action-btn green small"
													onclick={() =>
														runPluginAction('rfid-peripherals', 'tap_card', {
															uid: card.uid,
															node_id: rfidTapNode
														})}
												>
													TAP → {rfidTapNode}
												</button>
											</div>
										</div>
									{/each}
								</div>

								<div class="sub-box">
									<h3>ENROLL NEW NFC / RFID WRISTBAND</h3>
									<label class="field">
										<span>Card / Wristband UID (Hex)</span>
										<input type="text" bind:value={newCardUid} />
									</label>
									<div class="inline-controls">
										<input
											type="text"
											placeholder="Holder Name"
											bind:value={newCardHolder}
										/>
										<input
											type="text"
											maxlength="3"
											placeholder="3-Char Initials"
											bind:value={newCardInitials}
											style="width: 90px;"
										/>
										<input
											type="number"
											min="0"
											max="500"
											bind:value={newCardCredits}
											style="width: 90px;"
										/>
									</div>
									<label class="checkbox-field" style="margin: 0.6rem 0;">
										<input type="checkbox" bind:checked={newCardVip} />
										<span>Grant Unlimited VIP Free-Play Status</span>
									</label>
									<button
										type="button"
										class="action-btn primary full"
										onclick={() =>
											runPluginAction('rfid-peripherals', 'register_card', {
												uid: newCardUid,
												holder_name: newCardHolder,
												initials: newCardInitials,
												credits_balance: Number(newCardCredits),
												vip_freeplay: newCardVip
											})}
									>
										+ REGISTER WRISTBAND UID
									</button>
								</div>
							</div>

						<!-- PANEL 5: ALERTING & OPS SINKS -->
						{:else if selectedPlugin.manifest.id === 'alerting-sinks'}
							<div class="panel-grid-2">
								<div class="sub-box">
									<h3>THERMAL WATCHDOG & WEBHOOK SINKS</h3>
									<div class="inline-controls" style="margin-bottom: 1rem;">
										<span>CPU/GPU Thermal Alert Threshold (°C):</span>
										<input
											type="number"
											min="50"
											max="100"
											bind:value={thermalThresholdInput}
											style="width: 80px;"
										/>
										<button
											type="button"
											class="action-btn primary"
											onclick={() =>
												runPluginAction('alerting-sinks', 'set_thermal_threshold', {
													thermal_warn_temp_c: Number(thermalThresholdInput)
												})}
										>
											SET °C
										</button>
										<button
											type="button"
											class="action-btn green"
											onclick={() =>
												runPluginAction('alerting-sinks', 'send_test_alert', {
													message: 'Operator dispatched synthetic test ping from /admin'
												})}
										>
											🔔 TEST PING
										</button>
									</div>

									{#each selectedPlugin.state.sinks ?? [] as sink}
										<div class="list-item-row">
											<div>
												<strong>{sink.name} ({sink.kind.toUpperCase()})</strong>
												<small>{sink.webhook_url}</small>
											</div>
											<button
												type="button"
												class="toggle-pill {sink.enabled ? 'on' : 'off'}"
												onclick={() =>
													runPluginAction('alerting-sinks', 'toggle_sink', {
														sink_id: sink.id
													})}
											>
												{sink.enabled ? 'ACTIVE' : 'MUTED'}
											</button>
										</div>
									{/each}

									<h3 style="margin-top: 1rem;">ADD NOTIFICATION SINK</h3>
									<div class="inline-controls">
										<select bind:value={newSinkKind}>
											<option value="discord">Discord</option>
											<option value="slack">Slack</option>
											<option value="pagerduty">PagerDuty</option>
											<option value="telegram">Telegram</option>
										</select>
										<input type="text" placeholder="Channel Label" bind:value={newSinkName} />
										<input type="text" placeholder="Webhook URL" bind:value={newSinkUrl} />
										<button
											type="button"
											class="action-btn primary"
											onclick={() =>
												runPluginAction('alerting-sinks', 'add_sink', {
													name: newSinkName,
													kind: newSinkKind,
													webhook_url: newSinkUrl
												})}
										>
											+ ADD
										</button>
									</div>
								</div>

								<div class="sub-box">
									<h3>ALERT DELIVERY AUDIT LOG</h3>
									<div class="scroll-list">
										{#each selectedPlugin.state.deliveries ?? [] as d}
											<div class="list-item-row">
												<div>
													<strong>[{d.severity.toUpperCase()}] {d.title}</strong>
													<small>{d.sink_name} · {d.message}</small>
												</div>
												<span class="status-pill online">{d.status}</span>
											</div>
										{/each}
									</div>
								</div>
							</div>

						<!-- CUSTOM WEBHOOK PLUGIN PANEL -->
						{:else}
							<div class="sub-box">
								<h3>EXTERNAL WEBHOOK SUBSCRIBER STATUS</h3>
								<p>Target Endpoint: <code>{selectedPlugin.state.webhook_url}</code></p>
								<p>Events Dispatched: <strong>{selectedPlugin.state.dispatched_count ?? 0}</strong></p>
								<p>Last Status: <strong>{selectedPlugin.state.last_status ?? 'ready'}</strong></p>
							</div>
						{/if}
					</section>
				{/if}
			</div>

		<!-- =================================================================== -->
		<!-- TAB 3: NETPLAY RELAY & OTA SQUASHFS SYNC                            -->
		<!-- =================================================================== -->
		{:else if activeTab === 'netplay_ota'}
			<div class="panel-grid-2">
				<div class="sub-box">
					<h3>A.S.T.R.O. ROLLBACK NETPLAY UDP RELAY (0.0.0.0:9750)</h3>
					{#each netplaySessions as sess}
						<div class="list-item-row">
							<div>
								<strong>Session 0x{sess.session_id.toString(16).toUpperCase()}</strong>
								<small>
									Relayed Datagrams: {sess.relayed_packets} · Desync Escalations: {sess.desync_events}
								</small>
							</div>
							<div class="inline-controls">
								{#each sess.endpoints as ep}
									<span class="event-pill">
										{ep.node_id} ({ep.state}, {ep.loss_pct.toFixed(1)}% loss)
									</span>
								{/each}
							</div>
						</div>
					{/each}
				</div>

				<div class="sub-box">
					<h3>STAGED SQUASHFS OTA PAYLOADS (/data/beercade/sync)</h3>
					{#each otaPayloads as pkg}
						<div class="list-item-row">
							<div>
								<strong>{pkg.filename}</strong>
								<small>
									Size: {(pkg.size_bytes / (1024 * 1024)).toFixed(1)} MiB · SHA-256:
									<code>{pkg.sha256?.slice(0, 16) ?? 'verified'}...</code>
								</small>
							</div>
							<span class="status-pill online">STAGED</span>
						</div>
					{/each}
				</div>
			</div>

		<!-- =================================================================== -->
		<!-- TAB 4: MULTIPLEXED SSE EVENT BUS & AUDIT LEDGER                     -->
		<!-- =================================================================== -->
		{:else if activeTab === 'audit'}
			<div class="panel-grid-2">
				<div class="sub-box">
					<h3>RECENT TOKEN & CREDIT TRANSACTIONS</h3>
					{#each overview.transactions as tx}
						<div class="list-item-row">
							<div>
								<strong>{tx.node_id} (+{tx.credits} CR · {tx.kind})</strong>
								<small>Command: {tx.command_id ?? 'direct'}</small>
							</div>
							<span>Balance: {tx.balance_after} CR</span>
						</div>
					{/each}
				</div>

				<div class="sub-box">
					<h3>LIVE SSE BUS & CABINET ERROR EVENTS</h3>
					{#each overview.errors as err}
						<div class="list-item-row">
							<div>
								<strong class="warn-text">[{err.level.toUpperCase()}] {err.node_id}</strong>
								<small>{err.source}: {err.message}</small>
							</div>
						</div>
					{/each}
					{#each liveSseEvents as ev}
						<div class="list-item-row">
							<div>
								<strong>[{ev.channel}] {ev.at}</strong>
								<small>{ev.summary}</small>
							</div>
						</div>
					{/each}
				</div>
			</div>
		{/if}
	{/if}
</div>

<!-- Custom Webhook Plugin Registration Modal -->
{#if webhookModalOpen}
	<div class="modal-backdrop">
		<form class="webhook-modal" onsubmit={handleRegisterWebhook}>
			<h3>REGISTER CUSTOM WEBHOOK PLUGIN</h3>
			<label class="field">
				<span>Plugin ID (slug)</span>
				<input type="text" bind:value={whId} required />
			</label>
			<label class="field">
				<span>Display Name</span>
				<input type="text" bind:value={whName} required />
			</label>
			<label class="field">
				<span>Target Webhook URL (receives POST on B.O.S.S. events)</span>
				<input type="url" bind:value={whUrl} required />
			</label>
			<label class="field">
				<span>Description</span>
				<input type="text" bind:value={whDesc} />
			</label>
			<div class="inline-controls" style="justify-content: flex-end; margin-top: 1rem;">
				<button type="button" class="action-btn" onclick={() => (webhookModalOpen = false)}>
					CANCEL
				</button>
				<button type="submit" class="action-btn primary">REGISTER PLUGIN</button>
			</div>
		</form>
	</div>
{/if}

<PinKeypadModal
	open={unlockModalOpen}
	onClose={() => (unlockModalOpen = false)}
	onSuccess={() => {
		unlockModalOpen = false;
		authenticated = true;
		refreshAll();
	}}
/>

<style>
	/* Empty Fleet Alert Banner */
	.empty-fleet-alert {
		display: flex;
		align-items: center;
		gap: 1.25rem;
		background: rgba(var(--warn-rgb), 0.12);
		border: 1px solid var(--warn);
		border-radius: 12px;
		padding: 1.1rem 1.5rem;
		margin-bottom: 1.25rem;
	}

	.empty-fleet-alert .alert-icon {
		font-size: 2rem;
	}

	.empty-fleet-alert h3 {
		margin: 0 0 0.35rem 0;
		color: var(--warn);
		font-size: 1rem;
		font-weight: 800;
		letter-spacing: 0.05em;
	}

	.empty-fleet-alert p {
		margin: 0;
		font-size: 0.85rem;
		color: var(--text-soft);
		line-height: 1.4;
	}

	/* Discovery & Rescue Card */
	.discovery-card {
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		border-radius: 14px;
		padding: 1.25rem;
		margin-bottom: 1.5rem;
	}

	.discovery-head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.75rem;
		margin-bottom: 1.15rem;
		border-bottom: 1px solid var(--border);
		padding-bottom: 0.85rem;
	}

	.dh-title {
		display: flex;
		align-items: center;
		gap: 0.85rem;
	}

	.dh-icon {
		font-size: 1.75rem;
	}

	.dh-title h3 {
		margin: 0;
		font-size: 0.95rem;
		font-weight: 800;
		letter-spacing: 0.05em;
		color: var(--text-strong);
	}

	.dh-title p {
		margin: 0.2rem 0 0;
		font-size: 0.75rem;
		color: var(--text-muted);
	}

	.iface-badges {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.iface-badge {
		font-size: 0.72rem;
		font-family: monospace;
		padding: 0.25rem 0.6rem;
		border-radius: 6px;
		background: var(--surface-3);
		border: 1px solid var(--border);
		color: var(--text-soft);
	}

	.iface-badge.default {
		border-color: var(--accent);
		color: var(--accent);
		background: rgba(var(--accent-rgb), 0.12);
		font-weight: 700;
	}

	.discovery-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
		gap: 1.15rem;
	}

	.disc-box {
		background: var(--inset);
		border: 1px solid var(--border);
		border-radius: 10px;
		padding: 1rem;
		display: flex;
		flex-direction: column;
		justify-content: space-between;
	}

	.disc-box-title {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 0.4rem;
	}

	.disc-box-title h4 {
		margin: 0;
		font-size: 0.82rem;
		font-weight: 800;
		letter-spacing: 0.04em;
		color: var(--text);
	}

	.tag-auto {
		background: rgba(var(--accent-rgb), 0.18);
		color: var(--accent);
		font-size: 0.65rem;
		font-weight: 700;
		padding: 0.15rem 0.45rem;
		border-radius: 4px;
	}

	.tag-manual {
		background: rgba(var(--good-rgb), 0.18);
		color: var(--good);
		font-size: 0.65rem;
		font-weight: 700;
		padding: 0.15rem 0.45rem;
		border-radius: 4px;
	}

	.disc-box-desc {
		margin: 0 0 0.85rem;
		font-size: 0.73rem;
		color: var(--text-muted);
		line-height: 1.35;
	}

	.disc-input-row {
		display: flex;
		gap: 0.5rem;
		margin-bottom: 0.5rem;
	}

	.disc-text-input {
		flex: 1;
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		color: var(--text);
		border-radius: 8px;
		padding: 0.5rem 0.75rem;
		font-family: inherit;
		font-size: 0.78rem;
	}

	.disc-text-input:focus {
		outline: none;
		border-color: var(--accent);
	}

	.disc-sub-field {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.7rem;
		color: var(--text-faint);
		margin-top: 0.35rem;
	}

	.disc-small-input {
		flex: 1;
		background: var(--surface-2);
		border: 1px solid var(--border);
		color: var(--text-soft);
		border-radius: 6px;
		padding: 0.25rem 0.5rem;
		font-size: 0.7rem;
		font-family: monospace;
	}

	.action-btn.cyan {
		border-color: #00d4ff;
		background: rgba(0, 212, 255, 0.16);
		color: #00d4ff;
	}

	.action-btn.busy {
		opacity: 0.7;
		cursor: wait;
	}

	.discovery-results {
		margin-top: 1rem;
		padding-top: 1rem;
		border-top: 1px dashed var(--border);
	}

	.d-results-banner {
		font-size: 0.8rem;
		color: var(--text-soft);
		margin-bottom: 0.75rem;
	}

	.d-results-banner small {
		color: var(--text-faint);
		margin-left: 0.5rem;
	}

	.discovered-nodes-row {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
		gap: 0.75rem;
	}

	.disc-node-card {
		background: var(--surface-3);
		border: 1px solid var(--border);
		border-radius: 8px;
		padding: 0.75rem;
		display: flex;
		justify-content: space-between;
		align-items: center;
	}

	.disc-node-card.linked {
		border-color: rgba(var(--good-rgb), 0.5);
	}

	.dnc-meta {
		display: flex;
		flex-direction: column;
		gap: 0.15rem;
	}

	.dnc-id {
		font-size: 0.85rem;
		color: var(--text-strong);
	}

	.dnc-ip {
		font-family: monospace;
		font-size: 0.75rem;
		color: var(--text-muted);
	}

	.dnc-hw {
		font-size: 0.7rem;
		color: var(--text-faint);
	}

	.linked-tag {
		color: var(--good);
		font-size: 0.75rem;
		font-weight: 700;
	}

	.admin-shell {
		min-height: 100vh;
		padding: 1.25rem 1.75rem;
		box-sizing: border-box;
		max-width: 1680px;
		margin: 0 auto;
	}

	.admin-topbar {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		padding-bottom: 1rem;
		border-bottom: 1px solid var(--border);
		margin-bottom: 1.25rem;
		flex-wrap: wrap;
	}

	.brand-left {
		display: flex;
		align-items: center;
		gap: 1rem;
	}

	.boss-badge {
		background: var(--accent);
		color: var(--on-accent);
		font-weight: 800;
		padding: 0.35rem 0.7rem;
		border-radius: 6px;
		font-size: 0.8rem;
		letter-spacing: 0.08em;
	}

	.brand-left h1 {
		margin: 0;
		font-size: 1.35rem;
		color: var(--text-strong);
	}

	.brand-left p {
		margin: 0.15rem 0 0 0;
		font-size: 0.78rem;
		color: var(--text-muted);
	}

	.topbar-controls {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		flex-wrap: wrap;
	}

	.stream-indicator {
		font-size: 0.75rem;
		font-weight: 700;
		padding: 0.35rem 0.7rem;
		border-radius: 999px;
	}

	.stream-indicator.live {
		background: rgba(var(--good-rgb), 0.14);
		color: var(--good);
		border: 1px solid rgba(var(--good-rgb), 0.4);
	}

	.stream-indicator.sim {
		background: rgba(var(--warn-rgb), 0.14);
		color: var(--warn);
		border: 1px solid rgba(var(--warn-rgb), 0.4);
	}

	.theme-picker {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.72rem;
		font-weight: 700;
		color: var(--text-muted);
	}

	.theme-picker select {
		min-height: 2.25rem;
		padding: 0 0.6rem;
		border-radius: 8px;
		border: 1px solid var(--accent);
		background: var(--surface-2);
		color: var(--accent);
		font-family: inherit;
		font-weight: 700;
	}

	.idle-timer {
		font-size: 0.78rem;
		color: var(--text-soft);
		background: var(--surface-2);
		padding: 0.4rem 0.75rem;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
	}

	.nav-btn {
		text-decoration: none;
		color: var(--accent);
		border: 1px solid var(--accent);
		padding: 0.45rem 0.85rem;
		border-radius: 8px;
		font-size: 0.78rem;
		font-weight: 700;
	}

	.lock-btn {
		background: rgba(var(--danger-rgb), 0.16);
		border: 1px solid var(--danger);
		color: var(--danger-soft);
		padding: 0.45rem 0.85rem;
		border-radius: 8px;
		font-family: inherit;
		font-size: 0.78rem;
		font-weight: 700;
		cursor: pointer;
	}

	.toast-banner {
		background: rgba(var(--accent-rgb), 0.16);
		border: 1px solid var(--accent);
		color: var(--text-strong);
		padding: 0.65rem 1rem;
		border-radius: 10px;
		margin-bottom: 1rem;
		font-size: 0.85rem;
	}

	.kpi-strip {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: 0.9rem;
		margin-bottom: 1.25rem;
	}

	.kpi-card {
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 12px;
		padding: 0.9rem 1.1rem;
		display: flex;
		flex-direction: column;
	}

	.kpi-label {
		font-size: 0.7rem;
		color: var(--text-muted);
		letter-spacing: 0.1em;
	}

	.kpi-val {
		font-size: 1.25rem;
		margin: 0.25rem 0;
		color: var(--text-strong);
	}

	.kpi-val.cyan {
		color: var(--accent);
	}

	.kpi-val.green {
		color: var(--good);
	}

	.kpi-val.amber {
		color: var(--warn);
	}

	.kpi-card small {
		font-size: 0.72rem;
		color: var(--text-faint);
	}

	.console-tabs {
		display: flex;
		gap: 0.6rem;
		margin-bottom: 1.25rem;
		flex-wrap: wrap;
	}

	.tab-btn {
		padding: 0.7rem 1.15rem;
		border-radius: 10px;
		border: 1px solid var(--border);
		background: var(--surface-2);
		color: var(--text-muted);
		font-family: inherit;
		font-weight: 700;
		font-size: 0.82rem;
		cursor: pointer;
	}

	.tab-btn.active {
		border-color: var(--accent);
		background: rgba(var(--accent-rgb), 0.15);
		color: var(--accent);
	}

	.fleet-layout {
		display: grid;
		grid-template-columns: 1fr 370px;
		gap: 1.25rem;
	}

	.fleet-cards {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(420px, 1fr));
		gap: 1rem;
	}

	.node-card {
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 14px;
		padding: 1.15rem;
	}

	.node-head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 0.9rem;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.node-status-dot {
		display: inline-block;
		width: 10px;
		height: 10px;
		border-radius: 50%;
		margin-right: 0.4rem;
	}

	.node-status-dot.on {
		background: var(--good);
		box-shadow: 0 0 8px var(--good);
	}

	.node-title {
		font-size: 1.1rem;
		color: var(--text-strong);
		margin-right: 0.5rem;
	}

	.node-hw {
		font-size: 0.74rem;
		color: var(--text-muted);
	}

	.credits-badge {
		background: rgba(var(--good-rgb), 0.14);
		color: var(--good);
		padding: 0.25rem 0.6rem;
		border-radius: 6px;
		font-size: 0.74rem;
		font-weight: 700;
	}

	.telemetry-metrics {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.65rem;
		margin-bottom: 1rem;
	}

	.metric-box {
		background: var(--inset);
		border: 1px solid var(--border);
		border-radius: 8px;
		padding: 0.55rem 0.75rem;
		display: flex;
		flex-direction: column;
	}

	.metric-box span {
		font-size: 0.68rem;
		color: var(--text-faint);
	}

	.metric-box strong {
		font-size: 0.82rem;
		color: var(--text);
		margin-top: 0.15rem;
	}

	.warn-text {
		color: var(--danger) !important;
	}

	.rpc-toolbar {
		display: flex;
		flex-wrap: wrap;
		gap: 0.45rem;
	}

	.action-btn {
		padding: 0.5rem 0.8rem;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
		background: var(--surface-3);
		color: var(--text);
		font-family: inherit;
		font-weight: 700;
		font-size: 0.75rem;
		cursor: pointer;
	}

	.action-btn.primary {
		border-color: var(--accent);
		background: rgba(var(--accent-rgb), 0.16);
		color: var(--accent);
	}

	.action-btn.green {
		border-color: var(--good);
		background: rgba(var(--good-rgb), 0.16);
		color: var(--good);
	}

	.action-btn.danger {
		border-color: var(--danger);
		background: rgba(var(--danger-rgb), 0.16);
		color: var(--danger-soft);
	}

	.action-btn.small {
		padding: 0.3rem 0.55rem;
		font-size: 0.7rem;
	}

	.action-btn.full {
		width: 100%;
		margin-top: 0.6rem;
	}

	.side-panel,
	.sub-box {
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 14px;
		padding: 1.15rem;
		margin-bottom: 1rem;
	}

	.side-panel h3,
	.sub-box h3 {
		margin: 0 0 0.85rem 0;
		font-size: 0.88rem;
		color: var(--accent);
		letter-spacing: 0.06em;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		margin-bottom: 0.7rem;
		font-size: 0.76rem;
		color: var(--text-muted);
	}

	input[type='text'],
	input[type='number'],
	input[type='url'],
	select {
		padding: 0.55rem 0.7rem;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
		background: var(--inset);
		color: var(--text-strong);
		font-family: inherit;
		font-size: 0.82rem;
	}

	.checkbox-field {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.78rem;
		color: var(--text-soft);
	}

	.cmd-row,
	.list-item-row {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 0.6rem 0.75rem;
		background: var(--inset);
		border: 1px solid var(--border);
		border-radius: 8px;
		margin-bottom: 0.45rem;
		font-size: 0.8rem;
		gap: 0.75rem;
	}

	.list-item-row div {
		display: flex;
		flex-direction: column;
	}

	.list-item-row small {
		color: var(--text-faint);
		font-size: 0.72rem;
	}

	.plugin-hub-layout {
		display: grid;
		grid-template-columns: 300px 1fr;
		gap: 1.25rem;
	}

	.plugin-sidebar {
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 14px;
		padding: 1rem;
	}

	.plugin-sidebar-head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 0.85rem;
	}

	.plugin-sidebar-head h3 {
		margin: 0;
		font-size: 0.85rem;
		color: var(--accent);
	}

	.plugin-nav-item {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.4rem;
		background: var(--inset);
		border: 1px solid var(--border);
		border-radius: 10px;
		padding: 0.55rem 0.65rem;
		margin-bottom: 0.55rem;
	}

	.plugin-nav-item.active {
		border-color: var(--accent);
		background: rgba(var(--accent-rgb), 0.1);
	}

	.plugin-select-btn {
		background: transparent;
		border: none;
		color: inherit;
		font-family: inherit;
		text-align: left;
		cursor: pointer;
		flex: 1;
	}

	.plugin-nav-title {
		display: flex;
		justify-content: space-between;
		font-size: 0.82rem;
	}

	.plugin-cat-tag {
		font-size: 0.68rem;
		color: var(--text-faint);
	}

	.toggle-pill {
		padding: 0.25rem 0.55rem;
		border-radius: 6px;
		font-family: inherit;
		font-size: 0.7rem;
		font-weight: 800;
		cursor: pointer;
		border: 1px solid transparent;
	}

	.toggle-pill.on {
		background: rgba(var(--good-rgb), 0.18);
		border-color: var(--good);
		color: var(--good);
	}

	.toggle-pill.off {
		background: rgba(var(--muted-rgb), 0.12);
		border-color: var(--text-faint);
		color: var(--text-muted);
	}

	.plugin-pane-header {
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 14px;
		padding: 1.15rem;
		margin-bottom: 1rem;
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		gap: 1rem;
		flex-wrap: wrap;
	}

	.plugin-id-badge {
		font-size: 0.7rem;
		color: var(--accent);
		background: rgba(var(--accent-rgb), 0.12);
		padding: 0.18rem 0.5rem;
		border-radius: 4px;
	}

	.plugin-pane-header h2 {
		margin: 0.3rem 0;
		font-size: 1.25rem;
	}

	.plugin-pane-header p {
		margin: 0;
		font-size: 0.82rem;
		color: var(--text-muted);
	}

	.plugin-meta-pills {
		display: flex;
		gap: 0.4rem;
		flex-wrap: wrap;
	}

	.event-pill {
		background: var(--surface-3);
		border: 1px solid var(--border-strong);
		padding: 0.25rem 0.55rem;
		border-radius: 6px;
		font-size: 0.72rem;
		color: var(--text-soft);
	}

	.panel-grid-2 {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 1rem;
	}

	.inline-controls {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.mini-kpis {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 0.6rem;
		margin-bottom: 0.9rem;
	}

	.mini-kpis div {
		background: var(--inset);
		border: 1px solid var(--border);
		border-radius: 8px;
		padding: 0.6rem;
		display: flex;
		flex-direction: column;
	}

	.mini-kpis span {
		font-size: 0.66rem;
		color: var(--text-faint);
	}

	.scene-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.55rem;
	}

	.scene-btn {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 0.7rem;
		background: var(--inset);
		border: 1px solid var(--border);
		border-radius: 10px;
		color: var(--text-strong);
		font-family: inherit;
		font-size: 0.78rem;
		cursor: pointer;
		text-align: left;
	}

	.scene-btn.active {
		border-color: var(--scene-color);
		box-shadow: 0 0 12px rgba(var(--accent-rgb), 0.2);
	}

	.color-swatch {
		width: 14px;
		height: 14px;
		border-radius: 50%;
		flex-shrink: 0;
	}

	.bracket-card {
		background: var(--inset);
		border: 1px solid var(--border);
		border-radius: 10px;
		padding: 0.75rem;
		margin-bottom: 0.6rem;
	}

	.bracket-meta {
		display: flex;
		justify-content: space-between;
		font-size: 0.7rem;
		color: var(--text-muted);
		margin-bottom: 0.5rem;
	}

	.bracket-players {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.player-pick {
		flex: 1;
		padding: 0.5rem;
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		border-radius: 6px;
		color: var(--text);
		font-family: inherit;
		font-weight: 700;
		font-size: 0.78rem;
		cursor: pointer;
	}

	.player-pick.winner {
		border-color: var(--warn);
		color: var(--warn);
	}

	.status-pill {
		font-size: 0.68rem;
		padding: 0.15rem 0.45rem;
		border-radius: 4px;
		background: rgba(var(--good-rgb), 0.15);
		color: var(--good);
	}

	.sub-head-flex {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 0.75rem;
	}

	.locked-placeholder {
		text-align: center;
		padding: 4rem 1.5rem;
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 16px;
	}

	.modal-backdrop {
		position: fixed;
		inset: 0;
		background: rgba(var(--scrim-rgb), 0.85);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 999;
	}

	.webhook-modal {
		background: var(--surface-2);
		border: 2px solid var(--accent);
		border-radius: 14px;
		padding: 1.5rem;
		width: 100%;
		max-width: 440px;
	}

	@media (max-width: 1100px) {
		.fleet-layout,
		.plugin-hub-layout,
		.panel-grid-2 {
			grid-template-columns: 1fr;
		}
	}
</style>
