<script lang="ts">
  import { onMount } from 'svelte';
  import { playClickSound } from '$lib/audio';
  import machinesHero from '$lib/assets/machines_hero.jpg';

  let telemetry = $state<{
    timestamp: string;
    venue_status: string;
    machines_total: number;
    machines_active: number;
    astro_nodes: Array<{
      node_id: string;
      hardware: string;
      current_core: string;
      current_game: string;
      status: string;
      session_duration_sec: number;
    }>;
    featured_highscores: Array<{ game: string; score: number; initials: string }>;
  } | null>(null);

  let selectedCategory = $state('ALL');

  const machines = [
    { id: 'M-01', name: 'Godzilla Premium', type: 'Pinball', status: 'IN_USE', year: 2021 },
    { id: 'M-02', name: 'Attack from Mars', type: 'Pinball', status: 'IN_USE', year: 1995 },
    { id: 'M-03', name: 'Medieval Madness', type: 'Pinball', status: 'AVAILABLE', year: 1997 },
    { id: 'M-04', name: 'The Addams Family', type: 'Pinball', status: 'IN_USE', year: 1992 },
    { id: 'M-05', name: 'Street Fighter II: Champion Edition', type: 'ASTRO Arcade', status: 'IN_USE', year: 1992 },
    { id: 'M-06', name: 'Tekken 2', type: 'ASTRO Arcade', status: 'AVAILABLE', year: 1996 },
    { id: 'M-07', name: 'Metal Slug 3', type: 'ASTRO Arcade', status: 'IN_USE', year: 2000 },
    { id: 'M-08', name: 'Marvel vs. Capcom 2', type: 'ASTRO Arcade', status: 'IN_USE', year: 2000 },
    { id: 'M-09', name: 'Daytona USA Twin', type: 'Racing', status: 'IN_USE', year: 1994 },
    { id: 'M-10', name: 'Sega Rally Championship', type: 'Racing', status: 'AVAILABLE', year: 1994 },
    { id: 'M-11', name: 'Time Crisis 2', type: 'Lightgun', status: 'IN_USE', year: 1997 },
    { id: 'M-12', name: 'House of the Dead 2', type: 'Lightgun', status: 'AVAILABLE', year: 1998 }
  ];

  let filteredMachines = $derived(
    selectedCategory === 'ALL' ? machines : machines.filter(m => m.type === selectedCategory)
  );

  onMount(() => {
    // Initial REST fetch
    fetch('/api/telemetry/machines')
      .then(res => res.json())
      .then(data => { telemetry = data as any; })
      .catch(console.error);

    // Subscribe to SSE stream for live updates
    const eventSource = new EventSource('/api/telemetry/stream');
    eventSource.onmessage = (event) => {
      try {
        telemetry = JSON.parse(event.data);
      } catch (err) {
        console.error('Failed to parse telemetry event:', err);
      }
    };

    return () => eventSource.close();
  });

  function selectCategory(cat: string) {
    playClickSound();
    selectedCategory = cat;
  }
</script>

<svelte:head>
  <title>MACHINE ROSTER & LIVE TELEMETRY | THE BK</title>
</svelte:head>

<div class="max-w-6xl mx-auto py-12 px-4">
  <!-- Hero Image Banner -->
  <div class="relative rounded-sm overflow-hidden mb-12 h-52 md:h-80">
    <img src={machinesHero} alt="Beercade Machine Roster" class="w-full h-full object-cover object-center vhs-video" />
    <div class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/30 to-transparent"></div>
    <div class="absolute inset-0 flex flex-col justify-end p-8">
      <h1 class="glitch-text text-4xl md:text-6xl font-mono text-neon-green uppercase tracking-widest glow-neon-green font-bold" data-text="MACHINE ROSTER">
        MACHINE ROSTER
      </h1>
      <p class="text-crema/80 font-sans text-lg mt-2">
        Real-time hardware status from our on-premise BOSS tunnel &amp; custom ASTRO nodes.
      </p>
    </div>
  </div>

  <!-- Live Telemetry Status Banner -->
  <div class="bg-black border-2 border-neon-green/60 p-6 rounded-sm mb-12 shadow-[0_0_20px_rgba(57,255,20,0.2)]">
    <div class="flex flex-wrap justify-between items-center gap-4">
      <div class="flex items-center gap-3">
        <span class="relative flex h-4 w-4">
          <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-neon-green opacity-75"></span>
          <span class="relative inline-flex rounded-full h-4 w-4 bg-neon-green"></span>
        </span>
        <div>
          <div class="font-mono text-xs text-gray-400">BOSS TUNNEL STATUS</div>
          <div class="font-mono text-lg text-neon-green font-bold">ONLINE • LIVE STREAM ACTIVE</div>
        </div>
      </div>

      {#if telemetry}
        <div class="flex gap-8 font-mono">
          <div>
            <div class="text-xs text-gray-400">MACHINES ACTIVE</div>
            <div class="text-2xl text-white font-bold">{telemetry.machines_active} / {telemetry.machines_total}</div>
          </div>
          <div>
            <div class="text-xs text-gray-400">ASTRO NODES</div>
            <div class="text-2xl text-neon-cyan font-bold">{telemetry.astro_nodes.length} ONLINE</div>
          </div>
        </div>
      {/if}
    </div>
  </div>

  <!-- ASTRO Node Hardware Showcase Grid -->
  {#if telemetry}
    <div class="mb-14">
      <h2 class="text-2xl font-mono text-neon-cyan mb-6 uppercase tracking-wider flex items-center gap-2">
        <span>⚡ ASTRO RK3588 NODE HARDWARE STATUS</span>
      </h2>

      <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
        {#each telemetry.astro_nodes as node}
          <div class="bg-gray-950 border border-neon-cyan/40 p-6 rounded-sm relative overflow-hidden">
            <div class="flex justify-between items-start mb-4">
              <div>
                <div class="font-mono text-xs text-gray-400">{node.hardware}</div>
                <div class="font-mono text-lg text-white font-bold">{node.node_id}</div>
              </div>
              <span class="px-2 py-1 text-xs font-mono rounded font-bold {node.status === 'IN_USE' ? 'bg-neon-magenta text-black' : 'bg-neon-green text-black'}">
                {node.status}
              </span>
            </div>

            <div class="space-y-2 font-mono text-xs">
              <div><span class="text-gray-400">CURRENT GAME:</span> <span class="text-neon-cyan font-bold">{node.current_game}</span></div>
              <div><span class="text-gray-400">EMU CORE:</span> <span class="text-gray-200">{node.current_core}</span></div>
              {#if node.session_duration_sec > 0}
                <div><span class="text-gray-400">SESSION:</span> <span class="text-neon-green">{Math.floor(node.session_duration_sec / 60)}m {node.session_duration_sec % 60}s</span></div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Machine Roster Grid -->
  <div>
    <div class="flex flex-col md:flex-row justify-between items-center mb-6 gap-4">
      <h2 class="text-2xl font-mono text-neon-magenta uppercase tracking-wider">36 MACHINE ROSTER</h2>

      <!-- Category Filter -->
      <div class="flex flex-wrap gap-2 font-mono text-xs">
        {#each ['ALL', 'Pinball', 'ASTRO Arcade', 'Racing', 'Lightgun'] as category}
          <button
            onclick={() => selectCategory(category)}
            class="px-4 py-2 border uppercase transition-all cursor-pointer {selectedCategory === category ? 'border-neon-magenta bg-neon-magenta text-black font-bold' : 'border-gray-800 bg-black text-gray-400 hover:border-gray-600'}"
          >
            {category}
          </button>
        {/each}
      </div>
    </div>

    <div class="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 gap-4">
      {#each filteredMachines as machine}
        <div class="bg-gray-950 border border-gray-800 p-5 rounded-sm hover:border-neon-green transition-colors">
          <div class="flex justify-between items-start mb-2">
            <span class="text-xs font-mono text-gray-500">{machine.id}</span>
            <span class="text-xs font-mono font-bold {machine.status === 'IN_USE' ? 'text-neon-magenta' : 'text-neon-green'}">
              {machine.status === 'IN_USE' ? '● IN USE' : '● READY'}
            </span>
          </div>

          <div class="font-mono text-white font-bold text-base mb-1">{machine.name}</div>
          <div class="text-xs font-mono text-neon-cyan">{machine.type} • {machine.year}</div>
        </div>
      {/each}
    </div>
  </div>
</div>
