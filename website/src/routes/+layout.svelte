<script lang="ts">
  import './layout.css';
  import favicon from '$lib/assets/favicon.svg';
  import Header from '$lib/components/Header.svelte';
  import Footer from '$lib/components/Footer.svelte';
  import { onNavigate } from '$app/navigation';
  import { onMount } from 'svelte';

  let { children } = $props();
  let crtEnabled = $state(true);
  let showFlash = $state(false);

  // Page transition glitch flash
  onNavigate(() => {
    showFlash = true;
    setTimeout(() => { showFlash = false; }, 240);
  });

  // Marquee ticker items
  const tickerItems = [
    '🎮 32 MACHINES • $2 A PLAY',
    '⚡ ASTRO-01: STREET FIGHTER II IN PROGRESS',
    '🏆 HIGH SCORE: 980,500 (RYU)',
    '🍺 CRAFT BEER ON TAP',
    '🎯 2 MINUTES FROM REDFERN STATION',
    '🎉 FUNCTION BOOKINGS OPEN — GROUP RATES AVAILABLE',
    '👾 ASTRO-03: METAL SLUG 3 IN PROGRESS',
    '🔴 OPEN 4PM — 2AM',
    '🏅 HIGH SCORE: 1,450,200 (TARMA)',
    '🎪 $5 TOKEN STARTER PACKS AVAILABLE',
    '💥 ASTRO-04: MARVEL VS. CAPCOM 2 IN PROGRESS',
    '🎵 113-115 REGENT STREET, REDFERN NSW 2016',
  ];
  const tickerStr = [...tickerItems, ...tickerItems].join('   ✦   ');
</script>

<svelte:head>
  <link rel="icon" href={favicon} />
  <title>BEERCADE REDFERN — A Pub Possessed by Pinball</title>
</svelte:head>

<!-- SVG Filters for Barrel Distortion & Grain -->
<svg class="absolute w-0 h-0 overflow-hidden" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <filter id="crt-barrel-filter" x="-5%" y="-5%" width="110%" height="110%">
      <!-- Chromatic aberration: red channel offset -->
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0" result="red-chan" in="SourceGraphic"/>
      <feOffset in="red-chan" dx="-1.5" dy="0" result="red-shifted"/>
      <!-- Blue channel offset -->
      <feColorMatrix type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0" result="blue-chan" in="SourceGraphic"/>
      <feOffset in="blue-chan" dx="1.5" dy="0" result="blue-shifted"/>
      <!-- Merge all channels -->
      <feMerge>
        <feMergeNode in="red-shifted"/>
        <feMergeNode in="SourceGraphic"/>
        <feMergeNode in="blue-shifted"/>
      </feMerge>
    </filter>
  </defs>
</svg>

<div class="flex flex-col min-h-screen relative overflow-x-hidden">
  <!-- Page Transition Glitch Flash -->
  {#if showFlash}
    <div class="page-transition-flash"></div>
  {/if}

  <!-- CRT Scanline + Jitter Overlay -->
  {#if crtEnabled}
    <div class="crt-overlay"></div>
  {/if}

  <!-- Film Grain Noise -->
  <div class="film-grain"></div>

  <Header />

  <!-- Marquee Live Ticker -->
  <div class="border-b border-gray-900 bg-after-dark/95 py-1.5 overflow-hidden relative z-40">
    <div class="ticker-wrap">
      <span class="ticker-content font-mono text-xs text-high-score-orange tracking-widest">
        {tickerStr}
      </span>
    </div>
  </div>

  <main class="flex-grow relative z-0">
    {@render children()}
  </main>

  <Footer />

  <!-- CRT Toggle (floating) -->
  <button
    onclick={() => { crtEnabled = !crtEnabled; }}
    class="fixed bottom-24 right-4 z-[90] p-2 bg-black border border-neon-cyan text-neon-cyan text-xs font-mono opacity-50 hover:opacity-100 transition-opacity"
    aria-label="Toggle CRT Scanlines"
  >
    CRT: {crtEnabled ? 'ON' : 'OFF'}
  </button>
</div>
