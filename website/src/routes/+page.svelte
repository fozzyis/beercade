<script lang="ts">
  import { onMount } from 'svelte';
  import heroBg from '$lib/assets/hero_bg.jpg';
  import type { VenueTelemetryReport } from '$lib/types/telemetry';

  let telemetry = $state<{
    machines_active: number;
    machines_total: number;
    astro_nodes: Array<{ node_id: string; current_game: string; status: string }>;
  } | null>(null);

  // Slot machine counter state
  let displayMachines = $state(0);
  let displayPrice = $state(0);
  let displayMinutes = $state(0);

  onMount(() => {
    fetch('/api/telemetry/machines')
      .then(r => r.json())
      .then(d => { telemetry = d as any; })
      .catch(console.error);

    const es = new EventSource('/api/v1/edge/stream?channels=venue');
    es.addEventListener('venue', (e) => {
      try {
        const report = JSON.parse((e as MessageEvent).data) as VenueTelemetryReport;
        const nodes = Array.isArray(report.nodes) ? report.nodes : [];
        telemetry = {
          machines_active: report.nodes_online ?? nodes.length,
          machines_total: telemetry?.machines_total ?? Math.max(36, nodes.length),
          astro_nodes: nodes.map((n) => {
            const mode = n.mode && typeof n.mode === 'object' ? (n.mode as Record<string, unknown>) : null;
            const playing = Boolean(mode?.playing || mode?.type === 'Managed');
            return {
              node_id: n.node_id,
              current_game: n.active_rom || n.active_core || 'IDLE',
              status: playing || (n.credits?.credits_available ?? 0) > 0 ? 'IN_USE' : 'AVAILABLE'
            };
          })
        };
      } catch {
        // Ignore malformed SSE payload
      }
    });

    // Slot machine roll: count up from 0 with rapid digit flicker
    let step = 0;
    const steps = 28;
    const interval = setInterval(() => {
      step++;
      if (step < steps) {
        displayMachines = Math.floor(Math.random() * 39);
        displayPrice = Math.floor(Math.random() * 9);
        displayMinutes = Math.floor(Math.random() * 9);
      } else {
        displayMachines = 32;
        displayPrice = 2;
        displayMinutes = 2;
        clearInterval(interval);
      }
    }, 55);

    return () => { es.close(); clearInterval(interval); };
  });
</script>

<section class="min-h-[88vh] flex flex-col justify-center items-center text-center px-4 relative overflow-hidden">
  <!-- Hero Background -->
  <div class="absolute inset-0 z-[-1] bg-after-dark">
    <img src={heroBg} alt="Beercade Redfern Arcade Interior" class="w-full h-full object-cover opacity-40 mix-blend-luminosity scale-105" />
    <div class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/55 to-transparent"></div>
  </div>

  <!-- Glitch Hero Headline -->
  <h1
    class="glitch-text text-4xl md:text-7xl font-mono text-crema mb-4 uppercase tracking-widest glow-neon-orange"
    data-text="A PUB POSSESSED BY PINBALL"
  >
    A PUB POSSESSED BY PINBALL
  </h1>

  <p class="text-lg md:text-2xl max-w-2xl text-crema/90 mb-8 font-sans leading-relaxed">
    Sydney's premier arcade bar. Two minutes from Redfern Station.
  </p>

  <!-- Live Stats — Slot Machine Counters -->
  <div class="grid grid-cols-3 gap-6 md:gap-12 bg-after-dark/90 border border-gray-800 neon-border-cycle p-6 max-w-2xl w-full mb-10 backdrop-blur-md">
    <div>
      <div class="text-3xl md:text-4xl font-mono text-high-score-orange font-bold glow-neon-orange">{displayMachines}</div>
      <div class="text-xs font-mono text-crema/60 uppercase">MACHINES</div>
    </div>
    <div>
      <div class="text-3xl md:text-4xl font-mono text-neon-green font-bold glow-neon-green">${displayPrice}</div>
      <div class="text-xs font-mono text-crema/60 uppercase">A PLAY</div>
    </div>
    <div>
      <div class="text-3xl md:text-4xl font-mono text-neon-cyan font-bold glow-neon-cyan">{displayMinutes} MIN</div>
      <div class="text-xs font-mono text-crema/60 uppercase">FROM TRAIN</div>
    </div>
  </div>

  <!-- Door Policy Box -->
  <div class="bg-gray-950/90 border border-neon-magenta/50 box-glow-magenta p-6 max-w-3xl rounded-sm mb-10 text-left backdrop-blur-sm">
    <h2 class="text-sm font-mono text-neon-magenta mb-2 uppercase tracking-wider font-bold glow-neon-magenta">DOOR POLICY</h2>
    <p class="text-crema/80 text-xs md:text-sm leading-relaxed font-sans">
      And the welcome's for everyone. Gay, straight, trans, non-binary, however you identify and whoever you walked in with; book with us and you'll get no second look you didn't come for. We take the welcome seriously even if we take almost nothing else seriously. The only crowd we've got no time for is wankers. That's the whole door policy.
    </p>
  </div>

  <div class="flex flex-wrap justify-center gap-6">
    <a href="/tokens" class="bg-high-score-orange text-after-dark font-mono px-8 py-3.5 hover:bg-crema transition-all duration-300 uppercase tracking-widest font-bold text-sm box-glow-orange scan-wipe">
      BUY TOKENS ($5 OFFER)
    </a>
    <a href="/bookings" class="bg-transparent border-2 border-neon-cyan text-neon-cyan font-mono px-8 py-3.5 hover:bg-neon-cyan hover:text-after-dark transition-all duration-300 uppercase tracking-widest font-bold text-sm box-glow-cyan scan-wipe">
      BOOK A FUNCTION
    </a>
  </div>
</section>

<!-- Live Telemetry & Quick Action Showcase -->
<section class="py-16 px-4 max-w-6xl mx-auto border-t border-neon-cyan/20 mt-12">
  <div class="grid grid-cols-1 md:grid-cols-2 gap-12">
    <div>
      <div class="flex justify-between items-center mb-4">
        <h3 class="text-2xl font-mono text-neon-cyan glow-neon-cyan">LIVE ASTRO TELEMETRY</h3>
        <span class="inline-flex items-center gap-1.5 text-xs font-mono text-neon-green glow-neon-green">
          <span class="h-2 w-2 rounded-full bg-neon-green animate-ping"></span> LIVE STREAM
        </span>
      </div>

      <div class="bg-black border border-neon-cyan/40 box-glow-cyan p-6 font-mono text-sm space-y-4 rounded-sm">
        {#if telemetry}
          <div class="flex justify-between border-b border-gray-800 pb-3">
            <span class="text-gray-400">ACTIVE MACHINES:</span>
            <span class="text-neon-green font-bold glow-neon-green">{telemetry.machines_active} / {telemetry.machines_total}</span>
          </div>

          <div class="space-y-2">
            <div class="text-xs text-gray-500 uppercase">ASTRO Units In Action:</div>
            {#each telemetry.astro_nodes as node}
              <div class="flex justify-between text-xs bg-gray-950 p-2 border border-gray-800 hover:border-high-score-orange transition-colors">
                <span class="text-white font-bold">{node.node_id}</span>
                <span class="text-neon-magenta">{node.current_game}</span>
              </div>
            {/each}
          </div>

          <a href="/machines" class="block text-center text-xs text-neon-cyan hover:text-white uppercase pt-2 glow-neon-cyan">
            VIEW ALL MACHINES &rarr;
          </a>
        {:else}
          <div class="h-32 flex items-center justify-center text-gray-500 animate-pulse">
            CONNECTING TO BOSS TELEMETRY...
          </div>
        {/if}
      </div>
    </div>

    <div>
      <h3 class="text-2xl font-mono text-neon-magenta mb-4 glow-neon-magenta">FUNCTION BOOKINGS</h3>
      <div class="bg-black border border-neon-magenta/40 box-glow-magenta p-6 font-mono text-sm flex flex-col justify-between h-[230px] rounded-sm scan-wipe">
        <div>
          <div class="text-white font-bold text-lg mb-2">HOST YOUR SQUAD AT THE BK</div>
          <p class="text-gray-400 text-xs leading-relaxed">
            Reserve Main Bar, Pinball Alley, or ASTRO Lounge. Lock in group exclusive sliders, woodfired pizzas, and discounted token packs.
          </p>
        </div>

        <a href="/bookings" class="w-full text-center bg-neon-magenta text-black py-3 font-bold uppercase tracking-wider hover:bg-white transition-colors">
          BOOK AN EVENT NOW
        </a>
      </div>
    </div>
  </div>
</section>
