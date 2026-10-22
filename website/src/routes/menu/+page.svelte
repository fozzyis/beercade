<script lang="ts">
  import cocktailImg from '$lib/assets/cocktail.jpg';
  import { playClickSound } from '$lib/audio';
  import menuDrinksHero from '$lib/assets/menu_drinks_hero.jpg';
  import menuFoodHero from '$lib/assets/menu_food_hero.jpg';

  const cocktails = [
    { name: 'Negroni 8-Bit', price: '$21', desc: 'Tanqueray gin, Campari, Sweet Vermouth, orange twist', tag: 'SIGNATURE' },
    { name: 'Arcade Spritz', price: '$19', desc: 'Aperol, Prosecco, soda, fresh grapefruit', tag: 'POPULAR' },
    { name: 'Smoked Old Fashioned', price: '$23', desc: 'Bourbon, Angostura bitters, flamed orange, cherry', tag: 'FAVOURITE' },
    { name: 'Kazi Sour', price: '$20', desc: 'Japanese whisky, yuzu juice, egg white, neon sugar rim', tag: 'NEW' },
    { name: 'Pixel Passionfruit Mojito', price: '$20', desc: 'White rum, fresh passionfruit, mint, lime, soda', tag: 'REFRESHING' },
    { name: 'Cyberpunk Espresso Martini', price: '$22', desc: 'Vodka, Mr Black coffee liqueur, fresh espresso, vanilla', tag: 'HIGH ENERGY' }
  ];

  const craftTaps = [
    { name: 'Regent St Lager Jug', price: '$26', desc: 'Crisp local craft lager on tap (4.8% ABV)', tag: 'CRAFT TAP' },
    { name: 'Pacific Ale Jug', price: '$28', desc: 'Hoppy tropical pale ale (4.4% ABV)', tag: 'CRAFT TAP' },
    { name: 'Redfern Hazy IPA Pint', price: '$14', desc: 'Juicy citrus & mango notes (6.2% ABV)', tag: 'CRAFT TAP' },
    { name: 'Japanese Rice Lager Pint', price: '$13', desc: 'Ultra crisp & dry finish (5.0% ABV)', tag: 'CRAFT TAP' }
  ];

  const bites = [
    { name: 'Cheeseburger Sliders (3x)', price: '$18', desc: 'Angus beef, American cheese, BK secret sauce, pickles', tag: 'BEST SELLER' },
    { name: 'Loaded Fries', price: '$15', desc: 'Crispy fries, liquid cheese, bacon bits, jalapeños, green onion', tag: 'GF OPTION' },
    { name: 'Woodfired Pepperoni Pizza', price: '$24', desc: 'Hot pepperoni, mozzarella, hot honey drizzled crust', tag: 'PIZZA' },
    { name: 'Vegan Margherita Pizza', price: '$22', desc: 'Plant-based mozzarella, fresh basil, San Marzano tomato', tag: 'VEGAN' },
    { name: 'Buffalo Wings (10x)', price: '$19', desc: 'Spicy buffalo sauce, ranch dip, celery sticks', tag: 'SPICY / HALAL' },
    { name: 'Taco Fiesta Platter', price: '$26', desc: 'Slow cooked pulled pork & jackfruit tacos, fresh salsa', tag: 'PLATTER' }
  ];
</script>

<svelte:head>
  <title>COCKTAILS & FOOD MENU | THE BK</title>
</svelte:head>

<div class="max-w-6xl mx-auto py-12 px-4">
  <!-- Hero Section: Two cyberpunk images side by side -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mb-12">
    <!-- Drinks Hero -->
    <div class="relative rounded-sm overflow-hidden h-56 md:h-72">
      <img src={menuDrinksHero} alt="Signature Cocktails &amp; Craft Beer" class="w-full h-full object-cover object-center vhs-video" />
      <div class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/30 to-transparent"></div>
      <div class="absolute inset-0 flex flex-col justify-end p-6">
        <h1 class="glitch-text text-3xl md:text-4xl font-mono text-neon-magenta uppercase tracking-widest glow-neon-magenta font-bold" data-text="DRINKS &amp; FOOD MENU">
          DRINKS &amp; FOOD MENU
        </h1>
        <p class="text-crema/80 font-sans text-sm mt-1">Craft cocktails, cold craft beer, woodfired pizzas &amp; arcade bites.</p>
      </div>
    </div>
    <!-- Food Hero -->
    <div class="relative rounded-sm overflow-hidden h-56 md:h-72">
      <img src={menuFoodHero} alt="Arcade Bites &amp; Woodfired Pizza" class="w-full h-full object-cover object-center vhs-video" />
      <div class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/30 to-transparent"></div>
      <div class="absolute inset-0 flex flex-col justify-end p-6">
        <div class="font-mono text-xs text-neon-green glow-neon-green mb-1 uppercase tracking-widest">OPEN 4PM — 2AM NIGHTLY</div>
        <div class="text-2xl font-mono text-high-score-orange glow-neon-orange font-bold">🍕 ARCADE BITES</div>
        <p class="text-crema/70 font-sans text-sm mt-1">Sliders, loaded fries, woodfired pizzas, buffalo wings &amp; more.</p>
      </div>
    </div>
  </div>

  <!-- Featured Cocktail Card Banner -->
  <div class="bg-gray-950 border-2 border-neon-cyan/60 p-6 rounded-sm mb-12 flex flex-col md:flex-row items-center gap-8 box-glow-cyan">
    <img src={cocktailImg} alt="Signature Arcade Cocktail" class="w-full md:w-64 h-64 object-cover border-2 border-neon-cyan rounded-sm" />
    <div>
      <span class="bg-neon-cyan text-black font-mono text-xs px-3 py-1 uppercase font-bold mb-3 inline-block">
        FEATURED COCKTAIL OF THE MONTH
      </span>
      <h2 class="text-3xl font-mono text-white mb-2">NEGRONI 8-BIT</h2>
      <p class="text-gray-300 font-sans mb-4">
        Handcrafted Tanqueray Gin paired with Campari and Sweet Vermouth, served over custom glowing ice cubes with a burnt orange twist.
      </p>
      <div class="text-2xl font-mono text-neon-green font-bold glow-neon-green">$21</div>
    </div>
  </div>

  <div class="grid grid-cols-1 md:grid-cols-2 gap-12 mb-12">
    <!-- Drinks Section -->
    <div class="bg-gray-950 border border-neon-cyan/40 p-8 rounded-sm">
      <h2 class="text-2xl font-mono text-neon-cyan mb-6 uppercase tracking-wider border-b border-neon-cyan/20 pb-3 flex justify-between items-center">
        <span>🍸 SIGNATURE COCKTAILS</span>
      </h2>

      <div class="space-y-6">
        {#each cocktails as item}
          <div>
            <div class="flex justify-between items-center mb-1">
              <span class="font-mono text-white font-bold text-lg">{item.name}</span>
              <span class="font-mono text-neon-green font-bold text-lg">{item.price}</span>
            </div>
            <p class="text-xs text-gray-400 font-sans mb-1">{item.desc}</p>
            <span class="inline-block bg-black border border-neon-cyan text-neon-cyan text-[10px] font-mono px-2 py-0.5 uppercase">
              {item.tag}
            </span>
          </div>
        {/each}
      </div>
    </div>

    <!-- Craft Taps & Food Section -->
    <div class="space-y-12">
      <!-- Craft Taps -->
      <div class="bg-gray-950 border border-neon-green/40 p-8 rounded-sm">
        <h2 class="text-2xl font-mono text-neon-green mb-6 uppercase tracking-wider border-b border-neon-green/20 pb-3 flex justify-between items-center">
          <span>🍺 CRAFT BEER ON TAP</span>
        </h2>

        <div class="space-y-6">
          {#each craftTaps as item}
            <div>
              <div class="flex justify-between items-center mb-1">
                <span class="font-mono text-white font-bold text-lg">{item.name}</span>
                <span class="font-mono text-neon-green font-bold text-lg">{item.price}</span>
              </div>
              <p class="text-xs text-gray-400 font-sans mb-1">{item.desc}</p>
              <span class="inline-block bg-black border border-neon-green text-neon-green text-[10px] font-mono px-2 py-0.5 uppercase">
                {item.tag}
              </span>
            </div>
          {/each}
        </div>
      </div>

      <!-- Food Section -->
      <div class="bg-gray-950 border border-neon-magenta/40 p-8 rounded-sm">
        <h2 class="text-2xl font-mono text-neon-magenta mb-6 uppercase tracking-wider border-b border-neon-magenta/20 pb-3 flex justify-between items-center">
          <span>🍕 ARCADE BITES & PIZZAS</span>
        </h2>

        <div class="space-y-6">
          {#each bites as item}
            <div>
              <div class="flex justify-between items-center mb-1">
                <span class="font-mono text-white font-bold text-lg">{item.name}</span>
                <span class="font-mono text-neon-green font-bold text-lg">{item.price}</span>
              </div>
              <p class="text-xs text-gray-400 font-sans mb-1">{item.desc}</p>
              <span class="inline-block bg-black border border-neon-magenta text-neon-magenta text-[10px] font-mono px-2 py-0.5 uppercase">
                {item.tag}
              </span>
            </div>
          {/each}
        </div>
      </div>
    </div>
  </div>
</div>
