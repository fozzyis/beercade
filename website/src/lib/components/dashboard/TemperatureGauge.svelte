<script lang="ts">
	/**
	 * Semicircular SVG gauge for cabinet temperature (or any bounded metric).
	 * Color thresholds follow the venue health policy:
	 *   < warn  -> neon green, < crit -> high-score orange, >= crit -> magenta.
	 */
	interface Props {
		label: string;
		value: number;
		min?: number;
		max?: number;
		unit?: string;
		warn?: number;
		crit?: number;
	}

	const { label, value, min = 20, max = 100, unit = '°C', warn = 70, crit = 85 } = $props();

	const RADIUS = 46;
	const CIRCUMFERENCE = Math.PI * RADIUS; // half-circle arc length

	let clamped = $derived(Math.min(max, Math.max(min, value)));
	let ratio = $derived((clamped - min) / (max - min));
	let strokeColor = $derived(
		value >= crit
			? 'var(--color-neon-magenta)'
			: value >= warn
				? 'var(--color-high-score-orange)'
				: 'var(--color-neon-green)'
	);
	let displayValue = $derived(Math.round(value * 10) / 10);
</script>

<div class="flex flex-col items-center gap-1 select-none">
	<svg viewBox="0 0 120 66" class="h-16 w-28" role="img" aria-label="{label}: {displayValue}{unit}">
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
			class="fill-white font-mono"
			style="font-size: 17px; font-weight: bold;"
		>
			{displayValue}
			<tspan style="font-size: 10px;">{unit}</tspan>
		</text>
	</svg>
	<div class="font-mono text-[10px] tracking-widest text-gray-400 uppercase">{label}</div>
</div>
