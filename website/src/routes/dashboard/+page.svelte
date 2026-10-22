<script lang="ts">
	/**
	 * Live venue operations dashboard — real-time cabinet health cards fed by
	 * the multiplexed BOSS SSE hub (proxied through /api/v1/edge/stream).
	 */
	import { onMount } from 'svelte';
	import { venueStore } from '$lib/telemetry/venue-store.svelte';
	import CabinetHealthCard from '$lib/components/dashboard/CabinetHealthCard.svelte';
	import VenueStatsBar from '$lib/components/dashboard/VenueStatsBar.svelte';

	let nowMs = $state(Date.now());

	onMount(() => {
		venueStore.connect('venue,alerts,commands');
		// Cheap "now" ticker so staleness/UI-relative times update without
		// waiting for the next telemetry frame.
		const ticker = setInterval(() => (nowMs = Date.now()), 1000);
		return () => {
			clearInterval(ticker);
			venueStore.dispose();
		};
	});

	let nodes = $derived([...venueStore.allNodes].sort((a, b) => a.node_id.localeCompare(b.node_id)));
</script>

<svelte:head>
	<title>VENUE OPS DASHBOARD | THE BK</title>
</svelte:head>

<div class="mx-auto max-w-7xl px-4 py-10">
	<VenueStatsBar store={venueStore} {nowMs} />

	<div class="mb-5 flex items-center justify-between">
		<h2 class="font-mono text-xl tracking-wider text-neon-cyan uppercase">Cabinet Health</h2>
		<span class="font-mono text-xs text-gray-500">
			{nodes.length} node{nodes.length === 1 ? '' : 's'} tracked · SSE channels: venue / alerts / commands
		</span>
	</div>

	{#if venueStore.connection === 'connecting'}
		<div
			class="rounded-sm border border-gray-800 bg-gray-950 p-10 text-center font-mono text-gray-500"
		>
			<span class="animate-pulse">Establishing SSE link to B.O.S.S. hub…</span>
		</div>
	{:else if nodes.length === 0}
		<div
			class="rounded-sm border border-gray-800 bg-gray-950 p-10 text-center font-mono text-gray-500"
		>
			{#if venueStore.connection === 'reconnecting'}
				<span class="animate-pulse text-high-score-orange">
					BOSS hub unreachable — retrying with backoff…
				</span>
			{:else}
				No cabinet telemetry received yet. Waiting for heartbeats…
			{/if}
		</div>
	{:else}
		<div class="grid grid-cols-1 gap-6 md:grid-cols-2 xl:grid-cols-3">
			{#each nodes as node (node.node_id)}
				<CabinetHealthCard {node} {nowMs} />
			{/each}
		</div>
	{/if}
</div>
