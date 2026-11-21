<script lang="ts">
	/**
	 * Semicircular SVG gauge for cabinet temperature (or any bounded metric).
	 * Color thresholds follow the venue health policy:
	 *   < warn  -> neon green, < crit -> high-score orange, >= crit -> magenta.
	 */
	interface Props {
		label: string;
		value: number | null | undefined;
		min?: number;
		max?: number;
		unit?: string;
		warn?: number;
		crit?: number;
	}

	const { label, value, min = 20, max = 100, unit = '°C', warn = 70, crit = 85 }: Props = $props();

	const RADIUS = 46;
	const CIRCUMFERENCE = Math.PI * RADIUS; // half-circle arc length

	let numericValue = $derived(value ?? null);
	let clamped = $derived(
		numericValue === null ? min : Math.min(max, Math.max(min, numericValue))
	);
	let ratio = $derived(numericValue === null ? 0 : (clamped - min) / (max - min));
	let strokeColor = $derived(
		numericValue === null
			? 'rgba(255,255,255,0.16)'
			: numericValue >= crit
				? 'var(--color-neon-magenta)'
				: numericValue >= warn
					? 'var(--color-high-score-orange)'
					: 'var(--color-neon-green)'
	);
	let displayValue = $derived(numericValue === null ? '--' : Math.round(numericValue * 10) / 10);
	let ariaLabel = $derived(
		numericValue === null ? `${label}: unavailable` : `${label}: ${displayValue}${unit}`
	);
</script>

<div class="flex flex-col items-center gap-1 select-none">
	<svg viewBox="0 0 120 66" class="h-16 w-28" role="img" aria-label={ariaLabel}>
		<!-- track -->
		<path
			d="M 14 60 A {RADIUS} {RADIUS} 0 0 1 106 60"
			fill="none"
			stroke="rgba(255,255,255,0.08)"
			stroke-width="9"
			stroke-linecap="round"
		/>
		<!-- value arc -->
		<path
			d="M 14 60 A {RADIUS} {RADIUS} 0 0 1 106 60"
			fill="none"
			stroke={strokeColor}
			stroke-width="9"
			stroke-linecap="round"
			stroke-dasharray={CIRCUMFERENCE}
			stroke-dashoffset={CIRCUMFERENCE * (1 - ratio)}
			class="transition-[stroke-dashoffset,stroke] duration-700 ease-out"
		/>
		<text
			x="60"
			y="52"
			text-anchor="middle"
			class="font-mono {numericValue === null ? 'fill-gray-500' : 'fill-white'}"
			style="font-size: 17px; font-weight: bold;"
		>
			{#if numericValue === null}
				--
			{:else}
				{displayValue}
				<tspan style="font-size: 10px;">{unit}</tspan>
			{/if}
		</text>
	</svg>
	<div class="font-mono text-[10px] tracking-widest text-gray-400 uppercase">{label}</div>
</div>
