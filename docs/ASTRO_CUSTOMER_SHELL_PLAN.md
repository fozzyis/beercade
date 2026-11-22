# A.S.T.R.O. Customer Shell & B.O.S.S.-Driven Sessions — Plan

Status: Phase 1 & Phase 2 implemented (GA HEAD); out-of-game visual polish & hardware soak open. Owner split: A.S.T.R.O. + `shared` (Claude), B.O.S.S. (Antigravity).

## Goals

1. Zero customisation for players. ES-DE is removed from the cabinet; RetroArch's menu is unreachable.
2. A.S.T.R.O. holds **no game files at rest**. B.O.S.S. sends the files for a session (or for attract mode) and A.S.T.R.O. destroys them when they are no longer needed.
3. Attract mode is driven by B.O.S.S.: each time a cabinet enters attract, B.O.S.S. sends 2–3 random games from **different systems**.
4. A session shows the player's chosen games in a customer shell. Quitting a game returns to that list, never to a menu.
5. Players can change their games (and buy more time) at B.O.S.S. using their session code; B.O.S.S. pushes the update to the cabinet while time remains.
6. When time runs out, B.O.S.S. offers a top-up for a short grace window; files are only destroyed if the window closes without one.

## Session lifecycle (cabinet view)

```
          ┌──────────── StartAttract ────────────┐
          ▼                                       │
  [ATTRACT] ── StartSession ──► [STAGING] ──► [PICKER] ◄──┐
     ▲                          (download +    │ pick      │ exit combo /
     │                           verify)       ▼           │ game crash
     │                                      [IN_GAME] ─────┘
     │                                         │ time = 0 (from any state)
     │                                         ▼
     │                                     [EXPIRED] — "Time's up" + top-up
     │            UpdateSession(add_secs) ◄────┤      offer, grace countdown
     │            → back to PICKER             │ grace elapsed / EndSession
     └──────────── wipe session files ◄────────┘  → report SessionEnded, idle
                                                   until B.O.S.S. sends StartAttract
```

- **UpdateSession** may arrive in PICKER, IN_GAME or EXPIRED. New games are staged in the background; removed games are deleted once not running.
- On boot, and on any daemon start, `/data/beercade/session/` and `/data/beercade/attract/` are wiped before anything else (crash/power-loss safety).

## Protocol contract (`shared/crates/beercade-shared/src/models.rs` → `shared/proto/telemetry.ts`)

`shared/crates/beercade-shared/src/models.rs` is the canonical Rust definition,
and `cargo run -p beercade-shared --bin gen_ts_proto -- --check` (enforced in
CI) guarantees exact parity with `shared/proto/telemetry.ts`. Changes need both
owners.

```rust
pub struct GameRef {
    pub game_id: String,        // stable catalogue id
    pub title: String,
    pub system: String,         // "snes", "megadrive", "arcade", "psx", "pc", ...
    pub core: String,           // libretro core name, e.g. "snes9x"
    pub file_name: String,      // name to store as, e.g. "Donkey Kong Country.zip"
    pub size_bytes: u64,
    pub sha256: String,         // hex; cabinet refuses a mismatched file
    pub artwork_url: Option<String>,
}

// New / changed CabinetCommand variants
StartSession {
    session_id: String,
    session_code: String,       // what the player types at B.O.S.S. ("BRC-4KQ7")
    duration_secs: u32,
    topup_grace_secs: u32,      // e.g. 90
    games: Vec<GameRef>,
},
UpdateSession {
    session_id: String,
    games: Option<Vec<GameRef>>, // full replacement list when Some
    add_secs: u32,               // 0 = no time change
},
EndSession { session_id: String, reason: String }, // operator end / refund
StartAttract { games: Vec<GameRef>, secs_per_game: u32 },
```

Cabinet → B.O.S.S. (new `POST /api/v1/cabinet/:node_id/events`, node-authenticated):

```rust
pub enum CabinetEvent {
    SessionStaged   { session_id: String, ready_game_ids: Vec<String>, failed_game_ids: Vec<String> },
    GameStarted     { session_id: String, game_id: String },
    GameExited      { session_id: String, game_id: String, crashed: bool },
    SessionExpired  { session_id: String },            // grace window starts
    SessionEnded    { session_id: String, files_wiped: bool },
    ChangeRequested { session_id: String },            // player chose "Change my games"
    AttractNeeded,                                     // idle, no attract playlist
}
```

Game file delivery (B.O.S.S. side, node-authenticated):

`GET /api/v1/cabinet/:node_id/games/:game_id/file`
- Allowed only while `game_id` is in that node's **current** session or attract playlist; otherwise 403.
- Supports `Range` (resume after a dropped connection). Streams; no full-file buffering.
- Logged (node, game, session, bytes) for licence auditing.

## Already built: in-game overlay & `astro_rt` pause bridge

Designs: design canvas https://claude.ai/artifact/J4pSrtxcoEm2MNrAFD3ey3, `project/<theme>/Astro.dc.html` (taproom/retro, vector, bigsky, pinball; classic = current). They cover three in-game states only: **HUD**, **pause menu**, **time's up / CONTINUE?**.

Implemented in `astro/daemon/src/overlay.rs` + `input.rs` + `astro_rt` (`0002-astro-rt-vulkan-udev-input.patch`), drawn through RetroArch's OSD over its UDP command port (55355); venue theme from `GET /api/v1/ui/theme` sets the OSD colour; `astro_rt` runs 1000 Hz evdev polling and signals pause/menu requests over `/run/astro/menu.sock`.
- HUD lines at time milestones; toasts for credits/orders/scores.
- Pause menu (TAB, gamepad guide, or START+SELECT held), input isolated while open so ESC/hotkeys never reach RetroArch: RESUME, QUIT GAME (returns to `astro-shell` picker), ADD CREDITS, ORDER A DRINK, HIGH SCORES, NETPLAY, CALL STAFF, END SESSION (confirm twice).
- Time's up: game paused, `CONTINUE? N` countdown (`CONTINUE_SECS = 10`), more time resumes, otherwise the game closes and `astro-shell` presents the full-screen top-up window for `topup_grace_secs`.

## Still needs designs

The artboards don't cover the out-of-game screens the shell draws: **attract call-to-action, staging ("getting your games ready"), game picker, change-my-games (code + QR), full-screen time's up/top-up, session ended**. Ask the design agent for these four themes, matching the existing in-game artboards. The shell can be built against wireframes meanwhile.

## A.S.T.R.O. work — Implemented (Phase 1 & Phase 2, `bb34ebb`..`5978d03`)

Phase 1 — shell, attract, picker (**Implemented**)
1. `shared`: contract above + serde round-trip tests + `gen_ts_proto` TypeScript codegen (`shared/proto/telemetry.ts`).
2. Daemon `session` module (`astro/daemon/src/session.rs`): state machine above, countdown owned by the daemon, persisted atomically to `/data/beercade/session/state.json`.
3. Daemon `staging` module (`astro/daemon/src/staging.rs`): streaming `*.part` download, SHA-256 verification, LRU caching, wipe on boot/end. Files stay under `/data/beercade/{session,attract}/`.
4. **`astro-shell`** (`astro/shell`): resident Rust + SDL2 KMS customer screen driven via `/run/astro/shell.sock` (`shell_ipc.rs`), using `SIGSTOP`/`SIGCONT` DRM handoff so RetroArch gets exclusive KMS control without tearing down the shell.
5. RetroArch lockdown (`astro/rootfs/root/.config/retroarch/retroarch.cfg` + `astro/platform/x86_64/rootfs/...`): menu toggle unbound, Escape/quit unbound, `config_save_on_exit = false`, `pause_nonactive = false`.
6. Replace ES-DE: `astro-frontend.service` runs `astro-shell`; ES-DE removed from `NATIVE_BUILDS` and x86 images. *(Note: bundled dev/attract ROM files in `astro/roms/*.zip` remain in the repo for local testing and must be audited/cleaned before commercial deployment.)*
7. Tests: state-machine, streaming staging, and headless shell tests implemented in `astro-daemon` / `astro-shell`.

Phase 2 — change games & upsell (**Implemented**)
8. "Change my games" emits `ChangeRequested`; shell shows `BRC-XXXX` session code + QR and stays responsive until `UpdateSession` arrives.
9. `UpdateSession` handling: background staging of new games and atomic list swaps without killing the running game.
10. Top-up screen during `EXPIRED` mirrors the B.O.S.S. grace offer (`topup_grace_secs`).

## B.O.S.S. work — Implemented (`66b29f7`, `5978d03`)

See `docs/antigravity-boss-session-prompt.md` for the original brief; implemented in `boss/src/`:
- Game catalogue (`boss/src/catalogue.rs`): file scan + metadata + SHA-256 computation + default fallback entries.
- Session issuance & editing (`boss/src/plugins.rs`, `boss/src/routes.rs`, `website/src/routes/api/webhooks/stripe/+server.ts`): `BRC-XXXX` codes, `GET /api/v1/sessions/:code`, `POST /api/v1/sessions/:code/update`, `POST /api/v1/sessions/:code/end`.
- Attract playlists: multi-system random picks avoiding recent repeats (`catalogue.rs`).
- Node-authenticated `Range`-capable `GET /api/v1/cabinet/:node_id/games/:game_id/file` and `POST /api/v1/cabinet/:node_id/events`.
- Multiplexed command delivery over `WS_TELEMETRY` (`ws_telemetry.rs`) alongside HTTP fallback polling.

## Open items

1. **Out-of-game screen designs** (see "Still needs designs"). In-game designs are on the design canvas and implemented.
2. Licensing: confirm the venue's game licences allow per-session transfer to cabinets.
3. Large titles (PS2/GameCube ISOs, GBs): staging time at session start. Options: B.O.S.S. pre-stages at purchase time, or cap per-session size. Needs a decision before those systems are offered.
4. DATA partition size: it auto-grows to the disk, but cabinets need enough disk for the largest session + attract playlist.
5. Naming: plan says A.S.T.R.O.; confirm whether "ORION" is a rename or a separate unit type.
