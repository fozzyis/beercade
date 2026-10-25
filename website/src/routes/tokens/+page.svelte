<script lang="ts">
	import { onMount } from 'svelte';
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

	const gameTimePasses = [
		{
			id: 'astro-30m',
			label: '30-MIN A.S.T.R.O. SESSION',
			durationMins: 30,
			amount: 12,
			tokens: 10,
			badge: 'QUICK PLAY'
		},
		{
			id: 'astro-60m',
			label: '60-MIN UNLIMITED A.S.T.R.O. PASS',
			durationMins: 60,
			amount: 20,
			tokens: 25,
			badge: 'MOST POPULAR'
		},
		{
			id: 'astro-120m',
			label: '2-HOUR A.S.T.R.O. MARATHON PASS',
			durationMins: 120,
			amount: 35,
			tokens: 60,
			badge: 'BEST VALUE'
		}
	];

	const cabinetTargets = [
		{ id: '', label: 'Any O.R.I.O.N. Cabinet / Redeem at B.O.S.S. Kiosk QR' },
		{ id: 'ASTRO-01', label: 'ASTRO-01 — O.R.I.O.N. Rock 5C (Street Fighter III / II)' },
		{ id: 'ASTRO-02', label: 'ASTRO-02 — O.R.I.O.N. Rock 5C (Tekken / 3D Fighters)' },
		{ id: 'ASTRO-03', label: 'ASTRO-03 — O.R.I.O.N. Orange Pi 5 Pro (Metal Slug / Neo-Geo)' },
		{ id: 'ASTRO-04', label: 'ASTRO-04 — O.R.I.O.N. Orange Pi 5 Pro (Marvel vs. Capcom 2)' }
	];

	let selectedPassType = $state<string | null>('astro-60m');
	let selectedTier = $state(tiers[3]); // Default $50
	let email = $state('');
	let targetNodeId = $state('ASTRO-01');
	let isProcessing = $state(false);
	let isRedeeming = $state(false);
	let errorMessage = $state('');
	let redeemMessage = $state('');
	let voucherResult = $state<{
		voucherId: string;
		email: string;
		amount: number;
		tokens: number;
		bonusTokens: number;
		durationMins?: number | null;
		passLabel?: string | null;
		paymentProvider?: string;
		bossSynced?: boolean;
		targetNodeId?: string | null;
		qrData: string;
		passUrl: string;
	} | null>(null);

	const activePassObj = $derived(
		selectedPassType ? gameTimePasses.find((p) => p.id === selectedPassType) ?? null : null
	);

	function selectGameTimePass(passId: string) {
		playCoinSound();
		selectedPassType = passId;
		errorMessage = '';
	}

	function selectTier(tier: (typeof tiers)[0]) {
		playCoinSound();
		selectedPassType = null;
		selectedTier = tier;
		errorMessage = '';
	}

	async function handleCheckout() {
		errorMessage = '';
		if (!email.trim() || !email.includes('@')) {
			errorMessage = 'Please enter a valid email address for instant Square QR pass delivery.';
			return;
		}
		playClickSound();
		isProcessing = true;

		try {
			const res = await fetch('/api/checkout/token-pack', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({
					passType: selectedPassType || undefined,
					amount: activePassObj ? activePassObj.amount : selectedTier.amount,
					email: email.trim(),
					targetNodeId: targetNodeId || undefined,
					provider: 'square'
				})
			});
			const data = (await res.json()) as Record<string, any>;
			if (res.ok && data.success) {
				playSuccessSound();
				if (data.checkoutUrl) {
					window.location.href = data.checkoutUrl;
					return;
				}
				voucherResult = data as any;
			} else {
				errorMessage = data.error || 'Checkout failed. Please try again.';
			}
		} catch {
			errorMessage = 'Failed to connect to Square POS checkout engine.';
		} finally {
			isProcessing = false;
		}
	}

	async function redeemToCabinet() {
		if (!voucherResult) return;
		const node = voucherResult.targetNodeId || targetNodeId || 'ASTRO-01';
		isRedeeming = true;
		redeemMessage = '';
		try {
			const res = await fetch('/api/tokens', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({
					voucher_id: voucherResult.voucherId,
					node_id: node,
					credits: Math.min(100, voucherResult.tokens)
				})
			});
			const data = (await res.json()) as Record<string, any>;
			if (res.ok && data.success) {
				playCoinSound();
				redeemMessage = `✓ Dispatched ${data.tokens_added} credits directly to ${node}!`;
			} else {
				redeemMessage = 'Could not dispatch credits automatically — scan QR at the kiosk.';
			}
		} catch {
			redeemMessage = 'Saved to voucher — scan QR code at the venue kiosk.';
		} finally {
			isRedeeming = false;
		}
	}

	function resetPurchase() {
		playClickSound();
		voucherResult = null;
		redeemMessage = '';
		errorMessage = '';
	}

	onMount(() => {
		const params = new URLSearchParams(window.location.search);
		const voucherParam = params.get('voucher');
		if (voucherParam) {
			fetch(`/api/checkout/token-pack?voucher=${encodeURIComponent(voucherParam)}`)
				.then((r) => (r.ok ? r.json() : null))
				.then((data: any) => {
					if (data && data.success) {
						voucherResult = data;
					}
				})
				.catch(() => {});
		}
	});
</script>

<svelte:head>
	<title>TOKEN PACK STORE | THE BK</title>
</svelte:head>

<div class="mx-auto max-w-5xl px-4 py-12">
	<!-- Hero Image Banner -->
	<div class="relative mb-12 h-52 overflow-hidden rounded-sm md:h-72">
		<img
			src={tokensHero}
			alt="Arcade Token Pack Store"
			class="vhs-video h-full w-full object-cover object-center"
		/>
		<div
			class="absolute inset-0 bg-gradient-to-t from-after-dark via-after-dark/40 to-transparent"
		></div>
		<div class="absolute inset-0 flex flex-col justify-end p-8">
			<h1
				class="glitch-text glow-neon-green font-mono text-4xl font-bold tracking-widest text-neon-green uppercase md:text-6xl"
				data-text="TOKEN PACK STORE"
			>
				TOKEN PACK STORE
			</h1>
			<p class="mt-2 font-sans text-lg text-crema/80">
				Buy arcade tokens online. Instantly receive an 8-bit scannable QR voucher or inject credits
				straight to an ASTRO cabinet.
			</p>
		</div>
	</div>

	{#if !voucherResult}
		<!-- Square Online A.S.T.R.O. Timed Game-Time Passes -->
		<div class="mb-10">
			<div class="mb-4 flex flex-wrap items-center justify-between gap-2">
				<div>
					<span class="font-mono text-xs font-bold tracking-widest text-neon-cyan uppercase">
						POWERED BY SQUARE POS · DIRECT B.O.S.S. CABINET UNLOCK
					</span>
					<h2 class="font-mono text-2xl font-bold text-white uppercase">
						1. BUY A.S.T.R.O. TIMED GAME-TIME PASSES
					</h2>
				</div>
				<span class="border border-neon-green/50 bg-neon-green/10 px-3 py-1 font-mono text-xs text-neon-green">
					AUTO-UNLOCKS SELECTED O.R.I.O.N. CABINET
				</span>
			</div>

			<div class="grid grid-cols-1 gap-4 md:grid-cols-3">
				{#each gameTimePasses as pass (pass.id)}
					<button
						type="button"
						onclick={() => selectGameTimePass(pass.id)}
						class="relative flex cursor-pointer flex-col justify-between border-2 p-6 text-left transition-all active:scale-95 {selectedPassType ===
						pass.id
							? 'border-neon-cyan bg-neon-cyan/15 shadow-[0_0_25px_rgba(0,255,255,0.35)]'
							: 'border-gray-800 bg-black/70 hover:border-neon-cyan/60'}"
					>
						<span
							class="absolute -top-3 right-3 bg-neon-cyan px-2.5 py-0.5 font-mono text-xs font-bold tracking-wider text-black uppercase"
						>
							{pass.badge}
						</span>
						<div>
							<div class="mb-1 font-mono text-xs text-neon-cyan">
								{pass.durationMins} MINUTES TIMED SESSION
							</div>
							<div class="font-mono text-xl font-bold text-white">{pass.label}</div>
						</div>
						<div class="mt-4 flex items-baseline justify-between">
							<span class="font-mono text-3xl font-bold text-neon-green">${pass.amount}</span>
							<span class="font-mono text-xs text-gray-300">
								Includes {pass.tokens} A.S.T.R.O. Credits
							</span>
						</div>
					</button>
				{/each}
			</div>
		</div>

		<!-- Standard Token Packs Grid -->
		<div class="mb-4">
			<h2 class="font-mono text-2xl font-bold text-white uppercase">
				2. OR CHOOSE AN ARCADE TOKEN PACK
			</h2>
		</div>
		<div class="mb-12 grid grid-cols-2 gap-4 md:grid-cols-4">
			{#each tiers as tier (tier.amount)}
				<button
					type="button"
					onclick={() => selectTier(tier)}
					class="relative flex h-48 cursor-pointer flex-col justify-between border-2 p-6 text-left transition-all active:scale-95 {!selectedPassType &&
					selectedTier.amount === tier.amount
						? 'border-neon-green bg-neon-green/10 shadow-[0_0_20px_rgba(57,255,20,0.4)]'
						: 'border-gray-800 bg-black/60 hover:border-neon-cyan'}"
				>
					{#if tier.bonus > 0}
						<span
							class="absolute -top-3 right-3 bg-neon-magenta px-2 py-0.5 font-mono text-xs font-bold tracking-wider text-black uppercase"
						>
							+{tier.bonus}% BONUS
						</span>
					{/if}

					<div>
						<div class="mb-1 font-mono text-xs text-gray-400">{tier.tag}</div>
						<div class="font-mono text-3xl font-bold text-white">${tier.amount}</div>
					</div>

					<div>
						<div class="font-mono text-2xl font-bold text-neon-green">{tier.tokens} TOKENS</div>
						{#if tier.bonus > 0}
							<div class="font-mono text-xs text-neon-cyan">Includes bonus!</div>
						{/if}
					</div>
				</button>
			{/each}
		</div>

		<!-- Checkout Form Box -->
		<div
			class="mx-auto max-w-xl rounded-sm border-2 border-neon-cyan/50 bg-gray-950 p-8 backdrop-blur-md"
		>
			<h2 class="mb-6 text-center font-mono text-xl tracking-wider text-neon-cyan uppercase">
				CONFIRM SQUARE SELECTION:
				{#if activePassObj}
					<span class="text-neon-green">
						${activePassObj.amount} — {activePassObj.label} ({activePassObj.durationMins}M + {activePassObj.tokens} CR)
					</span>
				{:else}
					<span class="text-neon-green">${selectedTier.amount} ({selectedTier.tokens} TOKENS)</span>
				{/if}
			</h2>

			{#if errorMessage}
				<div
					class="mb-4 border border-neon-magenta bg-neon-magenta/10 p-3 text-center font-mono text-xs text-neon-magenta"
				>
					{errorMessage}
				</div>
			{/if}

			<div class="mb-4">
				<label for="email" class="mb-2 block font-mono text-xs text-gray-300 uppercase">
					DELIVERY EMAIL ADDRESS
				</label>
				<input
					id="email"
					type="email"
					bind:value={email}
					placeholder="your.name@example.com"
					class="w-full border border-gray-700 bg-black p-3 font-mono text-white transition-colors outline-none focus:border-neon-magenta"
				/>
			</div>

			<div class="mb-6">
				<label for="target-node" class="mb-2 block font-mono text-xs text-gray-300 uppercase">
					OPTIONAL: ASSIGN TO CABINET STATION (INSTANT CREDIT PUSH)
				</label>
				<select
					id="target-node"
					bind:value={targetNodeId}
					class="w-full border border-gray-700 bg-black p-3 font-mono text-white transition-colors outline-none focus:border-neon-cyan"
				>
					{#each cabinetTargets as cab (cab.id)}
						<option value={cab.id}>{cab.label}</option>
					{/each}
				</select>
			</div>

			<button
				type="button"
				onclick={handleCheckout}
				disabled={isProcessing}
				class="w-full cursor-pointer bg-neon-green py-4 font-mono text-lg font-bold tracking-widest text-black uppercase shadow-[0_0_15px_var(--color-neon-green)] transition-all hover:bg-white active:scale-98 disabled:opacity-50"
			>
				{isProcessing
					? 'CONNECTING TO SQUARE POS...'
					: activePassObj
						? `BUY ${activePassObj.durationMins}M A.S.T.R.O. PASS VIA SQUARE ($${activePassObj.amount})`
						: `PURCHASE $${selectedTier.amount} TOKEN PACK VIA SQUARE`}
			</button>

			<p class="mt-4 text-center font-mono text-xs text-gray-500">
				🔒 Square POS &amp; Stripe Encrypted • D1 Ledger Verified • B.O.S.S. Instant Cabinet Unlock
			</p>
		</div>
	{:else}
		<!-- Issued Voucher Card -->
		<div
			class="mx-auto max-w-md rounded-sm border-2 border-neon-green bg-gray-950 p-8 text-center shadow-[0_0_30px_rgba(57,255,20,0.3)]"
		>
			<div class="mb-2 font-mono text-xs tracking-widest text-neon-green uppercase">
				★ {voucherResult.durationMins ? 'SQUARE A.S.T.R.O. GAME-TIME PASS' : 'VOUCHER ISSUED'} ★
			</div>
			<h2 class="mb-4 font-mono text-3xl text-white">
				{#if voucherResult.durationMins}
					{voucherResult.durationMins}-MIN A.S.T.R.O. PASS (${voucherResult.amount})
				{:else}
					${voucherResult.amount} TOKEN VOUCHER
				{/if}
			</h2>

			<!-- Scannable Retro QR Code Container -->
			<div class="my-4 inline-block rounded border-4 border-black bg-white p-6">
				<div
					class="relative flex h-44 w-44 items-center justify-center bg-[radial-gradient(#000_30%,transparent_30%)] bg-[length:12px_12px]"
				>
					<div class="absolute inset-0 flex items-center justify-center border-8 border-black">
						<span class="bg-black p-1.5 font-mono text-xs font-bold text-neon-green">
							{voucherResult.voucherId}
						</span>
					</div>
				</div>
			</div>

			<div class="mb-2 font-mono text-sm text-neon-cyan">
				VOUCHER ID: {voucherResult.voucherId}
			</div>
			<div class="mb-4 font-mono text-lg font-bold text-neon-green">
				{voucherResult.tokens} TOKENS CREDITED
			</div>

			<p class="mb-6 font-mono text-xs text-gray-400">
				Sent to <span class="text-white">{voucherResult.email}</span>. Scan at the B.O.S.S. Jukebox
				Kiosk or push directly to a cabinet below:
			</p>

			{#if redeemMessage}
				<div
					class="mb-4 border border-neon-cyan bg-neon-cyan/10 p-3 font-mono text-xs text-neon-cyan"
				>
					{redeemMessage}
				</div>
			{/if}

			<div class="flex flex-col gap-3">
				<button
					type="button"
					onclick={redeemToCabinet}
					disabled={isRedeeming}
					class="cursor-pointer bg-neon-cyan px-4 py-3 font-mono text-xs font-bold text-black uppercase transition-colors hover:bg-white"
				>
					{isRedeeming
						? 'DISPATCHING TO B.O.S.S...'
						: `⚡ INJECT CREDITS TO ${voucherResult.targetNodeId || targetNodeId || 'ASTRO-01'} NOW`}
				</button>
				<a
					href={voucherResult.passUrl}
					class="border border-gray-600 bg-black px-4 py-2 font-mono text-xs text-white uppercase transition-colors hover:border-white"
				>
					🔗 VOUCHER PERMALINK
				</a>
				<button
					type="button"
					onclick={resetPurchase}
					class="cursor-pointer border border-neon-magenta bg-transparent px-4 py-2 font-mono text-xs text-neon-magenta uppercase transition-colors hover:bg-neon-magenta hover:text-black"
				>
					BUY ANOTHER PACK
				</button>
			</div>
		</div>
	{/if}
</div>
