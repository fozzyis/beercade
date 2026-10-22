<script lang="ts">
  import { playClickSound, playCoinSound, playSuccessSound } from '$lib/audio';
  import bookingsHero from '$lib/assets/bookings_hero.jpg';

  // Multi-step form state
  let currentStep = $state(1);

  // Step 1: Event & Venue Info
  let eventType = $state('Birthday');
  let bookingDate = $state('2026-08-15');
  let bookingTime = $state('18:00');
  let headcount = $state(12);
  let selectedArea = $state('Pinball Alley');

  // Step 2: Food Pre-Orders
  const foodItems = [
    { id: 'f1', name: 'Arcade Sliders Platter (20x)', price: 120, category: 'Platters', tag: 'Popular', qty: 0 },
    { id: 'f2', name: 'Loaded Fries & Wings Combo', price: 90, category: 'Platters', tag: 'GF Option', qty: 0 },
    { id: 'f3', name: 'Gourmet Woodfired Pizza Pack (5x)', price: 140, category: 'Pizza', tag: 'Vegan Option', qty: 0 },
    { id: 'f4', name: 'Retro Taco Fiesta Tray', price: 110, category: 'Platters', tag: 'Halal Option', qty: 0 }
  ];
  let selectedFood = $state(foodItems);

  // Step 3: Drinks & Token Bundles
  let drinkBundle = $state('Classic Bar Tab');
  let tokenAddonTier = $state(100); // Default $100 token pack add-on

  // Step 4: Contact & Payment Info
  let contactName = $state('');
  let contactEmail = $state('');
  let contactPhone = $state('');
  let isSubmitting = $state(false);

  let bookingResult = $state<{
    bookingRef: string;
    depositAmount: number;
    foodTotal: number;
    tokenAddonAmount: number;
    totalPaid: number;
    summary: {
      eventType: string;
      date: string;
      time: string;
      headcount: number;
      area: string;
      contactEmail: string;
    };
    calendarEventUrl: string;
  } | null>(null);

  // Computed totals
  let foodTotal = $derived(selectedFood.reduce((sum, item) => sum + (item.price * item.qty), 0));
  let depositAmount = $derived(headcount * 20); // $20 deposit per person
  let grandTotal = $derived(depositAmount + foodTotal + tokenAddonTier);

  function nextStep() {
    playClickSound();
    if (currentStep < 4) currentStep += 1;
  }

  function prevStep() {
    playClickSound();
    if (currentStep > 1) currentStep -= 1;
  }

  function updateFoodQty(itemIndex: number, change: number) {
    playCoinSound();
    const newQty = selectedFood[itemIndex].qty + change;
    if (newQty >= 0) {
      selectedFood[itemIndex].qty = newQty;
    }
  }

  async function submitBooking() {
    if (!contactName || !contactEmail || !contactPhone) {
      alert('Please complete all contact details.');
      return;
    }
    playClickSound();
    isSubmitting = true;

    try {
      const res = await fetch('/api/checkout/booking', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          eventType,
          date: bookingDate,
          time: bookingTime,
          headcount,
          area: selectedArea,
          foodPreOrders: selectedFood.filter(i => i.qty > 0).map(i => ({ price: i.price, quantity: i.qty })),
          drinkBundle,
          tokenAddonAmount: tokenAddonTier,
          contactName,
          contactEmail,
          contactPhone
        })
      });

      const data = (await res.json()) as any;
      if (res.ok && data.success) {
        playSuccessSound();
        bookingResult = data;
      } else {
        alert(data.error || 'Failed to lock booking');
      }
    } catch (e) {
      alert('Error connecting to booking service');
    } finally {
      isSubmitting = false;
    }
  }
</script>

<svelte:head>
  <title>FUNCTION & EVENT BOOKINGS | THE BK</title>
</svelte:head>

<div class="max-w-4xl mx-auto py-12 px-4">
  <!-- Hero Image Banner -->
  <div class="relative rounded-sm overflow-hidden mb-10 h-52 md:h-72">
    <img src={bookingsHero} alt="Function & Event Bookings at Beercade" class="w-full h-full object-cover object-center vhs-video" />
    <div class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/40 to-transparent"></div>
    <div class="absolute inset-0 flex flex-col justify-end p-8">
      <h1 class="glitch-text text-4xl md:text-6xl font-mono text-neon-cyan uppercase tracking-widest glow-neon-cyan font-bold" data-text="FUNCTION BOOKINGS">
        FUNCTION BOOKINGS
      </h1>
      <p class="text-crema/80 font-sans mt-2">
        Host your birthday, hen's night, corporate event or squad session at The BK Redfern.
      </p>
    </div>
  </div>

  {#if !bookingResult}
    <!-- Step Progress Bar -->
    <div class="flex justify-between items-center mb-10 border-b border-gray-800 pb-4 font-mono text-xs md:text-sm">
      <div class="{currentStep >= 1 ? 'text-neon-cyan font-bold' : 'text-gray-600'}">1. EVENT DETAILS</div>
      <div class="{currentStep >= 2 ? 'text-neon-magenta font-bold' : 'text-gray-600'}">2. FOOD PACKAGES</div>
      <div class="{currentStep >= 3 ? 'text-neon-green font-bold' : 'text-gray-600'}">3. DRINKS & TOKENS</div>
      <div class="{currentStep >= 4 ? 'text-white font-bold' : 'text-gray-600'}">4. DEPOSIT & LOCK</div>
    </div>

    <!-- Step 1: Event Details -->
    {#if currentStep === 1}
      <div class="bg-gray-950 border border-neon-cyan/40 p-8 rounded-sm space-y-6">
        <h2 class="text-xl font-mono text-neon-cyan uppercase tracking-wider">STEP 1: SELECT EVENT & VENUE AREA</h2>

        <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div>
            <label class="block font-mono text-xs text-gray-300 mb-2 uppercase">EVENT TYPE</label>
            <select bind:value={eventType} class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-cyan">
              <option>Birthday Party</option>
              <option>Hen's / Buck's Party</option>
              <option>Corporate Event</option>
              <option>Casual Squad Reservation</option>
            </select>
          </div>

          <div>
            <label class="block font-mono text-xs text-gray-300 mb-2 uppercase">HEADCOUNT (GUESTS)</label>
            <input type="number" min="4" max="100" bind:value={headcount} class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-cyan" />
          </div>

          <div>
            <label class="block font-mono text-xs text-gray-300 mb-2 uppercase">RESERVATION DATE</label>
            <input type="date" bind:value={bookingDate} class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-cyan" />
          </div>

          <div>
            <label class="block font-mono text-xs text-gray-300 mb-2 uppercase">START TIME</label>
            <input type="time" bind:value={bookingTime} class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-cyan" />
          </div>
        </div>

        <div>
          <label class="block font-mono text-xs text-gray-300 mb-2 uppercase">PREFERRED AREA</label>
          <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
            {#each ['Main Bar', 'Pinball Alley', 'ASTRO Lounge'] as area}
              <button
                type="button"
                onclick={() => selectedArea = area}
                class="p-4 border-2 font-mono text-center uppercase text-sm cursor-pointer transition-all {selectedArea === area ? 'border-neon-cyan text-neon-cyan bg-neon-cyan/10' : 'border-gray-800 text-gray-400 bg-black hover:border-gray-600'}"
              >
                {area}
              </button>
            {/each}
          </div>
        </div>

        <div class="flex justify-end pt-4">
          <button onclick={nextStep} class="bg-neon-cyan text-black font-mono px-8 py-3 font-bold uppercase tracking-wider hover:bg-white transition-colors">
            NEXT: FOOD SELECTION &rarr;
          </button>
        </div>
      </div>
    {/if}

    <!-- Step 2: Food Pre-Orders -->
    {#if currentStep === 2}
      <div class="bg-gray-950 border border-neon-magenta/40 p-8 rounded-sm space-y-6">
        <h2 class="text-xl font-mono text-neon-magenta uppercase tracking-wider">STEP 2: GROUP EXCLUSIVE FOOD PRE-ORDERS</h2>

        <div class="space-y-4">
          {#each selectedFood as item, idx}
            <div class="flex items-center justify-between p-4 bg-black border border-gray-800 rounded-sm">
              <div>
                <div class="font-mono text-white font-bold">{item.name}</div>
                <div class="text-xs font-mono text-neon-magenta">${item.price} • <span class="text-gray-400">{item.tag}</span></div>
              </div>

              <div class="flex items-center gap-3 font-mono">
                <button onclick={() => updateFoodQty(idx, -1)} class="w-8 h-8 bg-gray-800 text-white font-bold hover:bg-neon-magenta hover:text-black">-</button>
                <span class="w-8 text-center text-lg">{item.qty}</span>
                <button onclick={() => updateFoodQty(idx, 1)} class="w-8 h-8 bg-gray-800 text-white font-bold hover:bg-neon-green hover:text-black">+</button>
              </div>
            </div>
          {/each}
        </div>

        <div class="flex justify-between items-center pt-4">
          <button onclick={prevStep} class="border border-gray-700 text-gray-300 font-mono px-6 py-3 uppercase text-xs hover:border-white">
            &larr; BACK
          </button>
          <button onclick={nextStep} class="bg-neon-magenta text-black font-mono px-8 py-3 font-bold uppercase tracking-wider hover:bg-white transition-colors">
            NEXT: DRINKS & TOKENS &rarr;
          </button>
        </div>
      </div>
    {/if}

    <!-- Step 3: Drinks & Token Bundles -->
    {#if currentStep === 3}
      <div class="bg-gray-950 border border-neon-green/40 p-8 rounded-sm space-y-6">
        <h2 class="text-xl font-mono text-neon-green uppercase tracking-wider">STEP 3: DRINK TABS & DISCOUNTED TOKEN PACKS</h2>

        <div>
          <label class="block font-mono text-xs text-gray-300 mb-2 uppercase">BAR TAB / DRINK PACKAGE</label>
          <select bind:value={drinkBundle} class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-green">
            <option>Pay As You Go (Individual Tabs)</option>
            <option>Classic Bar Tab (Beer, Wine, Spirits)</option>
            <option>VIP Cocktail & Craft Beer Experience</option>
          </select>
        </div>

        <div>
          <label class="block font-mono text-xs text-gray-300 mb-2 uppercase">ATTACH DISCOUNTED TOKEN PACK ADD-ON</label>
          <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
            {#each [0, 50, 100, 200] as amt}
              <button
                type="button"
                onclick={() => { playCoinSound(); tokenAddonTier = amt; }}
                class="p-4 border-2 font-mono text-center uppercase text-sm cursor-pointer transition-all {tokenAddonTier === amt ? 'border-neon-green text-neon-green bg-neon-green/10' : 'border-gray-800 text-gray-400 bg-black hover:border-gray-600'}"
              >
                {amt === 0 ? 'NO TOKENS' : `$${amt} PACK`}
              </button>
            {/each}
          </div>
        </div>

        <div class="flex justify-between items-center pt-4">
          <button onclick={prevStep} class="border border-gray-700 text-gray-300 font-mono px-6 py-3 uppercase text-xs hover:border-white">
            &larr; BACK
          </button>
          <button onclick={nextStep} class="bg-neon-green text-black font-mono px-8 py-3 font-bold uppercase tracking-wider hover:bg-white transition-colors">
            NEXT: CONFIRM DEPOSIT &rarr;
          </button>
        </div>
      </div>
    {/if}

    <!-- Step 4: Deposit Checkout -->
    {#if currentStep === 4}
      <div class="bg-gray-950 border-2 border-neon-cyan/50 p-8 rounded-sm space-y-6">
        <h2 class="text-xl font-mono text-neon-cyan uppercase tracking-wider">STEP 4: CONTACT INFO & STRIPE DEPOSIT</h2>

        <!-- Order Summary Box -->
        <div class="bg-black p-4 border border-gray-800 font-mono text-sm space-y-2">
          <div class="flex justify-between"><span>{eventType} ({headcount} guests @ {selectedArea})</span><span>${depositAmount} deposit</span></div>
          {#if foodTotal > 0}<div class="flex justify-between text-neon-magenta"><span>Food Pre-Orders Total</span><span>${foodTotal}</span></div>{/if}
          {#if tokenAddonTier > 0}<div class="flex justify-between text-neon-green"><span>Token Add-on Pack</span><span>${tokenAddonTier}</span></div>{/if}
          <div class="border-t border-gray-700 pt-2 flex justify-between font-bold text-lg text-white">
            <span>TOTAL DUE NOW</span>
            <span class="text-neon-green">${grandTotal}</span>
          </div>
        </div>

        <div class="space-y-4">
          <div>
            <label class="block font-mono text-xs text-gray-300 mb-1 uppercase">CONTACT NAME</label>
            <input type="text" bind:value={contactName} placeholder="Alex Smith" class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-cyan" />
          </div>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label class="block font-mono text-xs text-gray-300 mb-1 uppercase">EMAIL ADDRESS</label>
              <input type="email" bind:value={contactEmail} placeholder="alex@example.com" class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-cyan" />
            </div>

            <div>
              <label class="block font-mono text-xs text-gray-300 mb-1 uppercase">PHONE NUMBER</label>
              <input type="tel" bind:value={contactPhone} placeholder="0400 000 000" class="w-full bg-black border border-gray-700 p-3 font-mono text-white outline-none focus:border-neon-cyan" />
            </div>
          </div>
        </div>

        <div class="flex justify-between items-center pt-4">
          <button onclick={prevStep} class="border border-gray-700 text-gray-300 font-mono px-6 py-3 uppercase text-xs hover:border-white">
            &larr; BACK
          </button>
          <button
            onclick={submitBooking}
            disabled={isSubmitting}
            class="bg-neon-green text-black font-mono px-8 py-4 font-bold text-lg uppercase tracking-wider hover:bg-white transition-all shadow-[0_0_15px_var(--color-neon-green)] cursor-pointer"
          >
            {isSubmitting ? 'LOCKING RESERVATION...' : `PAY DEPOSIT ($${grandTotal}) & LOCK`}
          </button>
        </div>
      </div>
    {/if}
  {:else}
    <!-- Booking Confirmation Card -->
    <div class="bg-gray-950 border-2 border-neon-cyan p-8 max-w-xl mx-auto text-center rounded-sm shadow-[0_0_30px_rgba(0,255,255,0.3)]">
      <div class="text-xs font-mono text-neon-cyan tracking-widest uppercase mb-2">★ RESERVATION LOCKED ★</div>
      <h2 class="text-3xl font-mono text-white mb-2">{bookingResult.summary.eventType}</h2>
      <div class="font-mono text-neon-green text-xl font-bold mb-6">REF: {bookingResult.bookingRef}</div>

      <div class="bg-black p-6 border border-gray-800 font-mono text-sm text-left mb-6 space-y-2">
        <div><span class="text-gray-400">Date & Time:</span> {bookingResult.summary.date} @ {bookingResult.summary.time}</div>
        <div><span class="text-gray-400">Guests & Area:</span> {bookingResult.summary.headcount} Guests • {bookingResult.summary.area}</div>
        <div><span class="text-gray-400">Deposit Paid:</span> ${bookingResult.totalPaid}</div>
      </div>

      <p class="text-xs text-gray-400 font-mono mb-6">
        Confirmation email & calendar invite sent to <span class="text-white">{bookingResult.summary.contactEmail}</span>.
      </p>

      <div class="flex justify-center gap-4">
        <a href={bookingResult.calendarEventUrl} target="_blank" class="bg-black border border-neon-cyan text-neon-cyan font-mono py-2 px-6 hover:bg-neon-cyan hover:text-black text-xs uppercase transition-colors">
          📅 ADD TO CALENDAR
        </a>
        <button onclick={() => bookingResult = null} class="bg-transparent border border-gray-600 text-white font-mono py-2 px-6 hover:border-white text-xs uppercase">
          BOOK ANOTHER EVENT
        </button>
      </div>
    </div>
  {/if}
</div>
