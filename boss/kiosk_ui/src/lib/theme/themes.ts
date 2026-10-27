import { fetchUiTheme, setUiTheme, subscribeBossEvents } from '$lib/bossApi';

export interface ThemeOption {
	id: string;
	label: string;
	blurb: string;
}

/** Keep in sync with themes.css and AVAILABLE_THEMES in boss/src/ui_theme.rs. */
export const THEMES: ThemeOption[] = [
	{ id: 'taproom', label: 'High Score Taproom', blurb: '70s stripes, 80s marquee & CRT, 90s bevels' },
	{ id: 'vector', label: 'Vector Night', blurb: 'Asteroids & Tempest glowing outlines' },
	{ id: 'bigsky', label: 'Big Sky Arcade', blurb: 'Late-90s Sega daytime racer' },
	{ id: 'pinball', label: 'Pinball Backglass', blurb: '70s painted glass & gold leaf' },
	{ id: 'classic', label: 'Classic Neon', blurb: 'Original cyan neon console' }
];

export const DEFAULT_THEME = 'taproom';

const CACHE_KEY = 'boss_ui_theme';
const EFFECTS_KEY = 'boss_ui_effects';

function isKnown(id: string | null | undefined): id is string {
	return !!id && THEMES.some((t) => t.id === id);
}

export function applyTheme(id: string): void {
	if (typeof document === 'undefined' || !isKnown(id)) return;
	document.documentElement.dataset.theme = id;
	try {
		localStorage.setItem(CACHE_KEY, id);
	} catch {
		// storage unavailable; the venue setting is still fetched on load
	}
}

export function applyEffects(level: 'full' | 'lite'): void {
	if (typeof document === 'undefined') return;
	document.documentElement.dataset.effects = level;
	try {
		localStorage.setItem(EFFECTS_KEY, level);
	} catch {
		// storage unavailable; the server value is applied on every load
	}
}

export function currentTheme(): string {
	if (typeof document === 'undefined') return DEFAULT_THEME;
	return document.documentElement.dataset.theme ?? DEFAULT_THEME;
}

/**
 * Paint the cached theme immediately, then adopt the venue-wide theme from
 * B.O.S.S. and follow live changes on the `ui` SSE channel.
 * Returns an unsubscribe function.
 */
export function initTheme(onChange?: (id: string) => void): () => void {
	let cached: string | null = null;
	try {
		cached = localStorage.getItem(CACHE_KEY);
	} catch {
		cached = null;
	}
	applyTheme(isKnown(cached) ? cached : DEFAULT_THEME);

	fetchUiTheme().then(({ theme, effects }) => {
		if (effects) applyEffects(effects);
		if (isKnown(theme)) {
			applyTheme(theme);
			onChange?.(theme);
		}
	});

	return subscribeBossEvents(['ui'], (_channel, data) => {
		if (data?.kind === 'theme_changed' && isKnown(data.theme)) {
			applyTheme(data.theme);
			onChange?.(data.theme);
		}
	});
}

/** Operator action: switch every screen in the venue. */
export async function setVenueTheme(id: string): Promise<boolean> {
	if (!isKnown(id)) return false;
	const ok = await setUiTheme(id);
	if (ok) applyTheme(id);
	return ok;
}
