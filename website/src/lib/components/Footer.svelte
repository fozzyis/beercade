<script lang="ts">
  import { onMount } from 'svelte';

  let isOpen = $state(false);
  let nextOpenTime = $state('4:00 PM');
  
  onMount(() => {
    const updateStatus = () => {
      const now = new Date();
      // Sydney Time (AEST/AEDT)
      const options = { timeZone: 'Australia/Sydney', hour: '2-digit', minute: '2-digit', hour12: false } as const;
      const timeStr = now.toLocaleTimeString('en-AU', options);
      const [hour] = timeStr.split(':').map(Number);
      
      // Open 4PM (16:00) to 2AM (02:00)
      if (hour >= 16 || hour < 2) {
        isOpen = true;
      } else {
        isOpen = false;
        nextOpenTime = '4:00 PM';
      }
    };
    
    updateStatus();
    const interval = setInterval(updateStatus, 60000); // Check every minute
    return () => clearInterval(interval);
  });
</script>

<footer class="border-t border-gray-800 bg-after-dark py-12 px-6 font-mono text-sm mt-auto relative z-10 text-crema">
  <div class="max-w-6xl mx-auto space-y-8">
    <div class="flex flex-col md:flex-row justify-between items-start gap-6 border-b border-gray-800 pb-8">
      <div>
        <h2 class="text-2xl font-mono text-high-score-orange font-bold tracking-widest mb-2">BEERCADE REDFERN</h2>
        <p class="text-xs text-crema/70">113-115 Regent Street, Redfern NSW 2016</p>
        <p class="text-xs text-crema/70">Two minutes from Redfern Station.</p>
      </div>

      <div class="flex items-center gap-3">
        <span class="relative flex h-3 w-3">
          {#if isOpen}
            <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-neon-green opacity-75"></span>
            <span class="relative inline-flex rounded-full h-3 w-3 bg-neon-green"></span>
          {:else}
            <span class="relative inline-flex rounded-full h-3 w-3 bg-red-500"></span>
          {/if}
        </span>
        <span class="text-xs md:text-sm font-bold tracking-wider">
          {#if isOpen}
            OPEN NOW • 113-115 REGENT ST
          {:else}
            CLOSED • OPENS AT {nextOpenTime}
          {/if}
        </span>
      </div>
    </div>

    <!-- Authentic Acknowledgement of Country & Door Policy -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-8 text-xs text-crema/80 leading-relaxed">
      <div class="space-y-2 border-l-2 border-high-score-orange pl-4">
        <div class="text-high-score-orange font-bold uppercase tracking-wider">ACKNOWLEDGEMENT OF COUNTRY</div>
        <p>
          Beercade pours on the land of the Gadigal people of the Eora Nation, and here in Redfern, a place that has been a heart of Aboriginal Sydney for generations. We acknowledge the Gadigal as the traditional custodians of this land, pay our respects to Elders past and present, and extend that respect to every Aboriginal and Torres Strait Islander person who walks through the door. Sovereignty was never ceded. This always was, and always will be, Aboriginal land.
        </p>
      </div>

      <div class="space-y-2 border-l-2 border-neon-magenta pl-4">
        <div class="text-neon-magenta font-bold uppercase tracking-wider">DOOR POLICY</div>
        <p>
          And the welcome’s for everyone. Gay, straight, trans, non-binary, however you identify and whoever you walked in with; book with us and you’ll get no second look you didn’t come for. We take the welcome seriously even if we take almost nothing else seriously. The only crowd we’ve got no time for is wankers. That’s the whole door policy.
        </p>
      </div>
    </div>

    <div class="pt-6 border-t border-gray-900 text-center text-xs text-crema/40 space-y-1">
      <p>Licensed venue. Licensee: Roger Robertson. Liquor Licence No. LIQS220000240.</p>
      <p>© 2026 Beercade Australia. Drink responsibly.</p>
    </div>
  </div>
</footer>
