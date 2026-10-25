<script lang="ts">
	import { onMount } from 'svelte';
	import { playClickSound, playCoinSound, playSuccessSound } from '$lib/audio';
	import bookingsHero from '$lib/assets/bookings_hero.jpg';

	// Multi-step form state
	let currentStep = $state(1);
	let errorMessage = $state('');

	// Step 1: Event & Venue Info
	let eventType = $state('Birthday Party');
	let bookingDate = $state('2026-11-14');
	let bookingTime = $state('18:00');
	let headcount = $state(12);
	let selectedArea = $state('Pinball Alley');

	// Step 2: Food Pre-Orders
	const foodItems = [
		{
			id: 'f1',
			name: 'Arcade Sliders Platter (20x)',
			price: 120,
			category: 'Platters',
			tag: 'Popular',
			qty: 0
		},
		{
			id: 'f2',
			name: 'Loaded Fries & Wings Combo',
			price: 90,
			category: 'Platters',
			tag: 'GF Option',
			qty: 0
		},
		{
			id: 'f3',
			name: 'Gourmet Woodfired Pizza Pack (5x)',
			price: 140,
			category: 'Pizza',
			tag: 'Vegan Option',
			qty: 0
		},
		{
			id: 'f4',
			name: 'Retro Taco Fiesta Tray',
			price: 110,
			category: 'Platters',
			tag: 'Halal Option',
			qty: 0
		}
	];
	let selectedFood = $state(foodItems);

	// Step 3: Drinks & Token Bundles
	let drinkBundle = $state('Classic Bar Tab (Beer, Wine, Spirits)');
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
	let foodTotal = $derived(selectedFood.reduce((sum, item) => sum + item.price * item.qty, 0));
	let depositAmount = $derived(headcount * 20); // $20 deposit per person
	let grandTotal = $derived(depositAmount + foodTotal + tokenAddonTier);

	function nextStep() {
		playClickSound();
		errorMessage = '';
		if (currentStep < 4) currentStep += 1;
	}

	function prevStep() {
		playClickSound();
		errorMessage = '';
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
		errorMessage = '';
		if (!contactName.trim() || !contactEmail.trim() || !contactPhone.trim()) {
			errorMessage = 'Please complete your contact name, email, and phone number.';
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
					foodPreOrders: selectedFood
						.filter((i) => i.qty > 0)
						.map((i) => ({ id: i.id, name: i.name, price: i.price, quantity: i.qty })),
					drinkBundle,
					tokenAddonAmount: tokenAddonTier,
					contactName: contactName.trim(),
					contactEmail: contactEmail.trim(),
					contactPhone: contactPhone.trim()
				})
			});

			const data = (await res.json()) as Record<string, any>;
			if (res.ok && data.success) {
				playSuccessSound();
				if (data.checkoutUrl) {
					window.location.href = data.checkoutUrl;
					return;
				}
				bookingResult = data as any;
			} else {
				errorMessage = data.error || 'Failed to lock booking.';
			}
		} catch {
			errorMessage = 'Error connecting to booking service.';
		} finally {
			isSubmitting = false;
		}
	}

	onMount(() => {
		const params = new URLSearchParams(window.location.search);
		const refParam = params.get('ref');
		if (refParam) {
			fetch(`/api/checkout/booking?ref=${encodeURIComponent(refParam)}`)
				.then((r) => (r.ok ? r.json() : null))
				.then((data: any) => {
					if (data?.success && data.booking) {
						const b = data.booking;
						bookingResult = {
							bookingRef: b.booking_ref,
							depositAmount: b.deposit_amount,
							foodTotal: b.food_total,
							tokenAddonAmount: b.token_addon_amount,
							totalPaid: b.total_amount,
							summary: {
								eventType: b.event_type,
								date: b.booking_date,
								time: b.booking_time,
								headcount: b.headcount,
								area: b.area,
								contactEmail: b.contact_email
							},
							calendarEventUrl: `/api/checkout/booking?ref=${encodeURIComponent(b.booking_ref)}&format=ics`
						};
					}
				})
				.catch(() => {});
		}
	});
</script>

<svelte:head>
	<title>FUNCTION & EVENT BOOKINGS | THE BK</title>
</svelte:head>

<div class="mx-auto max-w-4xl px-4 py-12">
	<!-- Hero Image Banner -->
	<div class="relative mb-10 h-52 overflow-hidden rounded-sm md:h-72">
		<img
			src={bookingsHero}
			alt="Function & Event Bookings at Beercade"
			class="vhs-video h-full w-full object-cover object-center"
		/>
		<div
			class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/40 to-transparent"
		></div>
		<div class="absolute inset-0 flex flex-col justify-end p-8">
			<h1
				class="glitch-text glow-neon-cyan font-mono text-4xl font-bold tracking-widest text-neon-cyan uppercase md:text-6xl"
				data-text="FUNCTION BOOKINGS"
			>
				FUNCTION BOOKINGS
			</h1>
			<p class="mt-2 font-sans text-crema/80">
				Host your birthday, hen's night, corporate event or squad session at The BK Redfern.
			</p>
		</div>
	</div>

	{#if !bookingResult}
		<!-- Step Progress Bar -->
		<div
			class="mb-10 flex items-center justify-between border-b border-gray-800 pb-4 font-mono text-xs md:text-sm"
		>
			<div class={currentStep >= 1 ? 'font-bold text-neon-cyan' : 'text-gray-600'}>
				1. EVENT DETAILS
			</div>
			<div class={currentStep >= 2 ? 'font-bold text-neon-magenta' : 'text-gray-600'}>
				2. FOOD PACKAGES
			</div>
			<div class={currentStep >= 3 ? 'font-bold text-neon-green' : 'text-gray-600'}>
				3. DRINKS & TOKENS
			</div>
			<div class={currentStep >= 4 ? 'font-bold text-white' : 'text-gray-600'}>
				4. DEPOSIT & LOCK
			</div>
		</div>

		{#if errorMessage}
			<div
				class="mb-6 border border-neon-magenta bg-neon-magenta/10 p-4 font-mono text-xs text-neon-magenta"
			>
				{errorMessage}
			</div>
		{/if}

		<!-- Step 1: Event Details -->
		{#if currentStep === 1}
			<div class="space-y-6 rounded-sm border border-neon-cyan/40 bg-gray-950 p-8">
				<h2 class="font-mono text-xl tracking-wider text-neon-cyan uppercase">
					STEP 1: SELECT EVENT & VENUE AREA
				</h2>

				<div class="grid grid-cols-1 gap-6 md:grid-cols-2">
					<div>
						<label for="event-type" class="mb-2 block font-mono text-xs text-gray-300 uppercase">
							EVENT TYPE
						</label>
						<select
							id="event-type"
							bind:value={eventType}
							class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-cyan"
						>
							<option>Birthday Party</option>
							<option>Hen's / Buck's Party</option>
							<option>Corporate Event</option>
							<option>Casual Squad Reservation</option>
						</select>
					</div>

					<div>
						<label for="headcount" class="mb-2 block font-mono text-xs text-gray-300 uppercase">
							HEADCOUNT (GUESTS)
						</label>
						<input
							id="headcount"
							type="number"
							min="4"
							max="100"
							bind:value={headcount}
							class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-cyan"
						/>
					</div>

					<div>
						<label for="booking-date" class="mb-2 block font-mono text-xs text-gray-300 uppercase">
							RESERVATION DATE
						</label>
						<input
							id="booking-date"
							type="date"
							bind:value={bookingDate}
							class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-cyan"
						/>
					</div>

					<div>
						<label for="booking-time" class="mb-2 block font-mono text-xs text-gray-300 uppercase">
							START TIME
						</label>
						<input
							id="booking-time"
							type="time"
							bind:value={bookingTime}
							class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-cyan"
						/>
					</div>
				</div>

				<div>
					<span class="mb-2 block font-mono text-xs text-gray-300 uppercase">PREFERRED AREA</span>
					<div class="grid grid-cols-1 gap-4 md:grid-cols-3">
						{#each ['Main Bar', 'Pinball Alley', 'ASTRO Lounge'] as area (area)}
							<button
								type="button"
								onclick={() => (selectedArea = area)}
								class="cursor-pointer border-2 p-4 text-center font-mono text-sm uppercase transition-all {selectedArea ===
								area
									? 'border-neon-cyan bg-neon-cyan/10 text-neon-cyan'
									: 'border-gray-800 bg-black text-gray-400 hover:border-gray-600'}"
							>
								{area}
							</button>
						{/each}
					</div>
				</div>

				<div class="flex justify-end pt-4">
					<button
						type="button"
						onclick={nextStep}
						class="bg-neon-cyan px-8 py-3 font-mono font-bold tracking-wider text-black uppercase transition-colors hover:bg-white"
					>
						NEXT: FOOD SELECTION &rarr;
					</button>
				</div>
			</div>
		{/if}

		<!-- Step 2: Food Pre-Orders -->
		{#if currentStep === 2}
			<div class="space-y-6 rounded-sm border border-neon-magenta/40 bg-gray-950 p-8">
				<h2 class="font-mono text-xl tracking-wider text-neon-magenta uppercase">
					STEP 2: GROUP EXCLUSIVE FOOD PRE-ORDERS
				</h2>

				<div class="space-y-4">
					{#each selectedFood as item, idx (item.id)}
						<div
							class="flex items-center justify-between rounded-sm border border-gray-800 bg-black p-4"
						>
							<div>
								<div class="font-mono font-bold text-white">{item.name}</div>
								<div class="font-mono text-xs text-neon-magenta">
									${item.price} • <span class="text-gray-400">{item.tag}</span>
								</div>
							</div>

							<div class="flex items-center gap-3 font-mono">
								<button
									type="button"
									onclick={() => updateFoodQty(idx, -1)}
									class="h-8 w-8 bg-gray-800 font-bold text-white hover:bg-neon-magenta hover:text-black"
								>
									-
								</button>
								<span class="w-8 text-center text-lg">{item.qty}</span>
								<button
									type="button"
									onclick={() => updateFoodQty(idx, 1)}
									class="h-8 w-8 bg-gray-800 font-bold text-white hover:bg-neon-green hover:text-black"
								>
									+
								</button>
							</div>
						</div>
					{/each}
				</div>

				<div class="flex items-center justify-between pt-4">
					<button
						type="button"
						onclick={prevStep}
						class="border border-gray-700 px-6 py-3 font-mono text-xs text-gray-300 uppercase hover:border-white"
					>
						&larr; BACK
					</button>
					<button
						type="button"
						onclick={nextStep}
						class="bg-neon-magenta px-8 py-3 font-mono font-bold tracking-wider text-black uppercase transition-colors hover:bg-white"
					>
						NEXT: DRINKS & TOKENS &rarr;
					</button>
				</div>
			</div>
		{/if}

		<!-- Step 3: Drinks & Token Bundles -->
		{#if currentStep === 3}
			<div class="space-y-6 rounded-sm border border-neon-green/40 bg-gray-950 p-8">
				<h2 class="font-mono text-xl tracking-wider text-neon-green uppercase">
					STEP 3: DRINK TABS & DISCOUNTED TOKEN PACKS
				</h2>

				<div>
					<label for="drink-bundle" class="mb-2 block font-mono text-xs text-gray-300 uppercase">
						BAR TAB / DRINK PACKAGE
					</label>
					<select
						id="drink-bundle"
						bind:value={drinkBundle}
						class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-green"
					>
						<option>Pay As You Go (Individual Tabs)</option>
						<option>Classic Bar Tab (Beer, Wine, Spirits)</option>
						<option>VIP Cocktail & Craft Beer Experience</option>
					</select>
				</div>

				<div>
					<span class="mb-2 block font-mono text-xs text-gray-300 uppercase">
						ATTACH DISCOUNTED TOKEN PACK ADD-ON
					</span>
					<div class="grid grid-cols-2 gap-4 md:grid-cols-4">
						{#each [0, 50, 100, 200] as amt (amt)}
							<button
								type="button"
								onclick={() => {
									playCoinSound();
									tokenAddonTier = amt;
								}}
								class="cursor-pointer border-2 p-4 text-center font-mono text-sm uppercase transition-all {tokenAddonTier ===
								amt
									? 'border-neon-green bg-neon-green/10 text-neon-green'
									: 'border-gray-800 bg-black text-gray-400 hover:border-gray-600'}"
							>
								{amt === 0 ? 'NO TOKENS' : `$${amt} PACK`}
							</button>
						{/each}
					</div>
				</div>

				<div class="flex items-center justify-between pt-4">
					<button
						type="button"
						onclick={prevStep}
						class="border border-gray-700 px-6 py-3 font-mono text-xs text-gray-300 uppercase hover:border-white"
					>
						&larr; BACK
					</button>
					<button
						type="button"
						onclick={nextStep}
						class="bg-neon-green px-8 py-3 font-mono font-bold tracking-wider text-black uppercase transition-colors hover:bg-white"
					>
						NEXT: CONFIRM DEPOSIT &rarr;
					</button>
				</div>
			</div>
		{/if}

		<!-- Step 4: Deposit Checkout -->
		{#if currentStep === 4}
			<div class="space-y-6 rounded-sm border-2 border-neon-cyan/50 bg-gray-950 p-8">
				<h2 class="font-mono text-xl tracking-wider text-neon-cyan uppercase">
					STEP 4: CONTACT INFO & STRIPE DEPOSIT
				</h2>

				<!-- Order Summary Box -->
				<div class="space-y-2 border border-gray-800 bg-black p-4 font-mono text-sm">
					<div class="flex justify-between">
						<span>{eventType} ({headcount} guests @ {selectedArea})</span>
						<span>${depositAmount} deposit</span>
					</div>
					{#if foodTotal > 0}
						<div class="flex justify-between text-neon-magenta">
							<span>Food Pre-Orders Total</span>
							<span>${foodTotal}</span>
						</div>
					{/if}
					{#if tokenAddonTier > 0}
						<div class="flex justify-between text-neon-green">
							<span>Token Add-on Pack</span>
							<span>${tokenAddonTier}</span>
						</div>
					{/if}
					<div class="flex justify-between border-t border-gray-700 pt-2 text-lg font-bold text-white">
						<span>TOTAL DUE NOW</span>
						<span class="text-neon-green">${grandTotal}</span>
					</div>
				</div>

				<div class="space-y-4">
					<div>
						<label for="contact-name" class="mb-1 block font-mono text-xs text-gray-300 uppercase">
							CONTACT NAME
						</label>
						<input
							id="contact-name"
							type="text"
							bind:value={contactName}
							placeholder="Alex Smith"
							class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-cyan"
						/>
					</div>

					<div class="grid grid-cols-1 gap-4 md:grid-cols-2">
						<div>
							<label
								for="contact-email"
								class="mb-1 block font-mono text-xs text-gray-300 uppercase"
							>
								EMAIL ADDRESS
							</label>
							<input
								id="contact-email"
								type="email"
								bind:value={contactEmail}
								placeholder="alex@example.com"
								class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-cyan"
							/>
						</div>

						<div>
							<label
								for="contact-phone"
								class="mb-1 block font-mono text-xs text-gray-300 uppercase"
							>
								PHONE NUMBER
							</label>
							<input
								id="contact-phone"
								type="tel"
								bind:value={contactPhone}
								placeholder="0400 000 000"
								class="w-full border border-gray-700 bg-black p-3 font-mono text-white outline-none focus:border-neon-cyan"
							/>
						</div>
					</div>
				</div>

				<div class="flex items-center justify-between pt-4">
					<button
						type="button"
						onclick={prevStep}
						class="border border-gray-700 px-6 py-3 font-mono text-xs text-gray-300 uppercase hover:border-white"
					>
						&larr; BACK
					</button>
					<button
						type="button"
						onclick={submitBooking}
						disabled={isSubmitting}
						class="cursor-pointer bg-neon-green px-8 py-4 font-mono text-lg font-bold tracking-wider text-black uppercase shadow-[0_0_15px_var(--color-neon-green)] transition-all hover:bg-white"
					>
						{isSubmitting ? 'LOCKING RESERVATION...' : `PAY DEPOSIT ($${grandTotal}) & LOCK`}
					</button>
				</div>
			</div>
		{/if}
	{:else}
		<!-- Booking Confirmation Card -->
		<div
			class="mx-auto max-w-xl rounded-sm border-2 border-neon-cyan bg-gray-950 p-8 text-center shadow-[0_0_30px_rgba(0,255,255,0.3)]"
		>
			<div class="mb-2 font-mono text-xs tracking-widest text-neon-cyan uppercase">
				★ RESERVATION LOCKED ★
			</div>
			<h2 class="mb-2 font-mono text-3xl text-white">{bookingResult.summary.eventType}</h2>
			<div class="mb-6 font-mono text-xl font-bold text-neon-green">
				REF: {bookingResult.bookingRef}
			</div>

			<div class="mb-6 space-y-2 border border-gray-800 bg-black p-6 text-left font-mono text-sm">
				<div>
					<span class="text-gray-400">Date & Time:</span>
					{bookingResult.summary.date} @ {bookingResult.summary.time}
				</div>
				<div>
					<span class="text-gray-400">Guests & Area:</span>
					{bookingResult.summary.headcount} Guests • {bookingResult.summary.area}
				</div>
				<div>
					<span class="text-gray-400">Deposit Paid:</span>
					${bookingResult.totalPaid}
				</div>
			</div>

			<p class="mb-6 font-mono text-xs text-gray-400">
				Confirmation & D1 ledger receipt recorded for
				<span class="text-white">{bookingResult.summary.contactEmail}</span>.
			</p>

			<div class="flex justify-center gap-4">
				<a
					href={bookingResult.calendarEventUrl}
					download="{bookingResult.bookingRef}.ics"
					class="border border-neon-cyan bg-black px-6 py-2 font-mono text-xs text-neon-cyan uppercase transition-colors hover:bg-neon-cyan hover:text-black"
				>
					📅 DOWNLOAD .ICS CALENDAR EVENT
				</a>
				<button
					type="button"
					onclick={() => (bookingResult = null)}
					class="border border-gray-600 bg-transparent px-6 py-2 font-mono text-xs text-white uppercase hover:border-white"
				>
					BOOK ANOTHER EVENT
				</button>
			</div>
		</div>
	{/if}
</div>
