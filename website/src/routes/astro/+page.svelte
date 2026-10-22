<script lang="ts">
  import { onMount } from 'svelte';
  import { playClickSound, playCoinSound } from '$lib/audio';
  import astroHero from '$lib/assets/astro_hero.jpg';

  // Exactly 4 ASTRO Units
  let astroUnits = $state([
    {
      id: 'ASTRO-01',
      name: 'ASTRO Alpha',
      hardware: 'ASTRO Custom Arcade Hardware',
      core: 'fbneo',
      coreName: 'FinalBurn Neo (Arcade)',
      currentGame: 'Street Fighter II: Champion Edition',
      players: '2P Co-Op / VS',
      status: 'IN_USE',
      sessionSec: 642,
      fps: 60,
      tempC: 44.5,
      highScore: '980,500 (RYU)',
      videoUrl: '/videos/astro-01.mp4'
    },
    {
      id: 'ASTRO-02',
      name: 'ASTRO Bravo',
      hardware: 'ASTRO Custom Arcade Hardware',
      core: 'pcsx2',
      coreName: 'PCSX2 (PlayStation 2)',
      currentGame: 'Tekken 3',
      players: '2P Versus',
      status: 'AVAILABLE',
      sessionSec: 0,
      fps: 60,
      tempC: 39.2,
      highScore: 'STAGE 10 CLEAR',
      videoUrl: '/videos/astro-02.mp4'
    },
    {
      id: 'ASTRO-03',
      name: 'ASTRO Charlie',
      hardware: 'ASTRO Custom Arcade Hardware',
      core: 'mame2003_plus',
      coreName: 'MAME2003+ (Neo Geo / Arcade)',
      currentGame: 'Metal Slug 3',
      players: '2P Co-Op',
      status: 'IN_USE',
      sessionSec: 1480,
      fps: 60,
      tempC: 46.8,
      highScore: '1,450,200 (TARMA)',
      videoUrl: '/videos/astro-03.mp4'
    },
    {
      id: 'ASTRO-04',
      name: 'ASTRO Delta',
      hardware: 'ASTRO Custom Arcade Hardware',
      core: 'flycast',
      coreName: 'Flycast (Sega Dreamcast / NAOMI)',
      currentGame: 'Marvel vs. Capcom 2',
      players: '2P Versus',
      status: 'IN_USE',
      sessionSec: 810,
      fps: 60,
      tempC: 42.1,
      highScore: '56 WIN STREAK',
      videoUrl: '/videos/astro-04.mp4'
    }
  ]);

  // Selected unit for live stream modal
  let activeStreamUnit = $state<typeof astroUnits[0] | null>(null);

  // Available games library search & filter
  let searchQuery = $state('');
  let selectedCoreFilter = $state('ALL');

  const gameLibrary = [
    { name: 'Street Fighter II: Champion Edition', core: 'FBNeo', genre: 'Fighting', players: 2, icon: '👊' },
    { name: 'Tekken 3', core: 'PCSX2', genre: '3D Fighting', players: 2, icon: '⚡' },
    { name: 'Metal Slug 3', core: 'MAME', genre: 'Run & Gun', players: 2, icon: '💥' },
    { name: 'Marvel vs. Capcom 2', core: 'Flycast', genre: 'Fighting', players: 2, icon: '☄️' },
    { name: 'Windjammers', core: 'FBNeo', genre: 'Sports Arcade', players: 2, icon: '🥏' },
    { name: 'Garou: Mark of the Wolves', core: 'FBNeo', genre: 'Fighting', players: 2, icon: '🐺' },
    { name: 'Virtua Fighter 4 Evolution', core: 'Flycast', genre: '3D Fighting', players: 2, icon: '⛩️' },
    { name: 'Pac-Man / Ms. Pac-Man', core: 'MAME', genre: 'Classic Maze', players: 2, icon: '🍒' },
    { name: 'Capcom vs. SNK 2', core: 'Flycast', genre: 'Fighting', players: 2, icon: '🔥' },
    { name: 'Soulcalibur II', core: 'PCSX2', genre: '3D Fighting', players: 2, icon: '⚔️' }
  ];

  let filteredGames = $derived(
    gameLibrary.filter(game => {
      const matchesSearch = game.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
                            game.genre.toLowerCase().includes(searchQuery.toLowerCase());
      const matchesCore = selectedCoreFilter === 'ALL' || game.core === selectedCoreFilter;
      return matchesSearch && matchesCore;
    })
  );

  function openStream(unit: typeof astroUnits[0]) {
    playCoinSound();
    activeStreamUnit = unit;
  }

  function closeStream() {
    playClickSound();
    activeStreamUnit = null;
  }
</script>

<svelte:head>
  <title>4x ASTRO UNITS & LIVE GAMEPLAY STREAMS | BEERCADE REDFERN</title>
</svelte:head>

<div class="max-w-6xl mx-auto py-12 px-4 text-crema">
  <!-- Hero Image Banner -->
  <div class="relative rounded-sm overflow-hidden mb-10 h-56 md:h-80">
    <img src={astroHero} alt="ASTRO Arcade Units Live Stream" class="w-full h-full object-cover object-center vhs-video" />
    <div class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/30 to-transparent"></div>
    <div class="absolute inset-0 flex flex-col justify-end p-8">
      <div class="inline-block bg-high-score-orange/20 border border-high-score-orange box-glow-orange px-4 py-1 text-xs font-mono text-high-score-orange uppercase mb-3 tracking-widest font-bold w-fit">
        4x ASTRO UNITS ONLINE
      </div>
      <h1 class="glitch-text text-4xl md:text-6xl font-mono text-high-score-orange uppercase tracking-widest font-bold glow-neon-orange" data-text="ASTRO UNITS &amp; LIVE STREAMS">
        ASTRO UNITS &amp; LIVE STREAMS
      </h1>
      <p class="text-crema/80 font-sans max-w-2xl text-lg mt-2">
        Click any ASTRO unit card below to launch the live video stream!
      </p>
    </div>
  </div>

  <!-- 4 ASTRO Unit Boxes Grid -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-8 mb-16">
    {#each astroUnits as unit}
      <button
        onclick={() => openStream(unit)}
        class="scan-wipe neon-border-cycle group relative bg-after-dark border-2 border-gray-800 p-6 rounded-sm text-left transition-all duration-300 overflow-hidden cursor-pointer active:scale-98"
      >
        <!-- Real Streaming Video Gameplay Background Loop (VHS Warp) -->
        <div class="vhs-tear absolute inset-0 z-0 overflow-hidden opacity-30 group-hover:opacity-55 transition-opacity">
          <video
            src={unit.videoUrl}
            autoplay
            loop
            muted
            playsinline
            class="vhs-video w-full h-full object-cover scale-110"
          ></video>
          <div class="absolute inset-0 bg-after-dark/55 mix-blend-multiply"></div>
        </div>

        <div class="relative z-10 flex justify-between items-start mb-4">
          <div>
            <span class="font-mono text-xs text-neon-cyan tracking-widest font-bold glow-neon-cyan">{unit.hardware}</span>
            <h2 class="font-mono text-2xl text-crema font-bold group-hover:text-high-score-orange transition-colors">{unit.id} • {unit.name}</h2>
          </div>

          <span class="px-3 py-1 text-xs font-mono font-bold uppercase rounded {unit.status === 'IN_USE' ? 'bg-high-score-orange text-after-dark animate-pulse glow-neon-orange' : 'bg-neon-green text-after-dark glow-neon-green'}">
            {unit.status === 'IN_USE' ? '● LIVE PLAYING' : '● AVAILABLE'}
          </span>
        </div>

        <!-- Live Game Display Card -->
        <div class="relative z-10 bg-after-dark/90 border border-gray-800 p-4 rounded mb-4 backdrop-blur-sm">
          <div class="text-xs font-mono text-crema/60 mb-1 uppercase">CURRENT GAMEPLAY:</div>
          <div class="text-lg font-mono text-high-score-orange font-bold mb-2 glow-neon-orange">{unit.currentGame}</div>
          <div class="flex justify-between text-xs font-mono text-crema/80">
            <span>CORE: <strong class="text-neon-cyan glow-neon-cyan">{unit.coreName}</strong></span>
            <span>PLAYERS: <strong class="text-crema">{unit.players}</strong></span>
          </div>
        </div>

        <!-- Real-time Hardware Telemetry Bar -->
        <div class="relative z-10 flex justify-between items-center text-xs font-mono text-crema/60 pt-2 border-t border-gray-800">
          <span>FPS: <strong class="text-neon-green glow-neon-green">{unit.fps} FPS</strong></span>
          <span>TEMP: <strong class="text-neon-cyan glow-neon-cyan">{unit.tempC}°C</strong></span>
          <span class="text-neon-magenta font-bold glow-neon-magenta group-hover:translate-x-1 transition-transform">
            ▶ WATCH STREAM &rarr;
          </span>
        </div>
      </button>
    {/each}
  </div>

  <!-- Available Games Library Section -->
  <div class="bg-after-dark border border-gray-800 p-8 rounded-sm">
    <div class="flex flex-col md:flex-row justify-between items-center mb-8 gap-4">
      <div>
        <h2 class="text-2xl font-mono text-high-score-orange uppercase tracking-wider font-bold">ASTRO GAMES LIBRARY</h2>
        <p class="text-xs font-mono text-crema/60">Available across all 4 ASTRO stations</p>
      </div>

      <!-- Search & Filters -->
      <div class="flex flex-wrap items-center gap-3 w-full md:w-auto">
        <input
          type="text"
          bind:value={searchQuery}
          placeholder="Search games or genre..."
          class="bg-black border border-gray-800 focus:border-high-score-orange p-2 text-xs font-mono text-crema outline-none w-full md:w-64"
        />

        <div class="flex gap-1 font-mono text-xs">
          {#each ['ALL', 'FBNeo', 'PCSX2', 'Flycast', 'MAME'] as coreFilter}
            <button
              onclick={() => { playClickSound(); selectedCoreFilter = coreFilter; }}
              class="px-3 py-1.5 border text-[11px] uppercase transition-colors cursor-pointer {selectedCoreFilter === coreFilter ? 'border-high-score-orange bg-high-score-orange text-after-dark font-bold' : 'border-gray-800 bg-black text-crema/60'}"
            >
              {coreFilter}
            </button>
          {/each}
        </div>
      </div>
    </div>

    <!-- Games Roster Cards -->
    <div class="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
      {#each filteredGames as game}
        <div class="bg-black border border-gray-800 p-4 rounded-sm hover:border-high-score-orange transition-colors flex items-center gap-4">
          <div class="text-3xl p-2 bg-gray-900 border border-gray-800 rounded">{game.icon}</div>
          <div class="font-mono">
            <div class="text-sm text-crema font-bold">{game.name}</div>
            <div class="text-xs text-high-score-orange">{game.genre} • <span class="text-crema/60">{game.core}</span></div>
          </div>
        </div>
      {/each}
    </div>
  </div>
</div>

<!-- Fullscreen Live ASTRO Gameplay Stream Modal -->
{#if activeStreamUnit}
  <div class="fixed inset-0 z-[200] bg-black/95 backdrop-blur-md flex items-center justify-center p-4">
    <div class="bg-after-dark border-2 border-high-score-orange p-6 max-w-4xl w-full rounded-sm shadow-[0_0_50px_rgba(255,95,31,0.5)] relative text-crema">
      <div class="flex justify-between items-center mb-4 border-b border-gray-800 pb-3">
        <div class="flex items-center gap-3">
          <span class="h-3 w-3 rounded-full bg-neon-green animate-ping"></span>
          <span class="font-mono text-lg text-high-score-orange font-bold">LIVE WEBRTC HARDWARE FEED: {activeStreamUnit.id} ({activeStreamUnit.name})</span>
        </div>
        <button onclick={closeStream} class="text-crema/70 hover:text-crema font-mono text-xs font-bold px-3 py-1 border border-gray-800 hover:border-high-score-orange cursor-pointer">
          ✕ CLOSE STREAM
        </button>
      </div>

      <!-- Real Gameplay Video Stream Container (VHS Warp) -->
      <div class="vhs-tear relative aspect-video bg-black border-2 border-high-score-orange box-glow-orange overflow-hidden mb-4 flex items-center justify-center">
        <video
          src={activeStreamUnit.videoUrl}
          autoplay
          loop
          muted
          playsinline
          class="vhs-video w-full h-full object-cover"
        ></video>

        <!-- Overlay HUD Telemetry -->
        <div class="absolute top-4 left-4 bg-after-dark/90 border border-neon-green p-2 font-mono text-xs text-neon-green font-bold glow-neon-green">
          LIVE: {activeStreamUnit.currentGame} ({activeStreamUnit.coreName})
        </div>

        <div class="absolute bottom-4 right-4 bg-after-dark/90 border border-neon-cyan p-2 font-mono text-xs text-neon-cyan flex gap-4 font-bold glow-neon-cyan">
          <span>FPS: 60.0</span>
          <span>LATENCY: 12ms</span>
          <span>ENCODER: ASTRO VPU H.264</span>
        </div>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-4 font-mono text-xs">
        <div class="bg-black p-3 border border-gray-800">
          <div class="text-high-score-orange font-bold mb-1">PLAYER 1 TELEMETRY</div>
          <div class="text-crema/60">STICK: NEUTRAL • BUTTONS: A+B ACTIVE</div>
        </div>
        <div class="bg-black p-3 border border-gray-800">
          <div class="text-neon-magenta font-bold mb-1">PLAYER 2 TELEMETRY</div>
          <div class="text-crema/60">STICK: RIGHT • BUTTONS: C ACTIVE</div>
        </div>
      </div>
    </div>
  </div>
{/if}
