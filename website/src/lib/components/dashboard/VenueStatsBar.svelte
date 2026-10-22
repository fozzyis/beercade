<script lang="ts">
	/**
	 * Venue-wide header strip: live connection badge, aggregate health
	 * numbers and the rolling alert feed from the BOSS alerts channel.
	 */
	import type { VenueStore } from '$lib/telemetry/venue-store.svelte';

	interface Props {
		store: VenueStore;
		nowMs: number;
	}

	const { store, nowMs } = $props();

	let connectionLabel = $derived(
		store.connection === 'live'
			? 'LIVE'
			: store.connection === 'connecting'
				? 'CONNECTING'
				: store.connection === 'reconnecting'
					? 'RECONNECTING'
					: 'OFFLINE'
	);
	let connectionClass = $derived(
		store.connection === 'live'
			? 'text-neon-green border-neon-green'
			: store.connection === 'reconnecting'
				? 'text-high-score-orange border-high-score-orange'
				: 'text-neon-magenta border-neon-magenta'
	);
	let secondsSinceEvent = $derived(
		store.lastEventAt === null ? null : Math.floor((nowMs - store.lastEventAt) / 1000)
	);

	function alertAge(ms: number): string {
		const sec = Math.max(0, Math.floor((nowMs - ms) / 1000));
		if (sec < 60) return `${sec}s ago`;
		const min = Math.floor(sec / 60);
		if (min < 60) return `${min}m ago`;
		return `${Math.floor(min / 60)}h ago`;
	}
</script>

<div class="mb-8 rounded-sm border border-gray-800 bg-gray-950/70">
	<div class="flex flex-col justify-between gap-4 p-5 md:flex-row md:items-center">
		<div>
			<div class="flex items-center gap-3">
				<h1
					class="glow-neon-green font-mono text-2xl font-bold tracking-widest text-neon-green uppercase md:text-3xl"
				>
					VENUE OPS
				</h1>
				<span class="rounded border px-2 py-0.5 font-mono text-[10px] font-bold {connectionClass}">
					{connectionLabel}
				</span>
			</div>
			{#if store.venue}
				<p class="mt-1 text-sm text-crema/70">
					{store.venue.name} — {store.venue.address}
				</p>
			{/if}
		</div>

		<div class="grid grid-cols-2 gap-4 text-center font-mono sm:grid-cols-4">
			<div>
				<div class="text-[10px] tracking-widest text-gray-500 uppercase">Online</div>
				<div class="text-2xl font-bold text-neon-cyan">{store.onlineNodes.length}</div>
			</div>
			<div>
				<div class="text-[10px] tracking-widest text-gray-500 uppercase">Avg CPU</div>
				<div class="text-2xl font-bold text-crema">
					{store.avgCpuTemp === null ? '—' : `${store.avgCpuTemp.toFixed(0)}°`}
				</div>
			</div>
			<div>
				<div class="text-[10px] tracking-widest text-gray-500 uppercase">Avg FPS</div>
				<div class="text-2xl font-bold text-crema">
					{store.avgFps === null ? '—' : store.avgFps.toFixed(1)}
				</div>
			</div>
			<div>
				<div class="text-[10px] tracking-widest text-gray-500 uppercase">Netplay</div>
				<div class="text-2xl font-bold text-neon-magenta">{store.netplaySessions}</div>
			</div>
		</div>
	</div>

	{#if secondsSinceEvent !== null && secondsSinceEvent > 5}
		<div class="border-t border-gray-800 px-5 py-1 font-mono text-[10px] text-gray-500">
			last stream event: {secondsSinceEvent}s ago
		</div>
	{/if}

	{#if store.alerts.length > 0}
		<div class="border-t border-gray-800 px-5 py-3">
			<div class="mb-2 font-mono text-[10px] tracking-widest text-gray-500 uppercase">
				Alert Feed
			</div>
			<ul class="max-h-40 space-y-1 overflow-y-auto">
				{#each store.alerts.slice(0, 10) as alert (alert.id)}
					<li class="flex justify-between gap-4 font-mono text-xs">
						<span
							class="{alert.severity === 'fatal'
								? 'text-neon-magenta'
								: alert.severity === 'warn'
									? 'text-high-score-orange'
									: 'text-neon-green'} truncate"
						>
							{alert.summary}
						</span>
						<span class="whitespace-nowrap text-gray-600">{alertAge(alert.at_epoch_ms)}</span>
					</li>
				{/each}
			</ul>
		</div>
	{/if}
</div>
