<script lang="ts">
	import { onMount } from 'svelte';
	import favicon from '$lib/assets/favicon.svg';
	import '@fontsource/jetbrains-mono/400.css';
	import '@fontsource/jetbrains-mono/700.css';
	import '@fontsource/archivo/400.css';
	import '@fontsource/archivo/600.css';
	import '@fontsource/archivo/800.css';
	import '@fontsource/bungee/400.css';
	import '@fontsource/vt323/400.css';
	import '@fontsource/major-mono-display/400.css';
	import '@fontsource/share-tech-mono/400.css';
	import '@fontsource/racing-sans-one/400.css';
	import '@fontsource/chakra-petch/700.css';
	import '@fontsource/chakra-petch/700-italic.css';
	import '@fontsource/shrikhand/400.css';
	import '@fontsource/space-mono/700.css';
	import '@fontsource/libre-franklin/400.css';
	import '@fontsource/libre-franklin/600.css';
	import '@fontsource/libre-franklin/800.css';
	import '$lib/theme/themes.css';
	import '$lib/theme/vector.css';
	import '$lib/theme/bigsky.css';
	import '$lib/theme/pinball.css';
	import { initTheme } from '$lib/theme/themes';

	let { children } = $props();

	onMount(() => initTheme());
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children()}
