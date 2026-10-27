<script lang="ts">
	import { onMount } from 'svelte';
	import favicon from '$lib/assets/favicon.svg';
	import '@fontsource/jetbrains-mono/latin-400.css';
	import '@fontsource/jetbrains-mono/latin-700.css';
	import '@fontsource/archivo/latin-400.css';
	import '@fontsource/archivo/latin-600.css';
	import '@fontsource/archivo/latin-800.css';
	import '@fontsource/bungee/latin-400.css';
	import '@fontsource/vt323/latin-400.css';
	import '@fontsource/major-mono-display/latin-400.css';
	import '@fontsource/share-tech-mono/latin-400.css';
	import '@fontsource/racing-sans-one/latin-400.css';
	import '@fontsource/chakra-petch/latin-700.css';
	import '@fontsource/chakra-petch/latin-700-italic.css';
	import '@fontsource/shrikhand/latin-400.css';
	import '@fontsource/space-mono/latin-700.css';
	import '@fontsource/libre-franklin/latin-400.css';
	import '@fontsource/libre-franklin/latin-600.css';
	import '@fontsource/libre-franklin/latin-800.css';
	import '$lib/theme/themes.css';
	import '$lib/theme/vector.css';
	import '$lib/theme/bigsky.css';
	import '$lib/theme/pinball.css';
	import '$lib/theme/controls.css';
	import '$lib/theme/effects.css';
	import { initTheme } from '$lib/theme/themes';

	let { children } = $props();

	onMount(() => initTheme());
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children()}
