# Graph Report - beercade  (2026-11-19)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 2037 nodes · 3941 edges · 148 communities (117 shown, 31 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS · INFERRED: 13 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `bb34ebb2`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- Community 0
- Community 1
- Community 2
- Community 3
- Community 4
- Community 5
- Community 6
- Community 7
- Community 8
- Community 9
- Community 10
- Community 11
- Community 12
- Community 13
- Community 14
- Community 15
- Community 16
- Community 17
- Community 18
- Community 19
- Community 20
- Community 21
- Community 22
- Community 23
- Community 24
- Community 25
- Community 26
- Community 27
- Community 28
- Community 29
- Community 30
- Community 31
- Community 32
- Community 33
- Community 34
- Community 35
- Community 36
- Community 37
- Community 38
- Community 39
- Community 40
- Community 41
- Community 42
- Community 43
- Community 44
- Community 45
- Community 46
- Community 47
- Community 48
- Community 49
- Community 50
- Community 51
- Community 52
- Community 53
- Community 54
- Community 55
- Community 56
- Community 57
- Community 58
- Community 59
- Community 60
- Community 61
- Community 62
- Community 63
- Community 64
- Community 65
- Community 66
- Community 67
- Community 68
- Community 69
- Community 70
- Community 71
- Community 72
- Community 73
- Community 74
- Community 75
- Community 76
- Community 77
- Community 78
- Community 79
- Community 80
- Community 81
- Community 82
- Community 83
- Community 84
- Community 85
- Community 86
- Community 87
- Community 88
- Community 89
- Community 90
- Community 91
- Community 92
- Community 93
- Community 94
- Community 95
- Community 96
- Community 97
- Community 98
- Community 99
- Community 100
- Community 101
- Community 102
- Community 103
- Community 104
- Community 105
- Community 106
- Community 107
- Community 108
- Community 109
- Community 110
- Community 111
- Community 112
- Community 113
- Community 114
- Community 115
- Community 116
- Community 117
- Community 118
- Community 119
- Community 120
- Community 121
- Community 122
- Community 123
- Community 124
- Community 125
- Community 126
- Community 127
- Community 128
- Community 129
- Community 130
- Community 131
- Community 132
- Community 133
- Community 134
- Community 135
- Community 136
- Community 137
- Community 138
- Community 139
- Community 140
- Community 141
- Community 142
- Community 143
- Community 144

## God Nodes (most connected - your core abstractions)
1. `AppState` - 45 edges
2. `GameRef` - 34 edges
3. `DaemonSharedState` - 31 edges
4. `test_app()` - 27 edges
5. `execute_cabinet_command()` - 23 edges
6. `getApiBase()` - 22 edges
7. `RequireRpcToken` - 21 edges
8. `Session` - 21 edges
9. `RollbackSession` - 20 edges
10. `RollbackSession<C>` - 20 edges

## Surprising Connections (you probably didn't know these)
- `g()` --references--> `GameRef`  [EXTRACTED]
  astro/daemon/src/shell_ipc.rs → shared/crates/beercade-shared/src/models.rs
- `RequireNodeToken` --references--> `NodeClaim`  [EXTRACTED]
  boss/src/auth.rs → shared/crates/beercade-shared/src/auth.rs
- `main()` --calls--> `format_bearer_header()`  [INFERRED]
  astro/daemon/src/main.rs → shared/crates/beercade-shared/src/auth.rs
- `verify_operator_pin()` --calls--> `constant_time_eq()`  [INFERRED]
  boss/src/routes.rs → shared/crates/beercade-shared/src/auth.rs
- `DaemonSharedState` --references--> `StationMode`  [EXTRACTED]
  astro/daemon/src/main.rs → shared/crates/beercade-shared/src/models.rs

## Import Cycles
- 2-file cycle: `boss/src/plugins.rs -> boss/src/state.rs -> boss/src/plugins.rs`

## Communities (148 total, 31 thin omitted)

### Community 0 - "Community 0"
Cohesion: 0.07
Nodes (70): ack_command(), ActiveGameInfo, apply_session_effects(), DaemonSharedState, execute_cabinet_command(), FakeRoot, find_rootfs_dir(), launch_game() (+62 more)

### Community 1 - "Community 1"
Cohesion: 0.05
Nodes (65): expired_and_ended(), frame(), g(), handle(), picker_lists_only_ready_games(), Arc, Client, RwLock (+57 more)

### Community 2 - "Community 2"
Cohesion: 0.07
Nodes (50): DEFAULT_GRACE_SECS, Effect, end_closes_running_game_and_wipes(), expiry_in_game_pauses_then_closes_then_wipes(), expiry_in_picker_has_no_game_to_pause(), g(), has_event(), missing_grace_uses_default() (+42 more)

### Community 3 - "Community 3"
Cohesion: 0.09
Nodes (49): announce(), bind_udp_relay(), build_announce_ack(), EndpointHealth, handle_datagram(), HEADER_LEN, http_roundtrip(), ingest_report() (+41 more)

### Community 4 - "Community 4"
Cohesion: 0.05
Nodes (44): website_src_lib_assets_astro_hero, website_src_lib_assets_bookings_hero, website_src_lib_assets_cocktail, website_src_lib_assets_machines_hero, website_src_lib_assets_menu_drinks_hero, website_src_lib_assets_menu_food_hero, website_src_lib_assets_tokens_hero, crossfadeToNextSong() (+36 more)

### Community 5 - "Community 5"
Cohesion: 0.09
Nodes (47): arc, boss_connect_handler(), BossConnectPayload, BossConnectResponse, build_discovery_router(), DEFAULT_DISCOVERY_PORT, DiscoveryState, node_info_handler() (+39 more)

### Community 6 - "Community 6"
Cohesion: 0.08
Nodes (34): cfg_carries_theme_colour_and_port(), cfg_path(), cfg_written_atomically(), continue_line(), CONTINUE_SECS, ctx(), DEFAULT_THEME, detail_and_back() (+26 more)

### Community 7 - "Community 7"
Cohesion: 0.07
Nodes (38): ABS_HAT0Y, ABS_Y, analog_stick_edges(), BTN_BASE3, BTN_BASE4, BTN_DPAD_DOWN, BTN_DPAD_UP, BTN_EAST (+30 more)

### Community 8 - "Community 8"
Cohesion: 0.05
Nodes (37): ANNOUNCE, ANNOUNCE_ACK, CLOCK_SYNC_INTERVAL_MS, CPU_TEMP_HYST_C, CPU_TEMP_LIMIT_C, crc32_incremental_matches_oneshot(), crc32_update(), DEFAULT_HISTORY_FRAMES (+29 more)

### Community 9 - "Community 9"
Cohesion: 0.13
Nodes (31): attract_dir(), bad_hash_leaves_nothing(), CONCURRENCY, fetches_and_verifies(), file_sha256(), forbidden_is_reported_per_game(), game(), game_path() (+23 more)

### Community 10 - "Community 10"
Cohesion: 0.17
Nodes (39): RequireRpcToken, ack_command(), AddCreditsRequest, cabinet_events(), cabinet_game_file(), cabinet_launch_game(), cabinet_ping(), cabinet_reboot() (+31 more)

### Community 11 - "Community 11"
Cohesion: 0.12
Nodes (35): asyncseekext, build_router(), build_router_with_relay(), parse_range_header(), PluginToggleRequest, RangeSpec, read_json_body(), Arc (+27 more)

### Community 12 - "Community 12"
Cohesion: 0.16
Nodes (32): check_free_space(), execute_ota_update(), FakeRoot, flash_inactive_slot(), Drop, Option, Path, PathBuf (+24 more)

### Community 13 - "Community 13"
Cohesion: 0.13
Nodes (19): Command, HalError, hwmon_temp(), Platform, point_extlinux_at(), read_millicelsius(), Display, Error (+11 more)

### Community 14 - "Community 14"
Cohesion: 0.15
Nodes (10): cfg_for(), crc32(), hard_resync_via_state_transfer(), late_input_below_resync_frame_is_ignored(), loopback_rollback_convergence(), now_epoch_ms(), now_epoch_us(), RollbackSession<C> (+2 more)

### Community 15 - "Community 15"
Cohesion: 0.13
Nodes (21): CatalogEntry, compute_sha256(), GameCatalogue, Default, HashMap, Option, Path, PathBuf (+13 more)

### Community 16 - "Community 16"
Cohesion: 0.18
Nodes (30): ActiveTimedSession, AlertDeliveryRecord, AlertingState, AlertSinkConfig, AstroGameTimeTier, BracketMatch, CreditPackTier, LightingCueLog (+22 more)

### Community 17 - "Community 17"
Cohesion: 0.08
Nodes (22): algorithm, astro_input_log_level, assign_port_if_needed(), astro_input_drain(), astro_input_poll_frame(), astro_input_start(), evdev_thread_main(), log_msg() (+14 more)

### Community 18 - "Community 18"
Cohesion: 0.17
Nodes (27): authHeaders(), buildFallbackNodes(), cabinetAddCredits(), cabinetLaunchGame(), cabinetPing(), cabinetReboot(), clearOperatorToken(), DiscoveredNode (+19 more)

### Community 19 - "Community 19"
Cohesion: 0.07
Nodes (26): compilerOptions, allowJs, checkJs, esModuleInterop, forceConsistentCasingInFileNames, moduleResolution, resolveJsonModule, rewriteRelativeImportExtensions (+18 more)

### Community 20 - "Community 20"
Cohesion: 0.13
Nodes (16): Config, EDGE_BUFFER_CAP, OPERATOR_SESSION_TTL_MS, PIN_LOCKOUT_MS, PIN_LOCKOUT_SECS, PIN_MAX_ATTEMPTS, PIN_RECORD_TTL_MS, PinFailureResult (+8 more)

### Community 21 - "Community 21"
Cohesion: 0.10
Nodes (23): eslint, eslint-config-prettier, @eslint/js, eslint-plugin-svelte, globals, prettier, prettier-plugin-svelte, prettier-plugin-tailwindcss (+15 more)

### Community 22 - "Community 22"
Cohesion: 0.18
Nodes (19): cpu_plan(), format_cpu_list(), hold_pm_qos(), main(), online_cpus(), parse_cpu_list(), partition_cpus(), plan_for_eight_cpus() (+11 more)

### Community 23 - "Community 23"
Cohesion: 0.09
Nodes (21): CabinetCommand, CommandAck, CommandRecord, CommandStatus, Acknowledged, Dispatched, Failed, Queued (+13 more)

### Community 24 - "Community 24"
Cohesion: 0.16
Nodes (18): HealthState, NetplayConfig, NetplayHealthReport, RollbackSession, Arc, HashMap, Receiver, String (+10 more)

### Community 25 - "Community 25"
Cohesion: 0.13
Nodes (19): asyncwriteext, Body, list_payloads_handler(), MAX_PAYLOAD_BYTES, payload_download_handler(), payload_upload_handler(), HeaderMap, Json (+11 more)

### Community 26 - "Community 26"
Cohesion: 0.10
Nodes (20): svelte, vite, name, private, type, version, @fontsource/archivo, @fontsource/bungee (+12 more)

### Community 27 - "Community 27"
Cohesion: 0.10
Nodes (20): AdminNodeEntry, AdminOverview, CabinetCommand, CommandAck, CommandRecord, CreditCounters, ErrorEvent, HighScoreEntry (+12 more)

### Community 28 - "Community 28"
Cohesion: 0.10
Nodes (21): devDependencies, eslint, eslint-config-prettier, @eslint/js, eslint-plugin-svelte, globals, prettier, prettier-plugin-svelte (+13 more)

### Community 29 - "Community 29"
Cohesion: 0.14
Nodes (14): B.E.E.R.C.A.D.E. Emulation QA & Frame-Time Telemetry Benchmark Harness…, os, pathlib, re, B.E.E.R.C.A.D.E. Phase 2.1 Full-Stack Mesh & Telemetry Test Harness Validates…, Tier 1 Feature Tests: BCM2712 Firmware & Boot (Requirement R1, AC 1, AC 2) Test…, Automated Verification Harness for Acceptance Criteria 1 & 2: - AC 1: The OS…, signal (+6 more)

### Community 30 - "Community 30"
Cohesion: 0.16
Nodes (13): ConsumerState, tail, ProducerState, head, SpscRing, consumer_, dropped_, kMask (+5 more)

### Community 31 - "Community 31"
Cohesion: 0.10
Nodes (20): devDependencies, @fontsource/archivo, @fontsource/bungee, @fontsource/chakra-petch, @fontsource/jetbrains-mono, @fontsource/libre-franklin, @fontsource/major-mono-display, @fontsource/racing-sans-one (+12 more)

### Community 32 - "Community 32"
Cohesion: 0.14
Nodes (10): BossPlugin, PluginCategory, PluginDescriptor, PluginManifest, PluginRegistry, PluginUiPanelSpec, Arc, Path (+2 more)

### Community 33 - "Community 33"
Cohesion: 0.16
Nodes (19): channel_stream(), BusMessage, Option, Receiver, Result, SharedState, State, String (+11 more)

### Community 34 - "Community 34"
Cohesion: 0.16
Nodes (11): AppState, EdgeItem, BusMessage, ErrorEvent, HighScoreEntry, NodeTelemetry, RwLock, TokenTransaction (+3 more)

### Community 35 - "Community 35"
Cohesion: 0.14
Nodes (13): DeterministicCore, manual_guest(), NEUTRAL_INPUT, rollback_with_history_gap_escalates(), Box, Send, Sync, StateLoad (+5 more)

### Community 36 - "Community 36"
Cohesion: 0.15
Nodes (15): axum, RequireNodeToken, Result, Self, SharedState, ingest_telemetry(), report_error(), ErrorEvent (+7 more)

### Community 37 - "Community 37"
Cohesion: 0.22
Nodes (4): AlertingSinksPlugin, Result, SharedState, Value

### Community 38 - "Community 38"
Cohesion: 0.12
Nodes (16): author, builtin, capabilities, category, description, enabled, id, name (+8 more)

### Community 39 - "Community 39"
Cohesion: 0.15
Nodes (7): Test automated QEMU BCM2712 boot verification harness (AC 1 & AC 2)., Test that bcm2712-rpi-5-b.dtb has valid FDT magic 0xd00dfeed and proper size., Test that bcm2712-rpi-5-b.dtb exposes model='Raspberry Pi 5' and BCM2712…, Test that bcm2712-d-rpi-5-b.dtb exists and contains valid D0 stepping…, Test that bcm2712_boss_defconfig contains all required architectural symbols., TestBcm2712Boot, QemuBcm2712Verifier

### Community 40 - "Community 40"
Cohesion: 0.23
Nodes (14): AVAILABLE_THEMES, DEFAULT_THEME, get_theme(), is_known(), load_initial(), IntoResponse, Json, SharedState (+6 more)

### Community 41 - "Community 41"
Cohesion: 0.14
Nodes (5): drift_controller_converges_and_is_jury_stable(), DriftController, Default, Self, synthetic_core_is_deterministic_and_order_sensitive()

### Community 42 - "Community 42"
Cohesion: 0.17
Nodes (12): auth, handle_socket(), IntoResponse, SharedState, State, ws_telemetry_handler(), models, pluginevent (+4 more)

### Community 43 - "Community 43"
Cohesion: 0.23
Nodes (9): active_slot_from_cmdline_then_mounts(), arm_thermals_read_zones_and_tolerate_missing_gpu(), FakeRoot, missing_sensor_is_an_error_not_a_fake_reading(), Drop, PathBuf, substrings_do_not_fake_a_slot_match(), unknown_environment_defaults_to_slot_a() (+1 more)

### Community 44 - "Community 44"
Cohesion: 0.16
Nodes (10): CabinetCommand, CommandAck, CreditCounters, ErrorEvent, HighScoreEntry, InputJitterStats, NetplayStats, NodeTelemetry (+2 more)

### Community 45 - "Community 45"
Cohesion: 0.20
Nodes (5): website_src_lib_assets_favicon, website_src_lib_assets_hero_bg, displayMachines, displayMinutes, displayPrice

### Community 46 - "Community 46"
Cohesion: 0.25
Nodes (6): make_dummy_telemetry(), NodeTelemetry, Self, test_thermal_alert_plugin_formats_missing_sensor_as_na(), test_thermal_alert_plugin_with_none_thermals(), test_thermal_alert_plugin_with_normal_thermals()

### Community 47 - "Community 47"
Cohesion: 0.27
Nodes (11): fetchUiTheme(), setUiTheme(), subscribeBossEvents(), applyEffects(), applyTheme(), DEFAULT_THEME, initTheme(), isKnown() (+3 more)

### Community 48 - "Community 48"
Cohesion: 0.15
Nodes (4): RfidPeripheralsPlugin, Client, RwLock, WebhookPlugin

### Community 49 - "Community 49"
Cohesion: 0.19
Nodes (6): ActiveNodeSession, NodeCabinetState, String, Vec, VecDeque, HashSet

### Community 50 - "Community 50"
Cohesion: 0.26
Nodes (12): ctypes, mmap, check_jit_alignment(), check_mprotect_alignment(), check_page_size(), log(), main(), AC_16K_1: System page size must be 16384. (+4 more)

### Community 51 - "Community 51"
Cohesion: 0.17
Nodes (6): boss_kiosk_ui_src_lib_assets_favicon, boss_kiosk_ui_src_lib_theme_bigsky, boss_kiosk_ui_src_lib_theme_controls, boss_kiosk_ui_src_lib_theme_effects, boss_kiosk_ui_src_lib_theme_pinball, boss_kiosk_ui_src_lib_theme_vector

### Community 52 - "Community 52"
Cohesion: 0.21
Nodes (8): stripe, buildIcsCalendar(), GET(), POST(), ALLOWED_TIERS, ASTRO_GAME_TIME_PASSES, computeBonusPct(), POST()

### Community 53 - "Community 53"
Cohesion: 0.17
Nodes (11): AR, CC, CFLAGS, CXX, CXXFLAGS, LDFLAGS, PKG_CONFIG_LIBDIR, PKG_CONFIG_PATH (+3 more)

### Community 54 - "Community 54"
Cohesion: 0.18
Nodes (8): constant_time_eq(), extract_bearer_token(), format_bearer_header(), NodeClaim, Option, String, validate_token(), super

### Community 55 - "Community 55"
Cohesion: 0.26
Nodes (9): clean_log(), finish(), has_fail(), hostpath(), MSYS2_ARG_CONV_EXCL, MSYS_NO_PATHCONV, running(), send() (+1 more)

### Community 56 - "Community 56"
Cohesion: 0.17
Nodes (12): scripts, build, check, check:watch, dev, format, gen, generate-types (+4 more)

### Community 57 - "Community 57"
Cohesion: 0.33
Nodes (3): FrameHistory, FrameSlot, ring_buffer_wraparound()

### Community 58 - "Community 58"
Cohesion: 0.38
Nodes (9): Packet, packet_roundtrip_all_variants(), put_addr(), put_u16(), put_u32(), put_u64(), put_u8(), SocketAddr (+1 more)

### Community 59 - "Community 59"
Cohesion: 0.18
Nodes (10): BossEventName, BossWebhookDelivery, CreditTransactionPayload, HighScorePayload, PluginActionRequest, PluginActionResponse, PluginCategory, PluginEvent (+2 more)

### Community 60 - "Community 60"
Cohesion: 0.24
Nodes (10): BROADCAST_INTERVAL, cache_immutable_assets(), main(), Arc, Duration, Request, Response, venue_broadcaster() (+2 more)

### Community 61 - "Community 61"
Cohesion: 0.20
Nodes (8): AlertFeedItem, ConnectionState, STALENESS_MS, CommandRecord, NodeOfflineAlert, NodeOnlineAlert, nodes, totalCreditsToday

### Community 62 - "Community 62"
Cohesion: 0.47
Nodes (4): Cursor, Cursor<'a>, Option, take_addr()

### Community 63 - "Community 63"
Cohesion: 0.27
Nodes (6): NetplayError, Display, Error, Formatter, From, Result

### Community 64 - "Community 64"
Cohesion: 0.24
Nodes (5): buildFallbackPlugins(), fetchPlugins(), refreshData(), triggerFanfareBanner(), n()

### Community 65 - "Community 65"
Cohesion: 0.20
Nodes (10): type, default, type, type, properties, author, builtin, description (+2 more)

### Community 66 - "Community 66"
Cohesion: 0.20
Nodes (10): type, icon, summary_metric_key, tab_label, ui_panel, type, type, properties (+2 more)

### Community 67 - "Community 67"
Cohesion: 0.29
Nodes (10): extract_client_ip(), FleetDiscoverRequest, ForceConnectRequest, PinUnlockRequest, RebootRequest, HeaderMap, Option, SocketAddr (+2 more)

### Community 68 - "Community 68"
Cohesion: 0.22
Nodes (5): CabinetCommand, CommandRecord, SseChannel, .ALL, Serialize

### Community 69 - "Community 69"
Cohesion: 0.40
Nodes (9): check_msix_routing(), check_rp1_driver_loaded(), check_rp1_pcie(), log(), main(), AC_RP1_1: RP1 must appear as a PCIe device with PCI ID 1de4:0001, Verify rp1_pci kernel module is loaded and bound., AC_RP1_2: Verify MSI-X interrupts are routed via bcm2712-mip to GIC SPI… (+1 more)

### Community 70 - "Community 70"
Cohesion: 0.31
Nodes (8): card_driver(), DRM_DEVICE, is_fb_driver(), probe_card(), SDL_AUDIODRIVER, SDL_KMSDRM_DEVICE_INDEX, SDL_VIDEODRIVER, astro_kms_launcher.sh script

### Community 71 - "Community 71"
Cohesion: 0.31
Nodes (8): FLUSH_INTERVAL, MAX_CONSECUTIVE_FAILURES, requeue_on_reject(), Arc, Duration, run_edge_forwarder(), EdgeBatchEnvelope, vecdeque

### Community 72 - "Community 72"
Cohesion: 0.31
Nodes (5): PluginEvent, CommandRecord, ErrorEvent, HighScoreEntry, TokenTransaction

### Community 73 - "Community 73"
Cohesion: 0.22
Nodes (6): assemble_and_flash_rootfs.sh script, build_all.sh script, build_cores.sh script, PATH, build_mesa.sh script, build_retroarch.sh script

### Community 74 - "Community 74"
Cohesion: 0.22
Nodes (8): CC, CFLAGS, CXX, CXXFLAGS, LDFLAGS, PKG_CONF_PATH, PKG_CONFIG_LIBDIR, PKG_CONFIG_SYSROOT_DIR

### Community 75 - "Community 75"
Cohesion: 0.33
Nodes (3): backoffMs(), VenueStore, VenueTelemetryReport

### Community 76 - "Community 76"
Cohesion: 0.32
Nodes (7): argparse, datetime, json, load_json_result(), main(), Run a verification script and capture its exit code and JSON result., run_script()

### Community 77 - "Community 77"
Cohesion: 0.25
Nodes (6): ROOT, TEXT, ref_node_fs, ref_node_path, ref_node_url, ref_node_zlib

### Community 78 - "Community 78"
Cohesion: 0.25
Nodes (8): items, type, enum, type, capabilities, subscribed_events, items, type

### Community 81 - "Community 81"
Cohesion: 0.46
Nodes (7): check_pisp_v4l2(), check_v3d_drm(), log(), main(), AC_GPU_2: pisp_be must be registered as a V4L2 memory-to-memory device., AC_GPU_1: Verify V3D DRM render node exists and V3D driver is bound., run()

### Community 82 - "Community 82"
Cohesion: 0.39
Nodes (7): EdgeBatchEnvelope, EdgeIngestResult, extractToken(), POST(), runChunked(), tokensMatch(), validateEnvelope()

### Community 83 - "Community 83"
Cohesion: 0.29
Nodes (7): scripts, build, check, check:watch, dev, prepare, preview

### Community 84 - "Community 84"
Cohesion: 0.29
Nodes (6): description, $id, required, $schema, title, type

### Community 87 - "Community 87"
Cohesion: 0.43
Nodes (4): PinAttemptRecord, PinAttemptTracker, HashMap, Option

### Community 88 - "Community 88"
Cohesion: 0.43
Nodes (6): banner(), die(), GNUPGHOME, in_root(), MTOOLS_SKIP_CHECK, container_build.sh script

### Community 89 - "Community 89"
Cohesion: 0.29
Nodes (6): ARCH, CC, CFLAGS, CXX, CXXFLAGS, LDFLAGS

### Community 90 - "Community 90"
Cohesion: 0.29
Nodes (6): ARCH, CFLAGS, CROSS_COMPILE, CXXFLAGS, LDFLAGS, build_kernel.sh script

### Community 91 - "Community 91"
Cohesion: 0.29
Nodes (6): ARCH, CFLAGS, CROSS_COMPILE, CXXFLAGS, LDFLAGS, build_kernel_pi5.sh script

### Community 92 - "Community 92"
Cohesion: 0.29
Nodes (4): latencyColor, npuRatio, secondsSinceHeartbeat, displayValue

### Community 93 - "Community 93"
Cohesion: 0.43
Nodes (6): fallbackStream(), GAMES, GET(), jittered(), sseHeaders(), syntheticNode()

### Community 94 - "Community 94"
Cohesion: 0.33
Nodes (5): AUDIODEV, MESA_LOADER_DRIVER_OVERRIDE, SDL_AUDIODRIVER, SDL_VIDEODRIVER, boot_esde.sh script

### Community 95 - "Community 95"
Cohesion: 0.33
Nodes (4): @sveltejs/adapter-cloudflare, @sveltejs/adapter-static, @tailwindcss/vite, ref_vite

### Community 96 - "Community 96"
Cohesion: 0.33
Nodes (6): astro-daemon, astro-shell, astro-tune, beercade-hal, beercade-shared, boss-server

### Community 97 - "Community 97"
Cohesion: 0.33
Nodes (5): ref_fs, ref_path, dir, fs, path

### Community 98 - "Community 98"
Cohesion: 0.47
Nodes (5): sha2, compute_sha256(), String, test_sha256(), verify_sha256()

### Community 99 - "Community 99"
Cohesion: 0.33
Nodes (5): build_esde_deps.sh script, PKG_CONFIG_LIBDIR, PKG_CONFIG_PATH, PKG_CONFIG_SYSROOT_DIR, build_frontend.sh script

### Community 100 - "Community 100"
Cohesion: 0.40
Nodes (4): CFLAGS, CXXFLAGS, LDFLAGS, build_x86_64.sh script

### Community 101 - "Community 101"
Cohesion: 0.40
Nodes (5): CommandStatus, Acknowledged, Dispatched, Failed, Queued

### Community 102 - "Community 102"
Cohesion: 0.60
Nodes (4): banner(), MSYS_NO_PATHCONV, build_image.sh script, winpath()

### Community 103 - "Community 103"
Cohesion: 0.40
Nodes (4): BL31, CROSS_COMPILE, ROCKCHIP_TPL, build_uboot.sh script

### Community 104 - "Community 104"
Cohesion: 0.60
Nodes (4): banner(), MSYS_NO_PATHCONV, build_image.sh script, winpath()

### Community 105 - "Community 105"
Cohesion: 0.60
Nodes (4): banner(), die(), MTOOLS_SKIP_CHECK, container_build.sh script

### Community 106 - "Community 106"
Cohesion: 0.40
Nodes (5): CommandStatus, Acknowledged, Dispatched, Failed, Queued

### Community 107 - "Community 107"
Cohesion: 0.50
Nodes (3): MESA_SHADER_CACHE_DIR, shader_precompile.sh script, VK_PIPELINE_CACHE_PATH

### Community 109 - "Community 109"
Cohesion: 0.83
Nodes (3): get_max_temp(), set_fan_duty(), pwm_fan.sh script

### Community 111 - "Community 111"
Cohesion: 0.50
Nodes (3): cstddef, cstdint, type_traits

### Community 112 - "Community 112"
Cohesion: 0.50
Nodes (4): DeviceState, assigned_port, fd, slot

### Community 113 - "Community 113"
Cohesion: 0.50
Nodes (4): description, pattern, type, id

### Community 114 - "Community 114"
Cohesion: 0.50
Nodes (4): description, minLength, type, name

### Community 115 - "Community 115"
Cohesion: 0.50
Nodes (4): version, description, pattern, type

### Community 116 - "Community 116"
Cohesion: 0.50
Nodes (3): PORT, server, ref_node_http

### Community 117 - "Community 117"
Cohesion: 0.67
Nodes (3): CacheLine, pad, byte

### Community 118 - "Community 118"
Cohesion: 0.67
Nodes (3): online_from_root(), PathBuf, tmp()

### Community 120 - "Community 120"
Cohesion: 0.67
Nodes (3): enum, type, category

### Community 121 - "Community 121"
Cohesion: 0.67
Nodes (3): default, type, enabled

### Community 122 - "Community 122"
Cohesion: 0.67
Nodes (3): webhook_url, format, type

### Community 124 - "Community 124"
Cohesion: 0.67
Nodes (3): allowScripts, esbuild@0.28.1, workerd@1.20260921.1

## Knowledge Gaps
- **476 isolated node(s):** `Platform`, `DiscoveredNode`, `DiscoveryReport`, `NetworkInterfacesReport`, `UiThemeSettings` (+471 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 806 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **31 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `AppState` connect `Community 34` to `Community 32`, `Community 68`, `Community 71`, `Community 15`, `Community 49`, `Community 20`, `Community 87`, `Community 60`?**
  _High betweenness centrality (0.024) - this node is a cross-community bridge._
- **Why does `DaemonSharedState` connect `Community 0` to `Community 1`, `Community 2`, `Community 5`?**
  _High betweenness centrality (0.016) - this node is a cross-community bridge._
- **Why does `FrameHistory` connect `Community 57` to `Community 8`, `Community 41`, `Community 14`, `Community 24`, `Community 58`?**
  _High betweenness centrality (0.015) - this node is a cross-community bridge._
- **What connects `Platform`, `DiscoveredNode`, `DiscoveryReport` to the rest of the system?**
  _476 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 0` be split into smaller, more focused modules?**
  _Cohesion score 0.07004103967168263 - nodes in this community are weakly interconnected._
- **Should `Community 1` be split into smaller, more focused modules?**
  _Cohesion score 0.05171907140758154 - nodes in this community are weakly interconnected._
- **Should `Community 2` be split into smaller, more focused modules?**
  _Cohesion score 0.06963470319634703 - nodes in this community are weakly interconnected._