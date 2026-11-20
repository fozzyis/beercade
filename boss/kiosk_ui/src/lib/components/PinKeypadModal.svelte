<script lang="ts">
	import { unlockWithPin, verifyOperatorToken } from '$lib/bossApi';

	interface Props {
		open: boolean;
		onClose: () => void;
		onSuccess: (token: string) => void;
	}

	let { open, onClose, onSuccess }: Props = $props();

	let authTab = $state<'pin' | 'token'>('pin');
	let pin = $state('');
	let bearerToken = $state('');
	let busy = $state(false);
	let errorMsg = $state('');
	let shake = $state(false);

	const keypadDigits = ['1', '2', '3', '4', '5', '6', '7', '8', '9', 'CLR', '0', 'DEL'];

	function triggerShake(message: string) {
		errorMsg = message;
		shake = true;
		setTimeout(() => {
			shake = false;
		}, 450);
	}

	async function handleDigit(key: string) {
		if (busy) return;
		errorMsg = '';
		if (key === 'CLR') {
			pin = '';
			return;
		}
		if (key === 'DEL') {
			pin = pin.slice(0, -1);
			return;
		}
		if (pin.length < 6) {
			pin += key;
		}
		if (pin.length === 6) {
			await submitPin();
		}
	}

	async function submitPin() {
		if (pin.length !== 6 || busy) return;
		busy = true;
		errorMsg = '';
		const res = await unlockWithPin(pin);
		busy = false;
		if (res.ok && res.token) {
			const issued = res.token;
			pin = '';
			onSuccess(issued);
		} else {
			pin = '';
			triggerShake(res.detail || 'Invalid 6-digit operator PIN.');
		}
	}

	async function submitBearerToken(e: SubmitEvent) {
		e.preventDefault();
		if (!bearerToken.trim() || busy) return;
		busy = true;
		errorMsg = '';
		const ok = await verifyOperatorToken(bearerToken.trim());
		busy = false;
		if (ok) {
			const clean = bearerToken.trim();
			bearerToken = '';
			onSuccess(clean);
		} else {
			triggerShake('Rejected BOSS_RPC_TOKEN.');
		}
	}
</script>

{#if open}
	<div
		class="modal-backdrop"
		role="dialog"
		aria-modal="true"
		aria-label="B.O.S.S. Operator Access Unlock"
	>
		<div class="modal-card {shake ? 'shake' : ''}">
			<div class="modal-header">
				<div>
					<span class="badge">DUAL-MODE SUPERVISORY GATE</span>
					<h2>B.O.S.S. Operator Unlock</h2>
				</div>
				<button class="close-btn" onclick={onClose} aria-label="Close modal">✕</button>
			</div>

			<div class="mode-tabs">
				<button
					type="button"
					class="mode-tab {authTab === 'pin' ? 'active' : ''}"
					onclick={() => {
						authTab = 'pin';
						errorMsg = '';
					}}
				>
					6-Digit Touch PIN
				</button>
				<button
					type="button"
					class="mode-tab {authTab === 'token' ? 'active' : ''}"
					onclick={() => {
						authTab = 'token';
						errorMsg = '';
					}}
				>
					LAN Bearer Token
				</button>
			</div>

			{#if errorMsg}
				<div class="error-banner">{errorMsg}</div>
			{/if}

			{#if authTab === 'pin'}
				<p class="hint">
					Enter the 6-digit physical kiosk operator PIN
				</p>

				<div class="pin-dots" aria-live="polite">
					{#each Array(6) as _, idx}
						<div class="pin-dot {idx < pin.length ? 'filled' : ''}">
							{idx < pin.length ? '●' : ''}
						</div>
					{/each}
				</div>

				<div class="keypad-grid">
					{#each keypadDigits as key}
						<button
							type="button"
							class="key-btn {key === 'CLR' || key === 'DEL' ? 'utility' : ''}"
							disabled={busy}
							onclick={() => handleDigit(key)}
						>
							{key}
						</button>
					{/each}
				</div>
			{:else}
				<form class="token-form" onsubmit={submitBearerToken}>
					<p class="hint">
						Authenticate from a venue LAN browser using <code>BOSS_RPC_TOKEN</code>:
					</p>
					<input
						type="password"
						class="token-input"
						placeholder="Paste BOSS_RPC_TOKEN..."
						bind:value={bearerToken}
						autocomplete="off"
					/>
					<button type="submit" class="submit-btn" disabled={busy || !bearerToken.trim()}>
						{busy ? 'VERIFYING...' : 'AUTHENTICATE CONSOLE →'}
					</button>
				</form>
			{/if}
		</div>
	</div>
{/if}

<style>
	.modal-backdrop {
		position: fixed;
		inset: 0;
		background: rgba(var(--scrim-rgb), 0.88);
		backdrop-filter: blur(8px);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 9999;
		padding: 1.5rem;
	}

	.modal-card {
		width: 100%;
		max-width: 420px;
		background: var(--surface-2);
		border: 2px solid var(--accent);
		box-shadow:
			0 0 30px rgba(var(--accent-rgb), 0.25),
			inset 0 0 15px rgba(var(--accent-rgb), 0.08);
		border-radius: 16px;
		padding: 1.5rem;
		color: var(--text);
	}

	.modal-card.shake {
		animation: shakeAnim 0.4s ease;
		border-color: var(--danger);
	}

	@keyframes shakeAnim {
		0%,
		100% {
			transform: translateX(0);
		}
		20%,
		60% {
			transform: translateX(-10px);
		}
		40%,
		80% {
			transform: translateX(10px);
		}
	}

	.modal-header {
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		margin-bottom: 1rem;
	}

	.badge {
		display: inline-block;
		font-size: 0.7rem;
		letter-spacing: 0.14em;
		color: var(--accent);
		background: rgba(var(--accent-rgb), 0.12);
		padding: 0.2rem 0.55rem;
		border-radius: 4px;
		font-weight: 700;
	}

	h2 {
		margin: 0.35rem 0 0 0;
		font-size: 1.35rem;
		color: var(--text-strong);
		letter-spacing: 0.04em;
	}

	.close-btn {
		background: transparent;
		border: 1px solid var(--border-strong);
		color: var(--text-muted);
		width: 36px;
		height: 36px;
		border-radius: 8px;
		cursor: pointer;
		font-size: 1rem;
	}

	.close-btn:hover {
		border-color: var(--danger);
		color: var(--danger);
	}

	.mode-tabs {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.5rem;
		margin-bottom: 1rem;
	}

	.mode-tab {
		padding: 0.6rem;
		background: var(--surface-3);
		border: 1px solid var(--border-strong);
		color: var(--text-muted);
		border-radius: 8px;
		font-family: inherit;
		font-weight: 700;
		cursor: pointer;
		font-size: 0.82rem;
	}

	.mode-tab.active {
		background: rgba(var(--accent-rgb), 0.15);
		border-color: var(--accent);
		color: var(--accent);
	}

	.error-banner {
		background: rgba(var(--danger-rgb), 0.16);
		border: 1px solid var(--danger);
		color: var(--danger-soft);
		padding: 0.6rem 0.8rem;
		border-radius: 8px;
		font-size: 0.85rem;
		margin-bottom: 0.85rem;
		text-align: center;
	}

	.hint {
		font-size: 0.82rem;
		color: var(--text-muted);
		text-align: center;
		margin: 0 0 1rem 0;
	}

	.hint code {
		color: var(--accent);
		background: rgba(var(--accent-rgb), 0.1);
		padding: 0.1rem 0.35rem;
		border-radius: 4px;
	}

	.pin-dots {
		display: flex;
		justify-content: center;
		gap: 0.65rem;
		margin-bottom: 1.25rem;
	}

	.pin-dot {
		width: 44px;
		height: 50px;
		border-radius: 10px;
		border: 2px solid var(--border-strong);
		background: var(--inset);
		display: flex;
		align-items: center;
		justify-content: center;
		font-size: 1.35rem;
		color: var(--accent);
	}

	.pin-dot.filled {
		border-color: var(--accent);
		box-shadow: 0 0 12px rgba(var(--accent-rgb), 0.35);
	}

	.keypad-grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 0.65rem;
	}

	.key-btn {
		height: 62px;
		border-radius: 12px;
		border: 1px solid var(--border-strong);
		background: var(--surface-3);
		color: var(--text-strong);
		font-size: 1.35rem;
		font-weight: 700;
		font-family: inherit;
		cursor: pointer;
		transition: all 0.12s ease;
	}

	.key-btn:hover,
	.key-btn:active {
		background: rgba(var(--accent-rgb), 0.2);
		border-color: var(--accent);
		transform: scale(0.98);
	}

	.key-btn.utility {
		font-size: 0.95rem;
		color: var(--warn);
	}

	.token-form {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.token-input {
		padding: 0.85rem;
		border-radius: 8px;
		border: 1px solid var(--border-strong);
		background: var(--inset);
		color: var(--text-strong);
		font-family: inherit;
		font-size: 0.95rem;
	}

	.submit-btn {
		padding: 0.9rem;
		border-radius: 8px;
		border: 1px solid var(--accent);
		background: rgba(var(--accent-rgb), 0.18);
		color: var(--accent);
		font-weight: 700;
		font-family: inherit;
		cursor: pointer;
	}
</style>
