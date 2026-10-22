<script lang="ts">
	/**
	 * Live cabinet health card: temperature gauges, current ROM, FPS,
	 * 1000Hz evdev input latency, NPU load, P2P netplay RTT and credits.
	 * Driven entirely by a `NodeTelemetry` heartbeat from BOSS.
	 */
	import type { NodeTelemetry } from '$lib/types/telemetry';
	import TemperatureGauge from './TemperatureGauge.svelte';

	interface Props {
		node: NodeTelemetry;
		/** Server-synced "now", refreshed periodically by the parent. */
		nowMs?: number;
	}

	const { node, nowMs = Date.now() } = $props();

	const STALENESS_MS = 15_000;

	let online = $derived(nowMs - node.timestamp_epoch_ms < STALENESS_MS);
	let netplay = $derived(node.netplay);
	let fpsColor = $derived(
		node.fps >= 55
			? 'text-neon-green'
			: node.fps >= 45
				? 'text-high-score-orange'
				: 'text-neon-magenta'
	);
	let latencyColor = $derived(
		node.input_jitter.p99_us <= 500
			? 'text-neon-green'
			: node.input_jitter.p99_us <= 1000
				? 'text-high-score-orange'
				: 'text-neon-magenta'
	);
	let npuRatio = $derived(Math.min(1, Math.max(0, node.npu_load_pct / 100)));
	let secondsSinceHeartbeat = $derived(Math.floor((nowMs - node.timestamp_epoch_ms) / 1000));

	function formatDuration(totalSeconds: number): string {
		const m = Math.floor(totalSeconds / 60);
		const s = totalSeconds % 60;
		return `${m}m ${s.toString().padStart(2, '0')}s`;
	}
</script>

<div
	class="relative overflow-hidden rounded-sm border p-5 transition-colors {online
		? 'border-neon-cyan/40 bg-gray-950 hover:border-neon-cyan'
		: 'border-gray-800 bg-black opacity-60'}"
>
	<!-- Header -->
	<div class="mb-4 flex items-start justify-between">
		<div>
			<div class="font-mono text-[10px] tracking-widest text-gray-500 uppercase">
				{node.hardware}
			</div>
			<div class="font-mono text-lg font-bold text-white">{node.node_id}</div>
			<div class="font-mono text-[10px] text-gray-500">{node.soc}</div>
		</div>
		<div class="flex flex-col items-end gap-1">
			<span
				class="rounded px-2 py-0.5 font-mono text-[10px] font-bold {online
					? 'bg-neon-green text-black'
					: 'bg-gray-700 text-gray-300'}"
			>
				{online ? '● ONLINE' : '○ OFFLINE'}
			</span>
			{#if online}
				<span class="font-mono text-[10px] text-gray-500">
					{secondsSinceHeartbeat}s ago
				</span>
			{:else}
				<span class="font-mono text-[10px] text-neon-magenta">
					{formatDuration(secondsSinceHeartbeat)} silent
				</span>
			{/if}
		</div>
	</div>

	<!-- Temperature gauges -->
	<div class="mb-4 flex justify-around">
		<TemperatureGauge label="CPU" value={node.cpu_temp_c} />
		<TemperatureGauge label="GPU" value={node.gpu_temp_c} />
	</div>

	<!-- Current ROM -->
	<div class="mb-3">
		<div class="font-mono text-[10px] tracking-widest text-gray-500 uppercase">Now Playing</div>
		<div class="truncate font-mono text-sm font-bold text-neon-cyan">{node.active_rom}</div>
		<div class="font-mono text-[10px] text-gray-400">
			core: <span class="text-gray-200">{node.active_core}</span>
		</div>
	</div>

	<!-- FPS + frame time -->
	<div class="mb-3 grid grid-cols-2 gap-3 font-mono text-xs">
		<div>
			<div class="text-[10px] tracking-widest text-gray-500 uppercase">FPS</div>
			<div class="{fpsColor} text-base font-bold">{node.fps.toFixed(1)}</div>
			<div class="text-gray-500">{node.frame_time_ms.toFixed(1)} ms/frame</div>
		</div>
		<div>
			<div class="text-[10px] tracking-widest text-gray-500 uppercase">Input p99 (1kHz evdev)</div>
			<div class="{latencyColor} text-base font-bold">{Math.round(node.input_jitter.p99_us)}µs</div>
			<div class="text-gray-500">
				max {Math.round(node.input_jitter.max_us)}µs · {node.input_jitter.dropped_events} dropped
			</div>
		</div>
	</div>

	<!-- NPU load bar -->
	<div class="mb-3">
		<div class="mb-1 flex justify-between font-mono text-[10px] tracking-widest uppercase">
			<span class="text-gray-500">NPU Load</span>
			<span class="font-bold text-neon-cyan">{node.npu_load_pct.toFixed(0)}%</span>
		</div>
		<div class="h-1.5 overflow-hidden rounded-full bg-gray-800">
			<div
				class="h-full bg-neon-cyan transition-all duration-700 ease-out"
				style="width: {npuRatio * 100}%"
			></div>
		</div>
	</div>

	<!-- Netplay -->
	{#if netplay}
		<div class="mb-3 rounded-sm border border-tilt-purple bg-tilt-purple/30 p-2">
			<div class="mb-1 flex items-center justify-between font-mono text-xs">
				<span class="text-[10px] font-bold tracking-widest text-neon-magenta uppercase">
					⚔ P2P Netplay {netplay.is_host ? '(HOST)' : '(GUEST)'}
				</span>
				<span class="font-bold text-white">{netplay.rtt_ms.toFixed(0)}ms RTT</span>
			</div>
			<div class="font-mono text-[10px] text-gray-400">
				peer {netplay.peer_id} · jitter {netplay.jitter_ms.toFixed(1)}ms · loss
				{netplay.packet_loss_pct.toFixed(1)}% · rollback {netplay.rollback_frames} f/s
			</div>
		</div>
	{/if}

	<!-- Credits -->
	<div class="grid grid-cols-3 gap-2 font-mono text-xs">
		<div>
			<div class="text-[10px] tracking-widest text-gray-500 uppercase">Credits</div>
			<div class="text-base font-bold text-high-score-orange">{node.credits.credits_available}</div>
		</div>
		<div>
			<div class="text-[10px] tracking-widest text-gray-500 uppercase">Today</div>
			<div class="font-bold text-crema">{node.credits.credits_dispensed_today}</div>
		</div>
		<div>
			<div class="text-[10px] tracking-widest text-gray-500 uppercase">Lifetime</div>
			<div class="font-bold text-crema">{node.credits.credits_inserted_total}</div>
		</div>
	</div>
</div>
