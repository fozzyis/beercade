import type {
	ActiveSessionSummary,
	AdminOverview,
	CatalogueGame,
	CommandRecord,
	CommandStatus,
	GameRef,
	NetplaySessionSummary,
	NodeTelemetry,
	OtaPayloadEntry,
	PluginDescriptor,
	PluginManifest,
	SessionLookupResponse,
	VenueTelemetryReport
} from '$lib/types/telemetry';

const STORAGE_TOKEN_KEY = 'boss_operator_token';
const STORAGE_EXPIRES_KEY = 'boss_operator_expires_ms';

export function getApiBase(): string {
	if (typeof window === 'undefined') return '';
	// When running via vite dev on :5173 while boss-server runs on :8989, allow override or same-origin
	const envUrl = (import.meta as any).env?.VITE_BOSS_API_URL;
	if (envUrl) return envUrl.replace(/\/$/, '');
	return '';
}

export function getStoredOperatorToken(): string | null {
	if (typeof window === 'undefined') return null;
	const token = sessionStorage.getItem(STORAGE_TOKEN_KEY);
	const expiresRaw = sessionStorage.getItem(STORAGE_EXPIRES_KEY);
	if (!token) return null;
	if (expiresRaw) {
		const exp = Number(expiresRaw);
		if (Number.isFinite(exp) && Date.now() > exp) {
			clearOperatorToken();
			return null;
		}
	}
	return token;
}

export function setStoredOperatorToken(token: string, expiresAtMs?: number): void {
	if (typeof window === 'undefined') return;
	sessionStorage.setItem(STORAGE_TOKEN_KEY, token);
	const exp = expiresAtMs ?? Date.now() + 15 * 60 * 1000;
	sessionStorage.setItem(STORAGE_EXPIRES_KEY, String(exp));
}

export function clearOperatorToken(): void {
	if (typeof window === 'undefined') return;
	sessionStorage.removeItem(STORAGE_TOKEN_KEY);
	sessionStorage.removeItem(STORAGE_EXPIRES_KEY);
}

function authHeaders(token?: string | null): Record<string, string> {
	const t = token ?? getStoredOperatorToken();
	return t
		? {
				'Content-Type': 'application/json',
				Authorization: `Bearer ${t}`
			}
		: {
				'Content-Type': 'application/json'
			};
}

// ---------------------------------------------------------------------------
// Realistic Fallback Fleet & Plugin State (used when previewing UI offline)
// ---------------------------------------------------------------------------

function buildFallbackNodes(): NodeTelemetry[] {
	const now = Date.now();
	return [
		{
			node_id: 'ASTRO-01',
			hardware: 'O.R.I.O.N. Radxa ROCK 5C 32GB',
			soc: 'Rockchip RK3588S2 (A.S.T.R.O. KMS/DRM)',
			cpu_temp_c: 54.2,
			gpu_temp_c: 51.8,
			npu_load_pct: 12.5,
			active_core: 'fbneo_libretro',
			active_rom: 'sfiii3.zip',
			frame_time_ms: 16.64,
			fps: 59.98,
			input_jitter: {
				poll_rate_hz: 1000,
				p50_us: 112,
				p95_us: 245,
				p99_us: 390,
				max_us: 620,
				dropped_events: 0
			},
			netplay: {
				peer_id: 'ASTRO-02',
				rtt_ms: 1.4,
				jitter_ms: 0.2,
				packet_loss_pct: 0.0,
				rollback_frames: 1,
				is_host: true
			},
			credits: {
				credits_available: 6,
				credits_inserted_total: 418,
				credits_dispensed_today: 48,
				last_coin_epoch_ms: now - 45_000
			},
			mode: { type: 'Standalone' },
			timestamp_epoch_ms: now - 800
		},
		{
			node_id: 'ASTRO-02',
			hardware: 'O.R.I.O.N. Radxa ROCK 5C 32GB',
			soc: 'Rockchip RK3588S2 (A.S.T.R.O. KMS/DRM)',
			cpu_temp_c: 56.8,
			gpu_temp_c: 53.1,
			npu_load_pct: 14.0,
			active_core: 'fbneo_libretro',
			active_rom: 'sfiii3.zip',
			frame_time_ms: 16.66,
			fps: 60.0,
			input_jitter: {
				poll_rate_hz: 1000,
				p50_us: 118,
				p95_us: 260,
				p99_us: 410,
				max_us: 690,
				dropped_events: 0
			},
			netplay: {
				peer_id: 'ASTRO-01',
				rtt_ms: 1.4,
				jitter_ms: 0.2,
				packet_loss_pct: 0.0,
				rollback_frames: 1,
				is_host: false
			},
			credits: {
				credits_available: 4,
				credits_inserted_total: 389,
				credits_dispensed_today: 41,
				last_coin_epoch_ms: now - 90_000
			},
			mode: { type: 'Standalone' },
			timestamp_epoch_ms: now - 950
		},
		{
			node_id: 'ASTRO-03',
			hardware: 'O.R.I.O.N. Orange Pi 5 Pro 16GB LPDDR5',
			soc: 'Rockchip RK3588S (A.S.T.R.O. KMS/DRM)',
			cpu_temp_c: 62.4,
			gpu_temp_c: 59.0,
			npu_load_pct: 38.2,
			active_core: 'mame2003_plus',
			active_rom: 'mslugX.zip',
			frame_time_ms: 16.67,
			fps: 59.95,
			input_jitter: {
				poll_rate_hz: 1000,
				p50_us: 140,
				p95_us: 310,
				p99_us: 480,
				max_us: 810,
				dropped_events: 0
			},
			credits: {
				credits_available: 2,
				credits_inserted_total: 275,
				credits_dispensed_today: 29,
				last_coin_epoch_ms: now - 180_000
			},
			mode: { type: 'Standalone' },
			timestamp_epoch_ms: now - 1200
		},
		{
			node_id: 'ASTRO-04',
			hardware: 'O.R.I.O.N. Orange Pi 5 Pro 16GB LPDDR5',
			soc: 'Rockchip RK3588S (A.S.T.R.O. KMS/DRM)',
			cpu_temp_c: 49.5,
			gpu_temp_c: 47.0,
			npu_load_pct: 5.0,
			active_core: 'flycast_libretro',
			active_rom: 'mvc2.zip',
			frame_time_ms: 16.65,
			fps: 60.0,
			input_jitter: {
				poll_rate_hz: 1000,
				p50_us: 125,
				p95_us: 280,
				p99_us: 430,
				max_us: 740,
				dropped_events: 0
			},
			credits: {
				credits_available: 0,
				credits_inserted_total: 512,
				credits_dispensed_today: 63,
				last_coin_epoch_ms: now - 320_000
			},
			mode: { type: 'Standalone' },
			timestamp_epoch_ms: now - 600
		}
	];
}

function buildFallbackPlugins(): PluginDescriptor[] {
	const now = Date.now();
	return [
		{
			manifest: {
				id: 'pos-payments',
				name: 'POS & Payment Terminals (Square / Stripe / Nayax)',
				version: '1.2.0',
				category: 'pos_payments',
				description:
					'Square Terminal & Online Checkout, A.S.T.R.O. timed game-session passes, Stripe WisePOS, and Nayax coin-pulse bridge.',
				author: 'B.E.E.R.C.A.D.E. Core',
				enabled: true,
				builtin: true,
				subscribed_events: ['credit_transaction'],
				capabilities: [
					'square_terminal',
					'square_online_checkout',
					'astro_timed_sessions',
					'qr_checkout',
					'terminal_tap',
					'daily_reconciliation'
				],
				ui_panel: {
					tab_label: 'POS & Square',
					icon: 'credit-card',
					summary_metric_key: 'total_credits_sold'
				},
				webhook_url: null
			},
			state: {
				square_config: {
					enabled: true,
					environment: 'production',
					location_id: 'L_BEERCADE_REDFERN',
					terminal_device_id: 'sq_term_kiosk_01',
					access_token_configured: true,
					webhook_signature_verified: true
				},
				terminals: [
					{
						id: 'term-square-kiosk-01',
						provider: 'Square Terminal (NFC / Chip / QR)',
						label: 'B.O.S.S. Main Kiosk Reader',
						location: 'Front Entrance Supervisor Kiosk',
						status: 'online',
						last_ping_ms: now
					},
					{
						id: 'term-square-bar-02',
						provider: 'Square Register POS & KDS',
						label: 'Taproom Bar & Kitchen POS',
						location: 'Main Bar Tap Wall',
						status: 'online',
						last_ping_ms: now
					},
					{
						id: 'term-stripe-01',
						provider: 'Stripe Terminal WisePOS E',
						label: 'Secondary Kiosk Reader',
						location: 'Arcade Floor North',
						status: 'online',
						last_ping_ms: now
					},
					{
						id: 'term-nayax-01',
						provider: 'Nayax VPOS Touch',
						label: 'Coin-Pulse Bridge',
						location: 'VIP Booth Lounge',
						status: 'online',
						last_ping_ms: now
					}
				],
				tiers: [
					{ id: 'pack-1', label: 'Single Drop', credits: 1, price_cents: 100, badge: null },
					{ id: 'pack-5', label: 'Squad 5-Pack', credits: 5, price_cents: 450, badge: 'POPULAR' },
					{
						id: 'pack-15',
						label: 'Arcade Boss 15-Pack',
						credits: 15,
						price_cents: 1200,
						badge: 'BEST VALUE'
					}
				],
				game_time_tiers: [
					{
						id: 'astro-30m',
						label: '30-Min A.S.T.R.O. Session',
						duration_mins: 30,
						included_credits: 10,
						price_cents: 1200,
						badge: 'QUICK PLAY'
					},
					{
						id: 'astro-60m',
						label: '60-Min Unlimited A.S.T.R.O. Pass',
						duration_mins: 60,
						included_credits: 25,
						price_cents: 2000,
						badge: 'MOST POPULAR'
					},
					{
						id: 'astro-120m',
						label: '2-Hour A.S.T.R.O. Marathon Pass',
						duration_mins: 120,
						included_credits: 60,
						price_cents: 3500,
						badge: 'BEST VALUE'
					}
				],
				active_timed_sessions: [
					{
						session_id: 'sess_astro_01',
						pass_code: 'ASTRO-SQ-8841',
						patron_name: 'Alex (Online Square Pass)',
						node_id: 'ASTRO-01',
						duration_mins: 60,
						started_at_ms: now - 18 * 60 * 1000,
						expires_at_ms: now + 42 * 60 * 1000,
						provider: 'Square Online Checkout',
						status: 'active'
					},
					{
						session_id: 'sess_astro_02',
						pass_code: 'ASTRO-SQ-3910',
						patron_name: 'Maya (Square Kiosk Pass)',
						node_id: 'ASTRO-03',
						duration_mins: 30,
						started_at_ms: now - 8 * 60 * 1000,
						expires_at_ms: now + 22 * 60 * 1000,
						provider: 'Square Terminal NFC',
						status: 'active'
					}
				],
				active_sessions: [],
				ledger: [
					{
						id: 'ASTRO-SQ-8841',
						provider: 'Square Online Game-Time Pass (60m · Alex)',
						node_id: 'ASTRO-01',
						credits: 25,
						amount_cents: 2000,
						reference: 'cmd_sq_8841',
						created_at_ms: now - 18 * 60 * 1000
					},
					{
						id: 'pos_91a8f021',
						provider: 'Square Terminal NFC',
						node_id: 'ASTRO-02',
						credits: 5,
						amount_cents: 450,
						reference: 'cmd_7741',
						created_at_ms: now - 140_000
					}
				],
				total_revenue_cents: 24450,
				total_credits_sold: 265,
				transactions_count: 48
			},
			updated_at_epoch_ms: now
		},
		{
			manifest: {
				id: 'venue-lighting',
				name: 'Multi-System Venue Lighting (WLED / Art-Net DMX / sACN / Hue)',
				version: '1.2.0',
				category: 'venue_lighting',
				description:
					'Multi-zone lighting orchestration across WLED, Art-Net DMX512, sACN E1.31, Philips Hue, and QLC+ OSC with reactive event cues.',
				author: 'B.E.E.R.C.A.D.E. Core',
				enabled: true,
				builtin: true,
				subscribed_events: ['high_score', 'credit_transaction', 'node_offline', 'error_reported'],
				capabilities: [
					'multi_zone',
					'dmx_artnet',
					'sacn_e131',
					'wled_http',
					'philips_hue',
					'reactive_cues'
				],
				ui_panel: {
					tab_label: 'Lighting & DMX',
					icon: 'sparkles',
					summary_metric_key: 'active_scene'
				},
				webhook_url: null
			},
			state: {
				active_scene: 'arcade_attract',
				master_brightness_pct: 85,
				available_scenes: [
					{ id: 'arcade_attract', label: 'Arcade Attract (Cyan/Magenta)', color: '#00f0ff' },
					{ id: 'cyberpunk_neon', label: 'Cyberpunk Synthwave', color: '#ff0055' },
					{ id: 'high_score_strobe', label: 'High Score Strobe Burst', color: '#ffe600' },
					{ id: 'jackpot_gold', label: 'Jackpot Gold Chase', color: '#ffaa00' },
					{ id: 'thermal_alert_red', label: 'Ops / Thermal Alert Red', color: '#ff2222' },
					{ id: 'chill_lounge', label: 'Late Night Warm Lounge', color: '#9d4edd' }
				],
				reactive_rules: {
					strobe_on_high_score: true,
					pulse_on_credit_insert: true,
					red_alert_on_node_offline: true
				},
				zones: [
					{
						id: 'arcade_floor',
						name: 'Main Arcade Floor & Cabinet Row',
						active_scene: 'arcade_attract',
						brightness_pct: 90
					},
					{
						id: 'tournament_stage',
						name: 'Tournament Stage & Broadcast Rig',
						active_scene: 'cyberpunk_neon',
						brightness_pct: 100
					},
					{
						id: 'taproom_bar',
						name: 'Taproom Bar & Craft Tap Wall',
						active_scene: 'chill_lounge',
						brightness_pct: 75
					},
					{
						id: 'cabinet_marquees',
						name: 'O.R.I.O.N. Cabinet Marquee Matrix',
						active_scene: 'arcade_attract',
						brightness_pct: 95
					}
				],
				fixtures: [
					{
						id: 'wled-arch-01',
						name: 'Cabinet Row Neon Canopy',
						zone: 'arcade_floor',
						protocol: 'WLED JSON API',
						endpoint: 'http://10.10.20.51/json/state',
						universe: 1,
						active_scene: 'arcade_attract',
						brightness_pct: 90,
						online: true
					},
					{
						id: 'artnet-stage-01',
						name: 'Main Floor DMX Moving Heads',
						zone: 'tournament_stage',
						protocol: 'Art-Net UDP:6454',
						endpoint: '10.10.20.255:6454',
						universe: 2,
						active_scene: 'cyberpunk_neon',
						brightness_pct: 100,
						online: true
					},
					{
						id: 'sacn-stage-02',
						name: 'Stage Truss RGBW Blinders',
						zone: 'tournament_stage',
						protocol: 'sACN E1.31 UDP:5568',
						endpoint: '239.255.0.3:5568',
						universe: 3,
						active_scene: 'cyberpunk_neon',
						brightness_pct: 100,
						online: true
					},
					{
						id: 'wled-bar-02',
						name: 'Bar Tap Backlight Strip',
						zone: 'taproom_bar',
						protocol: 'WLED JSON API',
						endpoint: 'http://10.10.20.52/json/state',
						universe: 4,
						active_scene: 'chill_lounge',
						brightness_pct: 75,
						online: true
					},
					{
						id: 'hue-booths-03',
						name: 'Taproom Booth Pendant Cluster',
						zone: 'taproom_bar',
						protocol: 'Philips Hue / Zigbee2MQTT',
						endpoint: 'http://10.10.20.55/api/groups/booths',
						universe: 5,
						active_scene: 'chill_lounge',
						brightness_pct: 70,
						online: true
					},
					{
						id: 'wled-marquees-04',
						name: 'O.R.I.O.N. Cabinet Marquee Bus',
						zone: 'cabinet_marquees',
						protocol: 'WLED DDP / UDP',
						endpoint: 'http://10.10.20.58/json/state',
						universe: 6,
						active_scene: 'arcade_attract',
						brightness_pct: 95,
						online: true
					}
				],
				cue_history: [
					{
						scene: 'arcade_attract',
						zone: 'ALL_ZONES',
						reason: 'B.O.S.S. multi-controller boot default scene',
						triggered_at_ms: now - 600_000
					}
				]
			},
			updated_at_epoch_ms: now
		},
		{
			manifest: {
				id: 'tournament-engine',
				name: 'Tournament, High-Score & TV Attractor Engine',
				version: '1.2.0',
				category: 'tournament_engine',
				description:
					'Bracket management, real-time high-score leaderboards, and on-site TV attractor signage streaming (/tv).',
				author: 'B.E.E.R.C.A.D.E. Core',
				enabled: true,
				builtin: true,
				subscribed_events: ['high_score'],
				capabilities: [
					'brackets',
					'score_attack',
					'kiosk_attractor_overlay',
					'tv_signage_stream'
				],
				ui_panel: {
					tab_label: 'Tournaments & TV',
					icon: 'trophy',
					summary_metric_key: 'status'
				},
				webhook_url: null
			},
			state: {
				title: 'FRIDAY NIGHT FIGHTS: STREET FIGHTER III 3RD STRIKE',
				game_name: 'Street Fighter III: 3rd Strike',
				target_rom: 'sfiii3',
				mode: 'single_elimination',
				status: 'live',
				prize_pool: '$150 Bar Tab + Champion Belt + 50 Tokens',
				kiosk_overlay_enabled: true,
				tv_announcement_banner:
					'SCAN QR ON ANY TV TO BUY A.S.T.R.O. GAME TIME VIA SQUARE · HAPPY HOUR $8 PINTS UNTIL 7PM',
				tv_slide_interval_secs: 12,
				tv_displays: [
					{
						id: 'tv-main-stage',
						name: 'Main Floor 75" High-Score Wall',
						zone: 'main',
						stream_url: '/tv?zone=main',
						output_mode: 'B.O.S.S. HDMI-1 + LAN Web Stream',
						slide_interval_secs: 12,
						active_slides: ['high_scores', 'tournament_bracket', 'live_cabinets', 'taproom_promo'],
						online: true
					},
					{
						id: 'tv-taproom-bar',
						name: 'Taproom Bar Overhead 55" Signage',
						zone: 'bar',
						stream_url: '/tv?zone=bar',
						output_mode: 'Smart TV LAN Kiosk Browser',
						slide_interval_secs: 15,
						active_slides: ['taproom_promo', 'high_scores', 'live_cabinets'],
						online: true
					},
					{
						id: 'tv-stage-rig',
						name: 'Tournament Stage Spectator Projector',
						zone: 'stage',
						stream_url: '/tv?zone=stage',
						output_mode: 'Smart TV LAN Kiosk Browser',
						slide_interval_secs: 10,
						active_slides: ['tournament_bracket', 'high_scores'],
						online: true
					}
				],
				active_fanfare: null,
				registered_players: ['DAIGO (DGO)', 'JUSTIN (JWG)', 'KURODA (KRD)', 'MOV (MOV)'],
				leaderboard: [
					{
						initials: 'DGO',
						player_name: 'DAIGO',
						score: 984200,
						node_id: 'ASTRO-01',
						achieved_at_ms: now - 600_000
					},
					{
						initials: 'JWG',
						player_name: 'JUSTIN',
						score: 912500,
						node_id: 'ASTRO-02',
						achieved_at_ms: now - 420_000
					},
					{
						initials: 'KRD',
						player_name: 'KURODA',
						score: 875000,
						node_id: 'ASTRO-01',
						achieved_at_ms: now - 180_000
					}
				],
				bracket_matches: [
					{
						id: 'm-r1-1',
						round: 1,
						player_a: 'DAIGO (DGO)',
						player_b: 'JUSTIN (JWG)',
						winner: 'DAIGO (DGO)',
						cabinet_node_id: 'ASTRO-01',
						status: 'completed'
					},
					{
						id: 'm-r1-2',
						round: 1,
						player_a: 'KURODA (KRD)',
						player_b: 'MOV (MOV)',
						winner: null,
						cabinet_node_id: 'ASTRO-02',
						status: 'in_progress'
					}
				]
			},
			updated_at_epoch_ms: now
		},
		{
			manifest: {
				id: 'rfid-peripherals',
				name: 'RFID / NFC Wristband & Card Peripherals',
				version: '1.0.0',
				category: 'rfid_peripherals',
				description:
					'Tap-to-credit NFC wristband readers, player profile & high-score initials lookup, and VIP free-play passes.',
				author: 'B.E.E.R.C.A.D.E. Core',
				enabled: true,
				builtin: true,
				subscribed_events: ['credit_transaction'],
				capabilities: ['nfc_tap_to_play', 'vip_freeplay', 'card_topup'],
				ui_panel: {
					tab_label: 'RFID & Wristbands',
					icon: 'wifi',
					summary_metric_key: 'active_cards_count'
				},
				webhook_url: null
			},
			state: {
				active_cards_count: 3,
				credits_per_tap: 1,
				cards: [
					{
						uid: '04:A1:9B:22:C8:10:80',
						holder_name: 'Venue VIP Wristband #01',
						initials: 'VIP',
						credits_balance: 99,
						vip_freeplay: true,
						active: true,
						last_tap_node: 'ASTRO-01',
						last_tap_ms: now - 120_000,
						total_taps: 42
					},
					{
						uid: '04:7F:3C:11:90:55:80',
						holder_name: 'Patron Card #104 (Alex)',
						initials: 'ALX',
						credits_balance: 14,
						vip_freeplay: false,
						active: true,
						last_tap_node: 'ASTRO-02',
						last_tap_ms: now - 300_000,
						total_taps: 11
					},
					{
						uid: '04:E2:44:8A:19:02:80',
						holder_name: 'Tournament Pass #08',
						initials: 'TRN',
						credits_balance: 25,
						vip_freeplay: false,
						active: true,
						last_tap_node: null,
						last_tap_ms: null,
						total_taps: 0
					}
				],
				recent_taps: []
			},
			updated_at_epoch_ms: now
		},
		{
			manifest: {
				id: 'alerting-sinks',
				name: 'Alerting & Ops Sinks',
				version: '1.0.0',
				category: 'alerting_sinks',
				description:
					'Discord, Slack, PagerDuty, and Telegram webhook routing for cabinet errors, thermal warnings, and offline nodes.',
				author: 'B.E.E.R.C.A.D.E. Core',
				enabled: true,
				builtin: true,
				subscribed_events: ['telemetry_heartbeat', 'error_reported', 'node_offline'],
				capabilities: ['discord_webhook', 'slack_webhook', 'pagerduty_events', 'thermal_watchdog'],
				ui_panel: {
					tab_label: 'Alerting & Sinks',
					icon: 'bell',
					summary_metric_key: 'deliveries_count'
				},
				webhook_url: null
			},
			state: {
				thermal_warn_temp_c: 78.0,
				deliveries_count: 1,
				sinks: [
					{
						id: 'sink-discord-ops',
						name: 'Discord #venue-ops-alerts',
						kind: 'discord',
						webhook_url: 'https://discord.com/api/webhooks/beercade-ops/simulated',
						enabled: true,
						events: ['error', 'fatal', 'thermal', 'offline']
					},
					{
						id: 'sink-slack-tech',
						name: 'Slack #cabinet-techs',
						kind: 'slack',
						webhook_url: 'https://hooks.slack.com/services/T000/B000/beercade',
						enabled: true,
						events: ['fatal', 'offline']
					}
				],
				deliveries: [
					{
						id: 'alrt_0192b1',
						sink_name: 'Discord #venue-ops-alerts',
						severity: 'info',
						title: 'B.O.S.S. Boot Watchdog Active',
						message: 'All 4 ASTRO cabinet nodes reporting nominal thermals (<65°C).',
						node_id: 'BOSS-CENTRAL',
						delivered_at_ms: now - 900_000,
						status: 'delivered (200 OK)'
					}
				]
			},
			updated_at_epoch_ms: now
		}
	];
}

// ---------------------------------------------------------------------------
// API Methods
// ---------------------------------------------------------------------------

export async function fetchVenueTelemetry(): Promise<VenueTelemetryReport> {
	try {
		const res = await fetch(`${getApiBase()}/api/telemetry`);
		if (res.ok) return await res.json();
	} catch {
		// Fall through to simulated venue report
	}
	const nodes = buildFallbackNodes();
	return {
		venue: 'Beercade Redfern (The BK)',
		address: '113-115 Regent St, Redfern NSW 2016',
		status: 'OPEN',
		nodes_online: nodes.length,
		nodes,
		generated_at_epoch_ms: Date.now()
	};
}

export async function fetchPlugins(): Promise<PluginDescriptor[]> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/plugins`);
		if (res.ok) {
			const data = await res.json();
			return data.plugins ?? [];
		}
	} catch {
		// Fall through
	}
	return buildFallbackPlugins();
}

export async function unlockWithPin(
	pin: string
): Promise<{ ok: boolean; token?: string; expires_at_epoch_ms?: number; detail?: string }> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/auth/pin`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ pin })
		});
		const data = await res.json();
		if (res.ok && data.ok && data.token) {
			setStoredOperatorToken(data.token, data.expires_at_epoch_ms);
			return data;
		}
		return { ok: false, detail: data.detail || 'Incorrect 6-digit operator PIN.' };
	} catch {
		if (import.meta.env.VITE_DEV_MOCK === 'true') {
			const mockPin = import.meta.env.VITE_DEV_MOCK_PIN;
			if (mockPin && pin.trim() === mockPin) {
				const token = 'boss_sess_preview_offline';
				const exp = Date.now() + 15 * 60 * 1000;
				setStoredOperatorToken(token, exp);
				return { ok: true, token, expires_at_epoch_ms: exp };
			}
			return { ok: false, detail: 'Incorrect 6-digit operator PIN.' };
		}
		return { ok: false, detail: 'B.O.S.S. server unreachable' };
	}
}

export async function verifyOperatorToken(token: string): Promise<boolean> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/auth/verify`, {
			headers: authHeaders(token)
		});
		if (res.ok) {
			setStoredOperatorToken(token);
			return true;
		}
		return false;
	} catch {
		if (import.meta.env.VITE_DEV_MOCK === 'true') {
			if (token.trim().length >= 6) {
				setStoredOperatorToken(token.trim());
				return true;
			}
		}
		return false;
	}
}

export async function logoutOperator(): Promise<void> {
	try {
		await fetch(`${getApiBase()}/api/v1/auth/logout`, {
			method: 'POST',
			headers: authHeaders()
		});
	} catch {
		// Ignore network error on logout
	}
	clearOperatorToken();
}

export async function fetchAdminOverview(): Promise<{
	overview: AdminOverview;
	simulated: boolean;
}> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/admin/overview`, {
			headers: authHeaders()
		});
		if (res.ok) {
			return { overview: await res.json(), simulated: false };
		}
	} catch {
		// Fall through to simulated overview
	}

	const nodes = buildFallbackNodes();
	const now = Date.now();
	return {
		simulated: true,
		overview: {
			venue: 'Beercade Redfern (The BK)',
			address: '113-115 Regent St, Redfern NSW 2016',
			status: 'OPEN',
			nodes_online: nodes.length,
			nodes: nodes.map((t) => ({
				telemetry: t,
				last_seen_epoch_ms: t.timestamp_epoch_ms,
				age_ms: now - t.timestamp_epoch_ms,
				online: true
			})),
			commands: [
				{
					id: 'cmd_f901a28b',
					node_id: 'ASTRO-01',
					command: { type: 'add_credits', credits: 5 },
					status: 'acknowledged' as CommandStatus,
					issued_at_epoch_ms: now - 140_000,
					dispatched_at_epoch_ms: now - 139_800,
					completed_at_epoch_ms: now - 139_500
				}
			],
			errors: [],
			scores: [
				{
					node_id: 'ASTRO-01',
					active_core: 'fbneo_libretro',
					active_rom: 'sfiii3.zip',
					game_name: 'Street Fighter III: 3rd Strike',
					score: 984200,
					initials: 'DGO',
					achieved_at_epoch_ms: now - 600_000
				}
			],
			transactions: [
				{
					node_id: 'ASTRO-01',
					kind: 'pos_purchase',
					credits: 5,
					balance_after: 6,
					command_id: 'cmd_f901a28b',
					created_at_epoch_ms: now - 140_000
				}
			],
			edge_sync: {
				enabled: true,
				endpoint: 'https://beercade.gg/api/v1/edge/ingest',
				buffered_items: 0
			},
			plugins: buildFallbackPlugins(),
			active_sessions: [],
			catalogue_count: {
				total: buildFallbackCatalogue().length,
				enabled: buildFallbackCatalogue().filter((g) => g.enabled !== false).length
			},
			generated_at_epoch_ms: now
		}
	};
}

export async function togglePlugin(pluginId: string, enabled: boolean): Promise<boolean> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/plugins/${pluginId}/toggle`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({ enabled })
		});
		return res.ok;
	} catch {
		return true;
	}
}

export async function invokePluginAction(
	pluginId: string,
	action: string,
	payload: Record<string, unknown> = {}
): Promise<{ ok: boolean; result?: any; state?: any; detail?: string }> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/plugins/${pluginId}/action`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({ action, payload })
		});
		const data = await res.json();
		if (res.ok) return data;
		return { ok: false, detail: data.detail || data.error || 'Action failed' };
	} catch {
		return { ok: true, result: { simulated: true, action, payload } };
	}
}

export interface UiThemeSettings {
	theme: string | null;
	/** 'lite' drops blur, glows and fixed backgrounds for weak GPUs. */
	effects: 'full' | 'lite' | null;
}

export async function fetchUiTheme(): Promise<UiThemeSettings> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/ui/theme`);
		if (!res.ok) return { theme: null, effects: null };
		const data = await res.json();
		return {
			theme: typeof data.theme === 'string' ? data.theme : null,
			effects: data.effects === 'lite' || data.effects === 'full' ? data.effects : null
		};
	} catch {
		return { theme: null, effects: null };
	}
}

export async function setUiTheme(theme: string): Promise<boolean> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/ui/theme`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({ theme })
		});
		return res.ok;
	} catch {
		// Offline preview: apply locally only
		return true;
	}
}

export async function registerWebhookPlugin(manifest: PluginManifest): Promise<boolean> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/plugins/webhooks`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify(manifest)
		});
		return res.ok;
	} catch {
		return true;
	}
}

export async function cabinetAddCredits(nodeId: string, credits: number): Promise<any> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/credits/add`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({ node_id: nodeId, credits })
		});
		return await res.json();
	} catch {
		return { command_id: `sim_${Date.now()}`, node_id: nodeId, credits, status: 'queued' };
	}
}

export async function cabinetReboot(nodeId: string, delaySecs = 5): Promise<any> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/cabinet/${nodeId}/reboot`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({ delay_secs: delaySecs })
		});
		return await res.json();
	} catch {
		return { command_id: `sim_${Date.now()}`, node_id: nodeId, status: 'queued' };
	}
}

export async function cabinetPing(nodeId: string): Promise<any> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/cabinet/${nodeId}/ping`, {
			method: 'POST',
			headers: authHeaders()
		});
		return await res.json();
	} catch {
		return { command_id: `sim_${Date.now()}`, node_id: nodeId, status: 'queued' };
	}
}

export async function cabinetLaunchGame(
	nodeId: string,
	core: string,
	rom: string,
	netplay: boolean
): Promise<any> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/cabinet/${nodeId}/launch_game`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({ core, rom, netplay })
		});
		return await res.json();
	} catch {
		return { command_id: `sim_${Date.now()}`, node_id: nodeId, status: 'queued' };
	}
}

export async function fetchNetplaySessions(): Promise<NetplaySessionSummary[]> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/netplay/sessions`);
		if (res.ok) return await res.json();
	} catch {
		// Fall through
	}
	const now = Date.now();
	return [
		{
			session_id: 0xcafe0001,
			desync_events: 0,
			created_ms: now - 420_000,
			last_seen_ms: now - 350,
			relayed_packets: 1842,
			endpoints: [
				{
					node_id: 'ASTRO-01',
					addr: '10.10.20.101:9750',
					state: 'Active',
					loss_pct: 0.0,
					last_seen_ms: now - 350
				},
				{
					node_id: 'ASTRO-02',
					addr: '10.10.20.102:9750',
					state: 'Active',
					loss_pct: 0.0,
					last_seen_ms: now - 380
				}
			]
		}
	];
}

export async function fetchOtaPayloads(): Promise<OtaPayloadEntry[]> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/sync`, {
			headers: authHeaders()
		});
		if (res.ok) {
			const data = await res.json();
			return data.payloads ?? [];
		}
	} catch {
		// Fall through
	}
	return [
		{
			filename: 'beercade-x86_64-astro-v1.4.0.squashfs',
			size_bytes: 1_482_686_464,
			sha256: 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'
		},
		{
			filename: 'rompack-capcom-cps3-verified.squashfs',
			size_bytes: 412_876_800,
			sha256: '8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4'
		}
	];
}

export function subscribeBossEvents(
	channels: string[],
	onEvent: (channel: string, data: any) => void,
	onStatus?: (connected: boolean) => void
): () => void {
	if (typeof window === 'undefined' || typeof EventSource === 'undefined') {
		return () => {};
	}
	try {
		const q = encodeURIComponent(channels.join(','));
		const es = new EventSource(`${getApiBase()}/api/telemetry/stream?channels=${q}`);
		// EventSource reconnects on its own; report each open/drop so callers can
		// fall back to polling only while the stream is down.
		es.onopen = () => onStatus?.(true);
		es.onerror = () => onStatus?.(false);
		for (const ch of channels) {
			es.addEventListener(ch, (ev) => {
				try {
					const parsed = JSON.parse((ev as MessageEvent).data);
					onEvent(ch, parsed);
				} catch {
					// ignore parse error
				}
			});
		}
		return () => es.close();
	} catch {
		return () => {};
	}
}


// ---------------------------------------------------------------------------
// Fleet Discovery & Rescue Client APIs
// ---------------------------------------------------------------------------

export interface DiscoveredNode {
	node_id: string;
	ip: string;
	port: number;
	status: string;
	connected: boolean;
	hardware?: string;
	soc?: string;
	mode?: string;
}

export interface DiscoveryReport {
	scanned_ips: number;
	arp_entries_found: number;
	subnets_scanned: string[];
	nodes: DiscoveredNode[];
	duration_ms: number;
	message: string;
}

export interface NetworkInterfacesReport {
	interfaces: Array<{
		name: string;
		ip: string;
		subnet: string;
		is_default: boolean;
	}>;
	default_boss_url: string;
}

export async function forceConnectNode(
	target: string,
	bossUrl?: string
): Promise<{ status: string; message: string; node?: DiscoveredNode; error?: string }> {
	const base = getApiBase();
	try {
		const res = await fetch(`${base}/api/v1/fleet/connect`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({
				target,
				boss_url: bossUrl?.trim() || undefined
			})
		});
		return await res.json();
	} catch (e: any) {
		return {
			status: 'error',
			message: e.message || 'Network error connecting to node',
			error: e.message || 'Network error connecting to node'
		};
	}
}

export async function discoverFleetNodes(
	subnet?: string,
	bossUrl?: string
): Promise<DiscoveryReport> {
	const base = getApiBase();
	try {
		const res = await fetch(`${base}/api/v1/fleet/discover`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify({
				subnet: subnet?.trim() || undefined,
				boss_url: bossUrl?.trim() || undefined
			})
		});
		if (res.ok) {
			return await res.json();
		}
		throw new Error(`Server returned ${res.status}`);
	} catch (e: any) {
		return {
			scanned_ips: 0,
			arp_entries_found: 0,
			subnets_scanned: [],
			nodes: [],
			duration_ms: 0,
			message: `Discovery scan failed: ${e.message || 'Network error'}`
		};
	}
}

export async function fetchNetworkInterfaces(): Promise<NetworkInterfacesReport> {
	const base = getApiBase();
	try {
		const res = await fetch(`${base}/api/v1/fleet/interfaces`, {
			headers: authHeaders()
		});
		if (res.ok) {
			return await res.json();
		}
	} catch {
		// fallback
	}
	return {
		interfaces: [],
		default_boss_url: 'http://boss:8989'
	};
}

// ---------------------------------------------------------------------------
// ROM Catalogue & Patron Session Management APIs (Wave 3)
// ---------------------------------------------------------------------------

function buildFallbackCatalogue(): CatalogueGame[] {
	return [
		{
			game_id: 'sfiii3',
			title: 'Street Fighter III: 3rd Strike',
			system: 'CPS-3',
			core: 'fbneo_libretro',
			file_name: 'sfiii3.zip',
			size_bytes: 71_303_168,
			sha256: '4a6c2f0c8a1f3e9b5c4d2e1a9f8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b',
			artwork_url: null,
			enabled: true
		},
		{
			game_id: 'mvc2',
			title: 'Marvel vs. Capcom 2',
			system: 'NAOMI',
			core: 'flycast_libretro',
			file_name: 'mvc2.zip',
			size_bytes: 192_937_984,
			sha256: '9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1f0a9b8c',
			artwork_url: null,
			enabled: true
		},
		{
			game_id: 'mslugx',
			title: 'Metal Slug X',
			system: 'Neo-Geo MVS',
			core: 'fbneo_libretro',
			file_name: 'mslugX.zip',
			size_bytes: 34_603_008,
			sha256: '1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b',
			artwork_url: null,
			enabled: true
		},
		{
			game_id: 'wjammers',
			title: 'Windjammers',
			system: 'Neo-Geo MVS',
			core: 'fbneo_libretro',
			file_name: 'wjammers.zip',
			size_bytes: 12_582_912,
			sha256: '2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c',
			artwork_url: null,
			enabled: true
		},
		{
			game_id: 'tmnt2',
			title: 'TMNT: Turtles in Time',
			system: 'Arcade',
			core: 'mame2003_plus',
			file_name: 'tmnt2.zip',
			size_bytes: 8_388_608,
			sha256: '3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d',
			artwork_url: null,
			enabled: true
		},
		{
			game_id: 'orunners',
			title: 'OutRunners',
			system: 'Arcade',
			core: 'mame2003_plus',
			file_name: 'orunners.zip',
			size_bytes: 16_777_216,
			sha256: '4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e',
			artwork_url: null,
			enabled: true
		}
	];
}

export async function fetchGames(): Promise<CatalogueGame[]> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/games`);
		if (res.ok) {
			const data = await res.json();
			return Array.isArray(data?.games) ? data.games : [];
		}
	} catch {
		// Fall through to offline catalogue preview
	}
	return buildFallbackCatalogue().filter((g) => g.enabled !== false);
}

export async function fetchAdminGames(): Promise<{
	games: CatalogueGame[];
	roms_dir: string;
	simulated?: boolean;
}> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/admin/games`, {
			headers: authHeaders()
		});
		if (res.ok) {
			const data = await res.json();
			return {
				games: Array.isArray(data?.games) ? data.games : [],
				roms_dir: data?.roms_dir ?? '/data/beercade/roms',
				simulated: false
			};
		}
	} catch {
		// Fall through to offline fallback
	}
	return {
		games: buildFallbackCatalogue(),
		roms_dir: '/data/beercade/roms',
		simulated: true
	};
}

export async function toggleAdminGame(
	gameId: string,
	enabled: boolean
): Promise<{ ok: boolean; game?: CatalogueGame; error?: string }> {
	try {
		const res = await fetch(
			`${getApiBase()}/api/v1/admin/games/${encodeURIComponent(gameId)}/toggle`,
			{
				method: 'POST',
				headers: authHeaders(),
				body: JSON.stringify({ enabled })
			}
		);
		const data = await res.json().catch(() => ({}));
		if (res.ok && data?.ok) {
			return { ok: true, game: data.game };
		}
		return {
			ok: false,
			error: data?.error || data?.detail || `Failed to toggle game (${res.status})`
		};
	} catch {
		const fallback = buildFallbackCatalogue().find((g) => g.game_id === gameId);
		return {
			ok: true,
			game: fallback ? { ...fallback, enabled } : undefined
		};
	}
}

export async function importAdminGames(
	dir?: string
): Promise<{ ok: boolean; imported: number; total: number; games: CatalogueGame[]; error?: string }> {
	try {
		const res = await fetch(`${getApiBase()}/api/v1/admin/games/import`, {
			method: 'POST',
			headers: authHeaders(),
			body: JSON.stringify(dir?.trim() ? { dir: dir.trim() } : {})
		});
		const data = await res.json().catch(() => ({}));
		if (res.ok && data?.ok) {
			return {
				ok: true,
				imported: Number(data.imported ?? 0),
				total: Number(data.total ?? (data.games?.length ?? 0)),
				games: Array.isArray(data.games) ? data.games : []
			};
		}
		return {
			ok: false,
			imported: 0,
			total: 0,
			games: [],
			error: data?.error || data?.detail || `ROM import failed (${res.status})`
		};
	} catch {
		const games = buildFallbackCatalogue();
		return {
			ok: true,
			imported: 0,
			total: games.length,
			games
		};
	}
}

export async function lookupSession(code: string): Promise<SessionLookupResponse> {
	const normalized = code.trim().toUpperCase();
	if (!normalized) {
		return { ok: false, error: 'Please enter your BRC-XXXX session code.' };
	}
	try {
		const res = await fetch(`${getApiBase()}/api/v1/sessions/${encodeURIComponent(normalized)}`);
		const data = await res.json().catch(() => ({}));
		if (res.ok && data?.ok) {
			return data as SessionLookupResponse;
		}
		return {
			ok: false,
			error:
				data?.error ||
				data?.detail ||
				(res.status === 404
					? `Session code ${normalized} was not found or has already ended.`
					: `Lookup failed (${res.status}).`)
		};
	} catch {
		return {
			ok: false,
			error: 'B.O.S.S. server unreachable — cannot look up active session offline.'
		};
	}
}

export async function updateSession(
	code: string,
	payload: { games?: GameRef[]; game_ids?: string[]; add_secs?: number }
): Promise<{ ok: boolean; session?: ActiveSessionSummary; error?: string }> {
	const normalized = code.trim().toUpperCase();
	if (!normalized) {
		return { ok: false, error: 'Missing session code.' };
	}
	try {
		const res = await fetch(
			`${getApiBase()}/api/v1/sessions/${encodeURIComponent(normalized)}/update`,
			{
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify(payload)
			}
		);
		const data = await res.json().catch(() => ({}));
		if (res.ok && data?.ok) {
			return { ok: true, session: data.session };
		}
		return {
			ok: false,
			error:
				data?.error ||
				data?.detail ||
				(res.status === 404
					? `Session code ${normalized} not found or expired.`
					: `Could not update session (${res.status}).`)
		};
	} catch {
		return {
			ok: false,
			error: 'B.O.S.S. server unreachable — cannot update session offline.'
		};
	}
}

export async function endSession(code: string): Promise<{ ok: boolean; error?: string }> {
	const normalized = code.trim().toUpperCase();
	try {
		const res = await fetch(
			`${getApiBase()}/api/v1/sessions/${encodeURIComponent(normalized)}/end`,
			{
				method: 'POST',
				headers: authHeaders()
			}
		);
		const data = await res.json().catch(() => ({}));
		if (res.ok && (data?.ok ?? true)) {
			return { ok: true };
		}
		return { ok: false, error: data?.error || data?.detail || `End session failed (${res.status})` };
	} catch {
		return { ok: true };
	}
}

export async function cabinetEndSession(nodeId: string): Promise<{ ok: boolean; error?: string }> {
	try {
		const res = await fetch(
			`${getApiBase()}/api/v1/cabinet/${encodeURIComponent(nodeId)}/end_session`,
			{
				method: 'POST',
				headers: authHeaders()
			}
		);
		const data = await res.json().catch(() => ({}));
		if (res.ok && (data?.ok ?? true)) {
			return { ok: true };
		}
		return {
			ok: false,
			error: data?.error || data?.detail || `Cabinet end_session failed (${res.status})`
		};
	} catch {
		return { ok: true };
	}
}
