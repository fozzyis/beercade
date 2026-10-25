<script lang="ts">
	import cocktailImg from '$lib/assets/cocktail.jpg';
	import { playClickSound } from '$lib/audio';
	import menuDrinksHero from '$lib/assets/menu_drinks_hero.jpg';
	import menuFoodHero from '$lib/assets/menu_food_hero.jpg';

	let activeSection = $state<'ALL' | 'COCKTAILS' | 'TAPS' | 'BITES'>('ALL');

	const cocktails = [
		{
			name: 'Negroni 8-Bit',
			price: '$21',
			desc: 'Tanqueray gin, Campari, Sweet Vermouth, orange twist',
			tag: 'SIGNATURE'
		},
		{
			name: 'Arcade Spritz',
			price: '$19',
			desc: 'Aperol, Prosecco, soda, fresh grapefruit',
			tag: 'POPULAR'
		},
		{
			name: 'Smoked Old Fashioned',
			price: '$23',
			desc: 'Bourbon, Angostura bitters, flamed orange, cherry',
			tag: 'FAVOURITE'
		},
		{
			name: 'Kazi Sour',
			price: '$20',
			desc: 'Japanese whisky, yuzu juice, egg white, neon sugar rim',
			tag: 'NEW'
		},
		{
			name: 'Pixel Passionfruit Mojito',
			price: '$20',
			desc: 'White rum, fresh passionfruit, mint, lime, soda',
			tag: 'REFRESHING'
		},
		{
			name: 'Cyberpunk Espresso Martini',
			price: '$22',
			desc: 'Vodka, Mr Black coffee liqueur, fresh espresso, vanilla',
			tag: 'HIGH ENERGY'
		}
	];

	const craftTaps = [
		{
			name: 'Regent St Lager Jug',
			price: '$26',
			desc: 'Crisp local craft lager on tap (4.8% ABV)',
			tag: 'CRAFT TAP'
		},
		{
			name: 'Pacific Ale Jug',
			price: '$28',
			desc: 'Hoppy tropical pale ale (4.4% ABV)',
			tag: 'CRAFT TAP'
		},
		{
			name: 'Redfern Hazy IPA Pint',
			price: '$14',
			desc: 'Juicy citrus & mango notes (6.2% ABV)',
			tag: 'CRAFT TAP'
		},
		{
			name: 'Japanese Rice Lager Pint',
			price: '$13',
			desc: 'Ultra crisp & dry finish (5.0% ABV)',
			tag: 'CRAFT TAP'
		}
	];

	const bites = [
		{
			name: 'Cheeseburger Sliders (3x)',
			price: '$18',
			desc: 'Angus beef, American cheese, BK secret sauce, pickles',
			tag: 'BEST SELLER'
		},
		{
			name: 'Loaded Fries',
			price: '$15',
			desc: 'Crispy fries, liquid cheese, bacon bits, jalapeños, green onion',
			tag: 'GF OPTION'
		},
		{
			name: 'Woodfired Pepperoni Pizza',
			price: '$24',
			desc: 'Hot pepperoni, mozzarella, hot honey drizzled crust',
			tag: 'PIZZA'
		},
		{
			name: 'Vegan Margherita Pizza',
			price: '$22',
			desc: 'Plant-based mozzarella, fresh basil, San Marzano tomato',
			tag: 'VEGAN'
		},
		{
			name: 'Buffalo Wings (10x)',
			price: '$19',
			desc: 'Spicy buffalo sauce, ranch dip, celery sticks',
			tag: 'SPICY / HALAL'
		},
		{
			name: 'Taco Fiesta Platter',
			price: '$26',
			desc: 'Slow cooked pulled pork & jackfruit tacos, fresh salsa',
			tag: 'PLATTER'
		}
	];

	function setSection(sec: typeof activeSection) {
		playClickSound();
		activeSection = sec;
	}
</script>

<svelte:head>
	<title>COCKTAILS & FOOD MENU | THE BK</title>
</svelte:head>

<div class="mx-auto max-w-6xl px-4 py-12">
	<!-- Hero Section: Two cyberpunk images side by side -->
	<div class="mb-10 grid grid-cols-1 gap-4 md:grid-cols-2">
		<!-- Drinks Hero -->
		<div class="relative h-56 overflow-hidden rounded-sm md:h-72">
			<img
				src={menuDrinksHero}
				alt="Signature Cocktails &amp; Craft Beer"
				class="vhs-video h-full w-full object-cover object-center"
			/>
			<div
				class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/30 to-transparent"
			></div>
			<div class="absolute inset-0 flex flex-col justify-end p-6">
				<h1
					class="glitch-text glow-neon-magenta font-mono text-3xl font-bold tracking-widest text-neon-magenta uppercase md:text-4xl"
					data-text="DRINKS &amp; FOOD MENU"
				>
					DRINKS &amp; FOOD MENU
				</h1>
				<p class="mt-1 font-sans text-sm text-crema/80">
					Craft cocktails, cold craft beer, woodfired pizzas &amp; arcade bites.
				</p>
			</div>
		</div>
		<!-- Food Hero -->
		<div class="relative h-56 overflow-hidden rounded-sm md:h-72">
			<img
				src={menuFoodHero}
				alt="Arcade Bites &amp; Woodfired Pizza"
				class="vhs-video h-full w-full object-cover object-center"
			/>
			<div
				class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/30 to-transparent"
			></div>
			<div class="absolute inset-0 flex flex-col justify-end p-6">
				<div class="glow-neon-green mb-1 font-mono text-xs tracking-widest text-neon-green uppercase">
					OPEN 4PM — 2AM NIGHTLY
				</div>
				<div class="glow-neon-orange font-mono text-2xl font-bold text-high-score-orange">
					🍕 ARCADE BITES
				</div>
				<p class="mt-1 font-sans text-sm text-crema/70">
					Sliders, loaded fries, woodfired pizzas, buffalo wings &amp; more.
				</p>
			</div>
		</div>
	</div>

	<!-- Interactive Menu Filter Tabs -->
	<div class="mb-10 flex flex-wrap justify-center gap-3 font-mono text-xs">
		{#each [
			['ALL', 'ALL MENU ITEMS'],
			['COCKTAILS', '🍸 SIGNATURE COCKTAILS'],
			['TAPS', '🍺 CRAFT TAPS'],
			['BITES', '🍕 ARCADE BITES & PIZZAS']
		] as [secKey, secLabel] (secKey)}
			<button
				type="button"
				onclick={() => setSection(secKey as any)}
				class="cursor-pointer border px-5 py-2.5 uppercase transition-all {activeSection === secKey
					? 'border-neon-cyan bg-neon-cyan font-bold text-black'
					: 'border-gray-800 bg-gray-950 text-crema/70 hover:border-gray-600'}"
			>
				{secLabel}
			</button>
		{/each}
	</div>

	<!-- Featured Cocktail Card Banner -->
	<div
		class="box-glow-cyan mb-12 flex flex-col items-center gap-8 rounded-sm border-2 border-neon-cyan/60 bg-gray-950 p-6 md:flex-row"
	>
		<img
			src={cocktailImg}
			alt="Signature Arcade Cocktail"
			class="h-64 w-full rounded-sm border-2 border-neon-cyan object-cover md:w-64"
		/>
		<div class="flex-1">
			<span
				class="mb-3 inline-block bg-neon-cyan px-3 py-1 font-mono text-xs font-bold text-black uppercase"
			>
				FEATURED COCKTAIL OF THE MONTH
			</span>
			<h2 class="mb-2 font-mono text-3xl text-white">NEGRONI 8-BIT</h2>
			<p class="mb-4 font-sans text-gray-300">
				Handcrafted Tanqueray Gin paired with Campari and Sweet Vermouth, served over custom glowing
				ice cubes with a burnt orange twist.
			</p>
			<div class="flex flex-wrap items-center justify-between gap-4">
				<div class="glow-neon-green font-mono text-2xl font-bold text-neon-green">$21</div>
				<a
					href="/bookings"
					class="border border-neon-magenta bg-neon-magenta/10 px-4 py-2 font-mono text-xs font-bold text-neon-magenta uppercase transition-colors hover:bg-neon-magenta hover:text-black"
				>
					PRE-ORDER PLATTERS FOR GROUP BOOKING &rarr;
				</a>
			</div>
		</div>
	</div>

	<div class="mb-12 grid grid-cols-1 gap-12 md:grid-cols-2">
		<!-- Drinks Section -->
		{#if activeSection === 'ALL' || activeSection === 'COCKTAILS'}
			<div class="rounded-sm border border-neon-cyan/40 bg-gray-950 p-8">
				<h2
					class="mb-6 flex items-center justify-between border-b border-neon-cyan/20 pb-3 font-mono text-2xl tracking-wider text-neon-cyan uppercase"
				>
					<span>🍸 SIGNATURE COCKTAILS</span>
				</h2>

				<div class="space-y-6">
					{#each cocktails as item (item.name)}
						<div>
							<div class="mb-1 flex items-center justify-between">
								<span class="font-mono text-lg font-bold text-white">{item.name}</span>
								<span class="font-mono text-lg font-bold text-neon-green">{item.price}</span>
							</div>
							<p class="mb-1 font-sans text-xs text-gray-400">{item.desc}</p>
							<span
								class="inline-block border border-neon-cyan bg-black px-2 py-0.5 font-mono text-[10px] text-neon-cyan uppercase"
							>
								{item.tag}
							</span>
						</div>
					{/each}
				</div>
			</div>
		{/if}

		<!-- Craft Taps & Food Section -->
		<div class="space-y-12">
			{#if activeSection === 'ALL' || activeSection === 'TAPS'}
				<div class="rounded-sm border border-neon-green/40 bg-gray-950 p-8">
					<h2
						class="mb-6 flex items-center justify-between border-b border-neon-green/20 pb-3 font-mono text-2xl tracking-wider text-neon-green uppercase"
					>
						<span>🍺 CRAFT BEER ON TAP</span>
					</h2>

					<div class="space-y-6">
						{#each craftTaps as item (item.name)}
							<div>
								<div class="mb-1 flex items-center justify-between">
									<span class="font-mono text-lg font-bold text-white">{item.name}</span>
									<span class="font-mono text-lg font-bold text-neon-green">{item.price}</span>
								</div>
								<p class="mb-1 font-sans text-xs text-gray-400">{item.desc}</p>
								<span
									class="inline-block border border-neon-green bg-black px-2 py-0.5 font-mono text-[10px] text-neon-green uppercase"
								>
									{item.tag}
								</span>
							</div>
						{/each}
					</div>
				</div>
			{/if}

			{#if activeSection === 'ALL' || activeSection === 'BITES'}
				<div class="rounded-sm border border-neon-magenta/40 bg-gray-950 p-8">
					<h2
						class="mb-6 flex items-center justify-between border-b border-neon-magenta/20 pb-3 font-mono text-2xl tracking-wider text-neon-magenta uppercase"
					>
						<span>🍕 ARCADE BITES & PIZZAS</span>
					</h2>

					<div class="space-y-6">
						{#each bites as item (item.name)}
							<div>
								<div class="mb-1 flex items-center justify-between">
									<span class="font-mono text-lg font-bold text-white">{item.name}</span>
									<span class="font-mono text-lg font-bold text-neon-green">{item.price}</span>
								</div>
								<p class="mb-1 font-sans text-xs text-gray-400">{item.desc}</p>
								<span
									class="inline-block border border-neon-magenta bg-black px-2 py-0.5 font-mono text-[10px] text-neon-magenta uppercase"
								>
									{item.tag}
								</span>
							</div>
						{/each}
					</div>
				</div>
			{/if}
		</div>
	</div>
</div>
