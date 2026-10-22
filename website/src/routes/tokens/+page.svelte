<script lang="ts">
  import { playClickSound, playCoinSound, playSuccessSound } from '$lib/audio';
  import tokensHero from '$lib/assets/tokens_hero.jpg';

  const tiers = [
    { amount: 5, tokens: 5, bonus: 0, tag: 'QUICK GAME' },
    { amount: 10, tokens: 10, bonus: 0, tag: 'SINGLE PLAYER' },
    { amount: 20, tokens: 20, bonus: 0, tag: 'CO-OP SESSION' },
    { amount: 50, tokens: 55, bonus: 10, tag: 'HIGH SCORE PACK' },
    { amount: 100, tokens: 115, bonus: 15, tag: 'ARCADE BOSS' },
    { amount: 200, tokens: 240, bonus: 20, tag: 'PARTY LOUNGE' },
    { amount: 300, tokens: 375, bonus: 25, tag: 'VIP SQUAD' },
    { amount: 500, tokens: 650, bonus: 30, tag: 'LEGENDARY TIER' }
  ];

  let selectedTier = $state(tiers[3]); // Default $50
  let email = $state('');
  let isProcessing = $state(false);
  let voucherResult = $state<{
    voucherId: string;
    email: string;
    amount: number;
    tokens: number;
    bonusTokens: number;
    qrData: string;
    passUrl: string;
  } | null>(null);

  function selectTier(tier: typeof tiers[0]) {
    playCoinSound();
    selectedTier = tier;
  }

  async function handleCheckout() {
    if (!email) {
      alert('Please enter your email address for voucher delivery.');
      return;
    }
    playClickSound();
    isProcessing = true;

    try {
      const res = await fetch('/api/checkout/token-pack', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ amount: selectedTier.amount, email })
      });
      const data = (await res.json()) as any;
      if (res.ok && data.success) {
        playSuccessSound();
        voucherResult = data;
      } else {
        alert(data.error || 'Checkout failed');
      }
    } catch (e) {
      alert('Failed to connect to checkout engine');
    } finally {
      isProcessing = false;
    }
  }

  function resetPurchase() {
    playClickSound();
    voucherResult = null;
  }
</script>

<svelte:head>
  <title>TOKEN PACK STORE | THE BK</title>
</svelte:head>

<div class="max-w-5xl mx-auto py-12 px-4">
  <!-- Hero Image Banner -->
  <div class="relative rounded-sm overflow-hidden mb-12 h-52 md:h-72">
    <img src={tokensHero} alt="Arcade Token Pack Store" class="w-full h-full object-cover object-center vhs-video" />
    <div class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/40 to-transparent"></div>
    <div class="absolute inset-0 flex flex-col justify-end p-8">
      <h1 class="glitch-text text-4xl md:text-6xl font-mono text-neon-green uppercase tracking-widest glow-neon-green font-bold" data-text="TOKEN PACK STORE">
        TOKEN PACK STORE
      </h1>
      <p class="text-crema/80 font-sans text-lg mt-2">
        Buy arcade tokens online. Instantly receive an 8-bit scannable QR voucher.
      </p>
    </div>
  </div>

  {#if !voucherResult}
    <!-- Tiers Grid -->
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4 mb-12">
      {#each tiers as tier}
        <button
          onclick={() => selectTier(tier)}
          class="relative p-6 border-2 transition-all text-left flex flex-col justify-between h-48 cursor-pointer active:scale-95 {selectedTier.amount === tier.amount ? 'border-neon-green bg-neon-green/10 shadow-[0_0_20px_rgba(57,255,20,0.4)]' : 'border-gray-800 bg-black/60 hover:border-neon-cyan'}"
        >
          {#if tier.bonus > 0}
            <span class="absolute -top-3 right-3 bg-neon-magenta text-black text-xs font-mono font-bold px-2 py-0.5 uppercase tracking-wider">
              +{tier.bonus}% BONUS
            </span>
          {/if}

          <div>
            <div class="text-xs font-mono text-gray-400 mb-1">{tier.tag}</div>
            <div class="text-3xl font-mono text-white font-bold">${tier.amount}</div>
          </div>

          <div>
            <div class="text-2xl font-mono text-neon-green font-bold">{tier.tokens} TOKENS</div>
            {#if tier.bonus > 0}
              <div class="text-xs font-mono text-neon-cyan">Includes bonus!</div>
            {/if}
          </div>
        </button>
      {/each}
    </div>

    <!-- Checkout Form Box -->
    <div class="bg-gray-950 border-2 border-neon-cyan/50 p-8 max-w-xl mx-auto rounded-sm backdrop-blur-md">
      <h2 class="text-xl font-mono text-neon-cyan mb-6 uppercase tracking-wider text-center">
        CONFIRM SELECTION: <span class="text-neon-green">${selectedTier.amount} ({selectedTier.tokens} TOKENS)</span>
      </h2>

      <div class="mb-6">
        <label for="email" class="block font-mono text-xs text-gray-300 mb-2 uppercase">DELIVERY EMAIL ADDRESS</label>
        <input
          id="email"
          type="email"
          bind:value={email}
          placeholder="your.name@example.com"
          class="w-full bg-black border border-gray-700 focus:border-neon-magenta p-3 font-mono text-white outline-none transition-colors"
        />
      </div>

      <button
        onclick={handleCheckout}
        disabled={isProcessing}
        class="w-full bg-neon-green text-black font-mono py-4 text-lg font-bold hover:bg-white transition-all uppercase tracking-widest shadow-[0_0_15px_var(--color-neon-green)] active:scale-98 disabled:opacity-50 cursor-pointer"
      >
        {isProcessing ? 'ISSUING VOUCHER...' : `PURCHASE $${selectedTier.amount} PACK`}
      </button>

      <p class="text-center text-xs text-gray-500 font-mono mt-4">
        🔒 Stripe Edge Encrypted • Instant QR Voucher Issued
      </p>
    </div>
  {:else}
    <!-- Issued Voucher Card -->
    <div class="bg-gray-950 border-2 border-neon-green p-8 max-w-md mx-auto text-center rounded-sm shadow-[0_0_30px_rgba(57,255,20,0.3)] animate-fade-in">
      <div class="text-xs font-mono text-neon-green tracking-widest uppercase mb-2">★ VOUCHER ISSUED ★</div>
      <h2 class="text-3xl font-mono text-white mb-4">${voucherResult.amount} TOKEN VOUCHER</h2>

      <!-- Mock Retro QR Code Container -->
      <div class="bg-white p-6 inline-block my-4 rounded border-4 border-black">
        <div class="w-44 h-44 bg-[radial-gradient(#000_30%,transparent_30%)] bg-[length:12px_12px] flex items-center justify-center relative">
          <div class="absolute inset-0 border-8 border-black flex items-center justify-center">
            <span class="font-mono text-xs font-bold bg-black text-neon-green p-1">THE BK QR</span>
          </div>
        </div>
      </div>

      <div class="font-mono text-sm text-neon-cyan mb-2">VOUCHER ID: {voucherResult.voucherId}</div>
      <div class="font-mono text-lg text-neon-green font-bold mb-6">{voucherResult.tokens} TOKENS CREDITED</div>

      <p class="text-xs text-gray-400 font-mono mb-6">
        Sent to <span class="text-white">{voucherResult.email}</span>. Show this QR code at the bar or scan at any on-premise token dispenser.
      </p>

      <div class="flex flex-col gap-3">
        <a
          href={voucherResult.passUrl}
          target="_blank"
          class="bg-black border border-gray-600 text-white font-mono py-2 px-4 hover:border-white text-xs uppercase transition-colors"
        >
          📱 ADD TO APPLE WALLET
        </a>
        <button
          onclick={resetPurchase}
          class="bg-transparent border border-neon-magenta text-neon-magenta font-mono py-2 px-4 hover:bg-neon-magenta hover:text-black text-xs uppercase transition-colors"
        >
          BUY ANOTHER PACK
        </button>
      </div>
    </div>
  {/if}
</div>
