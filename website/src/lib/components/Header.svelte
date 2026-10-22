<script lang="ts">
	import {
		playClickSound,
		isAudioEnabled,
		toggleAudio,
		isBGMEnabled,
		toggleBGM,
		getCurrentSongName
	} from '$lib/audio';

	let audioOn = $state(true);
	let bgmOn = $state(false);
	let songName = $state('');

	// Update song name display every second when BGM is on
	import { onMount } from 'svelte';
	onMount(() => {
		const id = setInterval(() => {
			if (bgmOn) songName = getCurrentSongName();
		}, 1000);
		return () => clearInterval(id);
	});

	function handleAudioToggle() {
		audioOn = toggleAudio();
		if (audioOn) {
			playClickSound();
		}
	}

	function handleBgmToggle() {
		bgmOn = toggleBGM();
		if (bgmOn) {
			audioOn = true;
		}
	}

	function handleNavClick() {
		playClickSound();
	}
</script>

<header
	class="sticky top-0 z-50 border-b border-gray-800 bg-after-dark/95 p-4 text-crema backdrop-blur-md"
>
	<div class="mx-auto flex max-w-6xl items-center justify-between">
		<div class="flex items-center gap-4">
			<a
				href="/"
				onclick={handleNavClick}
				class="glow-neon-magenta font-mono text-2xl font-bold tracking-widest text-high-score-orange transition-colors duration-200 hover:text-crema"
			>
				BEERCADE
			</a>
			<nav class="ml-6 hidden gap-5 font-mono text-sm md:flex">
				<a
					href="/tokens"
					onclick={handleNavClick}
					class="text-crema/80 transition-all duration-200 hover:scale-105 hover:text-high-score-orange"
					>TOKENS</a
				>
				<a
					href="/astro"
					onclick={handleNavClick}
					class="font-bold text-neon-green transition-all duration-200 hover:scale-105 hover:text-white"
					>ASTRO LIVE</a
				>
				<a
					href="/bookings"
					onclick={handleNavClick}
					class="text-crema/80 transition-all duration-200 hover:scale-105 hover:text-neon-cyan"
					>BOOKINGS</a
				>
				<a
					href="/machines"
					onclick={handleNavClick}
					class="text-crema/80 transition-all duration-200 hover:scale-105 hover:text-neon-cyan"
					>MACHINES</a
				>
				<a
					href="/dashboard"
					onclick={handleNavClick}
					class="text-crema/80 transition-all duration-200 hover:scale-105 hover:text-neon-magenta"
					>OPS DASHBOARD</a
				>
				<a
					href="/menu"
					onclick={handleNavClick}
					class="text-crema/80 transition-all duration-200 hover:scale-105 hover:text-neon-green"
					>MENU</a
				>
			</nav>
		</div>

		<div class="flex items-center gap-2 md:gap-3">
			<button
				onclick={handleBgmToggle}
				class="cursor-pointer border border-gray-800 p-2 font-mono text-xs text-crema/80 transition-colors hover:border-high-score-orange hover:text-high-score-orange {bgmOn
					? 'border-high-score-orange bg-high-score-orange/10 font-bold text-high-score-orange'
					: ''}"
				title="Toggle 8-Bit Retro Background Music"
			>
				{bgmOn ? `🎵 ${songName || 'BGM: ON'}` : '🔇 BGM: OFF'}
			</button>

			<button
				onclick={handleAudioToggle}
				class="cursor-pointer border border-gray-800 p-2 font-mono text-xs text-crema/80 transition-colors hover:border-neon-cyan hover:text-neon-cyan"
				title="Toggle Sound Effects"
			>
				{audioOn ? '🔊 SFX ON' : '🔇 SFX OFF'}
			</button>

			<a
				href="/bookings"
				onclick={handleNavClick}
				class="bg-high-score-orange px-4 py-2 font-mono text-xs font-bold tracking-wider text-after-dark uppercase shadow-[0_0_10px_rgba(255,95,31,0.4)] transition-all duration-200 hover:bg-crema active:scale-95 md:text-sm"
			>
				Book Function
			</a>
		</div>
	</div>
</header>
