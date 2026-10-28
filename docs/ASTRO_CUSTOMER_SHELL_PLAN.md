# A.S.T.R.O. Customer Shell & B.O.S.S.-Driven Sessions — Plan

Status: approved direction, 2026-10-29. Owner split: A.S.T.R.O. + `shared` (Claude), B.O.S.S. (Antigravity).

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

## Protocol contract (`shared/crates/beercade-shared/src/models.rs`)

This section is the agreed interface between both sides. Changes need both owners.

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

## Already built: in-game overlay (commit e388456)

Designs: design canvas https://claude.ai/artifact/J4pSrtxcoEm2MNrAFD3ey3, `project/<theme>/Astro.dc.html` (taproom/retro, vector, bigsky, pinball; classic = current). They cover three in-game states only: **HUD**, **pause menu**, **time's up / CONTINUE?**.

Implemented in `astro/daemon/src/overlay.rs` + `input.rs`, drawn through RetroArch's OSD over its UDP command port (55355); venue theme from `GET /api/v1/ui/theme` sets the OSD colour; the launcher merges the overlay appendconfig with the GL fallback.
- HUD lines at time milestones; toasts for credits/orders/scores.
- Pause menu (TAB, gamepad guide, or START+SELECT held), evdev grabbed while open so ESC/hotkeys never reach RetroArch: RESUME, ADD CREDITS, ORDER A DRINK, HIGH SCORES, NETPLAY, CALL STAFF, END SESSION (confirm twice).
- Time's up: game paused, `CONTINUE? N` countdown (`CONTINUE_SECS = 10`), more time resumes, otherwise the game closes.

Limits to know: RetroArch's OSD is **one line of text in one colour**. The artboards' panels, glow, countdown ring and price buttons can't be drawn that way; the overlay is a faithful text version. The rc1 test image (built 2026-10-28 21:55) predates this commit, which is why no HUD showed.

Changes this plan makes to the overlay:
- Pause menu gains **QUIT GAME** (back to the game picker, session keeps running) above END SESSION; END SESSION keeps its meaning (ends the session for everyone, wipe files).
- Time's up: the in-game `CONTINUE?` window stays short (10 s) and just pauses the game; when it closes the shell takes over and shows the full top-up screen for the rest of `topup_grace_secs` from B.O.S.S. Files are wiped only when that grace ends.
- The menu combo is the cabinet's only way out of a game. Escape and RetroArch's menu are unbound in the base config as well (belt and braces for when the daemon isn't running).

## Still needs designs

The artboards don't cover the out-of-game screens the shell draws: **attract call-to-action, staging ("getting your games ready"), game picker, change-my-games (code + QR), full-screen time's up/top-up, session ended**. Ask the design agent for these four themes, matching the existing in-game artboards. The shell can be built against wireframes meanwhile.

## A.S.T.R.O. work (Claude)

Phase 1 — shell, attract, picker
1. `shared`: contract above + serde round-trip tests; keep the old `EndSession` JSON accepted for one release.
2. Daemon `session` module: state machine above, countdown owned by the daemon, persisted to `/data/beercade/session/state.json` so a reboot mid-session resumes the right time (files are re-fetched; they were wiped).
3. Daemon `staging` module: download to `*.part`, verify SHA-256, rename; concurrency 2; wipe on boot; delete with unlink + `fstrim` hint. Files never leave `/data/beercade/{session,attract}/`.
4. **`astro-shell`** (new crate, Rust + SDL2 on KMS): attract screen, "See the bar to play" call to action, game picker (big tiles, joystick/buttons only), time-left bar, "Change my games" screen with session code + QR, "Time's up" + top-up screen. Talks to the daemon over a Unix socket (JSON lines). Releases DRM before RetroArch starts, re-acquires after.
   - Visual design: the four venue themes from the design canvas (colours, fonts, glow treatment as in the in-game artboards); out-of-game screens per "Still needs designs".
5. RetroArch lockdown (x86 cfg): menu toggle unbound, Escape/quit key unbound, `config_save_on_exit = false`, `pause_nonactive = false`. Leaving a game goes only through the existing pause menu (QUIT GAME / END SESSION). Time warnings already come from the overlay HUD.
6. Replace ES-DE: `astro-frontend.service` runs `astro-shell`; drop ES-DE from `NATIVE_BUILDS` and the image; remove the seeded attract ROMs from the DATA payload (they now come from B.O.S.S.).
7. Tests: state-machine unit tests (expiry, grace, top-up mid-game, update while staging), staging tests against a fake HTTP server (bad hash, resume, 403), shell input tests headless.

Phase 2 — change games & upsell (needs B.O.S.S. phase 2)
8. "Change my games" emits `ChangeRequested`; shell shows the code/QR and keeps the picker usable until B.O.S.S. responds.
9. `UpdateSession` handling: stage new games in the background, swap lists atomically, never delete the running game.
10. Top-up screen during `EXPIRED` mirrors the offer B.O.S.S. shows.

## B.O.S.S. work (Antigravity) — summary

See `docs/antigravity-boss-session-prompt.md` for the full brief.
- Game catalogue (files + metadata + system), with SHA-256 computed on import.
- Game selection during purchase; session code issued per session.
- Session code look-up to edit a live session's games; upsell more time / extra slots there.
- Attract playlists: 2–3 random games from different systems per entry into attract; avoid repeating recent picks.
- Node-authenticated file endpoint and events endpoint above.
- Top-up offer during the grace window; `UpdateSession{add_secs}` on purchase, `EndSession` otherwise.

## Open items

1. **Out-of-game screen designs** (see "Still needs designs"). In-game designs are on the design canvas and implemented.
2. Licensing: confirm the venue's game licences allow per-session transfer to cabinets.
3. Large titles (PS2/GameCube ISOs, GBs): staging time at session start. Options: B.O.S.S. pre-stages at purchase time, or cap per-session size. Needs a decision before those systems are offered.
4. DATA partition size: it auto-grows to the disk, but cabinets need enough disk for the largest session + attract playlist.
5. Naming: plan says A.S.T.R.O.; confirm whether "ORION" is a rename or a separate unit type.
