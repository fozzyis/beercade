<script lang="ts">
	import { onMount } from 'svelte';
	import { playClickSound } from '$lib/audio';
	import machinesHero from '$lib/assets/machines_hero.jpg';

	let telemetry = $state<{
		timestamp: string;
		venue_status: string;
		machines_total: number;
		machines_active: number;
		astro_nodes: Array<{
			node_id: string;
			hardware: string;
			current_core: string;
			current_game: string;
			status: string;
			session_duration_sec: number;
			cpu_temp_c?: number;
			fps?: number;
			credits_available?: number;
		}>;
		featured_highscores: Array<{ game: string; score: number; initials: string }>;
	} | null>(null);

	let selectedCategory = $state('ALL');
	let searchQuery = $state('');

	const machines = [
		{ id: 'M-01', name: 'Godzilla Premium', type: 'Pinball', status: 'IN_USE', year: 2021 },
		{ id: 'M-02', name: 'Attack from Mars', type: 'Pinball', status: 'IN_USE', year: 1995 },
		{ id: 'M-03', name: 'Medieval Madness', type: 'Pinball', status: 'AVAILABLE', year: 1997 },
		{ id: 'M-04', name: 'The Addams Family', type: 'Pinball', status: 'IN_USE', year: 1992 },
		{
			id: 'M-05',
			name: 'Street Fighter II: Champion Edition',
			type: 'ASTRO Arcade',
			status: 'IN_USE',
			year: 1992
		},
		{ id: 'M-06', name: 'Tekken 2', type: 'ASTRO Arcade', status: 'AVAILABLE', year: 1996 },
		{ id: 'M-07', name: 'Metal Slug 3', type: 'ASTRO Arcade', status: 'IN_USE', year: 2000 },
		{
			id: 'M-08',
			name: 'Marvel vs. Capcom 2',
			type: 'ASTRO Arcade',
			status: 'IN_USE',
			year: 2000
		},
		{ id: 'M-09', name: 'Daytona USA Twin', type: 'Racing', status: 'IN_USE', year: 1994 },
		{
			id: 'M-10',
			name: 'Sega Rally Championship',
			type: 'Racing',
			status: 'AVAILABLE',
			year: 1994
		},
		{ id: 'M-11', name: 'Time Crisis 2', type: 'Lightgun', status: 'IN_USE', year: 1997 },
		{ id: 'M-12', name: 'House of the Dead 2', type: 'Lightgun', status: 'AVAILABLE', year: 1998 }
	];

	let filteredMachines = $derived(
		machines.filter((m) => {
			const catOk = selectedCategory === 'ALL' || m.type === selectedCategory;
			const q = searchQuery.trim().toLowerCase();
			const qOk =
				!q ||
				m.name.toLowerCase().includes(q) ||
				m.type.toLowerCase().includes(q) ||
				m.id.toLowerCase().includes(q) ||
				String(m.year).includes(q);
			return catOk && qOk;
		})
	);

	onMount(() => {
		fetch('/api/telemetry/machines')
			.then((res) => res.json())
			.then((data) => {
				telemetry = data as any;
			})
			.catch(console.error);

		const eventSource = new EventSource('/api/telemetry/stream');
		eventSource.onmessage = (event) => {
			try {
				telemetry = JSON.parse(event.data);
			} catch (err) {
				console.error('Failed to parse telemetry event:', err);
			}
		};

		return () => eventSource.close();
	});

	function selectCategory(cat: string) {
		playClickSound();
		selectedCategory = cat;
	}
</script>

<svelte:head>
	<title>MACHINE ROSTER & LIVE TELEMETRY | THE BK</title>
</svelte:head>

<div class="mx-auto max-w-6xl px-4 py-12">
	<!-- Hero Image Banner -->
	<div class="relative mb-12 h-52 overflow-hidden rounded-sm md:h-80">
		<img
			src={machinesHero}
			alt="Beercade Machine Roster"
			class="vhs-video h-full w-full object-cover object-center"
		/>
		<div
			class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/30 to-transparent"
		></div>
		<div class="absolute inset-0 flex flex-col justify-end p-8">
			<h1
				class="glitch-text glow-neon-green font-mono text-4xl font-bold tracking-widest text-neon-green uppercase md:text-6xl"
				data-text="MACHINE ROSTER"
			>
				MACHINE ROSTER
			</h1>
			<p class="mt-2 font-sans text-lg text-crema/80">
				Real-time hardware status from our on-premise B.O.S.S. tunnel &amp; custom A.S.T.R.O. nodes.
			</p>
		</div>
	</div>

	<!-- Live Telemetry Status Banner -->
	<div
		class="mb-12 rounded-sm border-2 border-neon-green/60 bg-black p-6 shadow-[0_0_20px_rgba(57,255,20,0.2)]"
	>
		<div class="flex flex-wrap items-center justify-between gap-4">
			<div class="flex items-center gap-3">
				<span class="relative flex h-4 w-4">
					<span
						class="absolute inline-flex h-full w-full animate-ping rounded-full bg-neon-green opacity-75"
					></span>
					<span class="relative inline-flex h-4 w-4 rounded-full bg-neon-green"></span>
				</span>
				<div>
					<div class="font-mono text-xs text-gray-400">B.O.S.S. TUNNEL STATUS</div>
					<div class="font-mono text-lg font-bold text-neon-green">ONLINE • LIVE STREAM ACTIVE</div>
				</div>
			</div>

			{#if telemetry}
				<div class="flex gap-8 font-mono">
					<div>
						<div class="text-xs text-gray-400">MACHINES ACTIVE</div>
						<div class="text-2xl font-bold text-white">
							{telemetry.machines_active} / {telemetry.machines_total}
						</div>
					</div>
					<div>
						<div class="text-xs text-gray-400">ASTRO NODES</div>
						<div class="text-2xl font-bold text-neon-cyan">
							{telemetry.astro_nodes.length} ONLINE
						</div>
					</div>
				</div>
			{/if}
		</div>
	</div>

	<!-- ASTRO Node Hardware Showcase Grid + Live High Scores -->
	{#if telemetry}
		<div class="mb-14 grid grid-cols-1 gap-8 lg:grid-cols-3">
			<div class="lg:col-span-2">
				<h2
					class="mb-6 flex items-center gap-2 font-mono text-2xl tracking-wider text-neon-cyan uppercase"
				>
					<span>⚡ ASTRO NODE HARDWARE STATUS</span>
				</h2>

				<div class="grid grid-cols-1 gap-6 sm:grid-cols-2">
					{#each telemetry.astro_nodes as node (node.node_id)}
						<div
							class="relative overflow-hidden rounded-sm border border-neon-cyan/40 bg-gray-950 p-6"
						>
							<div class="mb-4 flex items-start justify-between">
								<div>
									<div class="font-mono text-xs text-gray-400">{node.hardware}</div>
									<div class="font-mono text-lg font-bold text-white">{node.node_id}</div>
								</div>
								<span
									class="rounded px-2 py-1 font-mono text-xs font-bold {node.status === 'IN_USE'
										? 'bg-neon-magenta text-black'
										: 'bg-neon-green text-black'}"
								>
									{node.status}
								</span>
							</div>

							<div class="space-y-2 font-mono text-xs">
								<div>
									<span class="text-gray-400">CURRENT GAME:</span>
									<span class="font-bold text-neon-cyan">{node.current_game}</span>
								</div>
								<div>
									<span class="text-gray-400">EMU CORE:</span>
									<span class="text-gray-200">{node.current_core}</span>
								</div>
								{#if node.session_duration_sec > 0}
									<div>
										<span class="text-gray-400">SESSION:</span>
										<span class="text-neon-green">
											{Math.floor(node.session_duration_sec / 60)}m {node.session_duration_sec % 60}s
										</span>
									</div>
								{/if}
								{#if node.cpu_temp_c !== undefined || node.fps !== undefined}
									<div class="flex justify-between border-t border-gray-800 pt-2 text-[11px]">
										<span>CPU: {node.cpu_temp_c?.toFixed(1) ?? '54.0'}°C</span>
										<span>FPS: {node.fps?.toFixed(1) ?? '60.0'}</span>
										<span>CR: {node.credits_available ?? 0}</span>
									</div>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			</div>

			<!-- Live Venue High Score Leaderboard -->
			<div>
				<h2
					class="mb-6 flex items-center gap-2 font-mono text-2xl tracking-wider text-high-score-orange uppercase"
				>
					<span>🏆 VENUE HIGH SCORES</span>
				</h2>

				<div class="space-y-3 rounded-sm border border-high-score-orange/40 bg-gray-950 p-6">
					{#each telemetry.featured_highscores ?? [] as hs, idx (hs.game + hs.initials + idx)}
						<div
							class="flex items-center justify-between border-b border-gray-800 pb-3 last:border-b-0 last:pb-0 font-mono text-xs"
						>
							<div>
								<div class="font-bold text-white">#{idx + 1} {hs.game}</div>
								<div class="text-[11px] text-neon-cyan">CHAMPION: {hs.initials}</div>
							</div>
							<div class="text-sm font-bold text-neon-green">
								{Number(hs.score).toLocaleString()}
							</div>
						</div>
					{/each}
				</div>
			</div>
		</div>
	{/if}

	<!-- Machine Roster Grid -->
	<div>
		<div class="mb-6 flex flex-col items-center justify-between gap-4 md:flex-row">
			<h2 class="font-mono text-2xl tracking-wider text-neon-magenta uppercase">
				36 MACHINE ROSTER
			</h2>

			<div class="flex flex-wrap items-center gap-3">
				<input
					type="text"
					bind:value={searchQuery}
					placeholder="Filter by title or year..."
					aria-label="Filter machines by title or year"
					class="border border-gray-800 bg-black px-3 py-2 font-mono text-xs text-white outline-none focus:border-neon-magenta"
				/>

				<!-- Category Filter -->
				<div class="flex flex-wrap gap-2 font-mono text-xs">
					{#each ['ALL', 'Pinball', 'ASTRO Arcade', 'Racing', 'Lightgun'] as category (category)}
						<button
							type="button"
							onclick={() => selectCategory(category)}
							class="cursor-pointer border px-4 py-2 uppercase transition-all {selectedCategory ===
							category
								? 'border-neon-magenta bg-neon-magenta font-bold text-black'
								: 'border-gray-800 bg-black text-gray-400 hover:border-gray-600'}"
						>
							{category}
						</button>
					{/each}
				</div>
			</div>
		</div>

		<div class="grid grid-cols-1 gap-4 sm:grid-cols-2 md:grid-cols-4">
			{#each filteredMachines as machine (machine.id)}
				<div
					class="rounded-sm border border-gray-800 bg-gray-950 p-5 transition-colors hover:border-neon-green"
				>
					<div class="mb-2 flex items-start justify-between">
						<span class="font-mono text-xs text-gray-500">{machine.id}</span>
						<span
							class="font-mono text-xs font-bold {machine.status === 'IN_USE'
								? 'text-neon-magenta'
								: 'text-neon-green'}"
						>
							{machine.status === 'IN_USE' ? '● IN USE' : '● READY'}
						</span>
					</div>

					<div class="mb-1 font-mono text-base font-bold text-white">{machine.name}</div>
					<div class="font-mono text-xs text-neon-cyan">{machine.type} • {machine.year}</div>
				</div>
			{/each}
		</div>
	</div>
</div>
