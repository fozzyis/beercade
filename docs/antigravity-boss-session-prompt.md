You are working on B.O.S.S. (`boss/`) in the beercade repo (branch `main`). Another agent (Claude) is building the matching A.S.T.R.O. side at the same time. Read `docs/ASTRO_CUSTOMER_SHELL_PLAN.md` first; its "Protocol contract" section is the shared interface. Do not change the contract on your own: if something in it doesn't work for B.O.S.S., write the problem and your proposed change at the bottom of that plan under "Contract change requests" and carry on with the rest.

## What we're building

Cabinets (A.S.T.R.O.) will keep **no game files at rest**. B.O.S.S. owns the game library, decides what each cabinet plays, sends the files for each session and for attract mode, and tells the cabinet when a session starts, changes, or ends. The cabinet deletes the files when the session ends.

## Your tasks (B.O.S.S. only: `boss/`, plus `shared/` only where the contract says)

Phase 1
1. **Game catalogue.** Persisted list of games: `game_id`, title, system (snes, megadrive, arcade, psx, pc, ...), libretro core, file name, size, SHA-256 (computed on import, not trusted from input), optional artwork. Admin import from a library folder on the B.O.S.S. data partition. Admin page to list/enable/disable games.
2. **Game choice at purchase.** When a player buys a timed session (all existing paths: Square pass redemption, QR checkout, POS time slot), they pick their games from the enabled catalogue on the kiosk. Configurable max games per session (default 10). Each session gets a short, human-typeable **session code** (e.g. `BRC-4KQ7`, no ambiguous characters).
3. **StartSession with games.** Replace today's `StartSession { session_id, duration_secs }` enqueue (see `boss/src/plugins.rs`, timed pass activation) with the contract's `StartSession` carrying `session_code`, `topup_grace_secs` (setting, default 90) and `games: Vec<GameRef>`.
4. **File endpoint.** `GET /api/v1/cabinet/:node_id/games/:game_id/file`, node-token authenticated like the existing cabinet routes in `boss/src/routes.rs`. Only serve a game that is in that node's current session or current attract playlist (403 otherwise). Stream the file, support HTTP `Range`, log node/game/session/bytes.
5. **Events endpoint.** `POST /api/v1/cabinet/:node_id/events` accepting the contract's `CabinetEvent`. Update session state, broadcast to the admin/TV UIs over the existing SSE/WS channel.
6. **Attract mode, driven by B.O.S.S.** Whenever a node is idle (no session) — at boot/first heartbeat, after `SessionEnded`, or on `AttractNeeded` — enqueue `StartAttract` with 2–3 random enabled games, **each from a different system**, avoiding games used in that node's last few playlists. `secs_per_game` is a setting (default 90). Rotate with a fresh playlist every N minutes (setting, default 15).

Phase 2
7. **Change my games.** Kiosk entry "I'm already playing": player types their session code, sees their current games and time left, edits the list, confirms. B.O.S.S. enqueues `UpdateSession { games: Some(..), add_secs: 0 }` for that node. Only allowed while the session has time left or is inside its top-up grace window.
8. **Upsell.** On that same screen, if the player has free game slots or less than a configurable amount of time left (default 10 min), offer "Add 15/30 minutes" (prices from the existing pack/pricing config). Purchase → `UpdateSession { games: None, add_secs }`.
9. **Top-up on expiry.** On `SessionExpired`, show the top-up offer on the kiosk/TV for that node for `topup_grace_secs`. Purchase → `UpdateSession { add_secs }`. No purchase → `EndSession { session_id, reason: "expired" }`, then start attract (task 6).

## Constraints

- Match the existing code style in `boss/src` (Axum handlers, state in `state.rs`, plugin actions in `plugins.rs`) and in `boss/kiosk_ui` (SvelteKit, venue theme tokens).
- Keep every existing purchase path working; tests under `boss/tests/` must keep passing. Add tests for: catalogue import hashing, file endpoint auth (403 for a game not in the node's session), Range requests, attract picks from distinct systems, code look-up, expiry → top-up → UpdateSession, expiry → no top-up → EndSession.
- Don't touch `astro/`. Don't edit the contract types in `shared/` beyond what the plan specifies.
- No commits with AI attribution lines.

## Done means

Phase 1 merged with tests passing: a purchased session sends a `StartSession` with games, the file endpoint serves only those games, and idle nodes get rotating attract playlists from different systems. Then phase 2. Report what you built, what you changed in `shared/`, and any contract change requests.
