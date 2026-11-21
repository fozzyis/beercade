## 2026-10-27T23:09:39Z

You are Worker M2 for Milestone 2 of B.E.E.R.C.A.D.E.
Working directory: d:\data\dev\beercade\.agents\teamwork\worker_m2

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

MANDATORY: Read the authoritative user request at:
d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md
Also read:
d:\data\dev\beercade\.agents\teamwork\PROJECT.md
d:\data\dev\beercade\.agents\teamwork\TEST_INFRA.md
Also read the detailed Explorer handoff reports:
- d:\data\dev\beercade\.agents\teamwork\explorer_m2_1\handoff.md
- d:\data\dev\beercade\.agents\teamwork\explorer_m2_2\handoff.md
- d:\data\dev\beercade\.agents\teamwork\explorer_m2_3\handoff.md

Your Scope for Milestone 2:
1. Requirement R5: Implement operator PIN rate limiting in boss/src/state.rs and boss/src/routes.rs.
   - In boss/src/state.rs: Implement `PinAttemptTracker` (tracked by client identifier, 5 failed attempts -> 60s lockout, resetting on success, clean expired records). Add `pub pin_tracker: tokio::sync::Mutex<PinAttemptTracker>` to `AppState`.
   - In boss/src/routes.rs: Update `verify_operator_pin` to extract client IP from headers (`CF-Connecting-IP`, `X-Forwarded-For`, `X-Real-IP`, `ConnectInfo<SocketAddr>`, fallback `"127.0.0.1"`).
     - If client is locked out: return 429 Too Many Requests with `Retry-After: <secs>` header and JSON `{"ok": false, "error": "rate_limited", "detail": "...", "retry_after_secs": u64}`.
     - If PIN matches: reset failure count for client, mint session token, return 200 OK with `{"ok": true, "token": token, "expires_at_epoch_ms": expires_at_epoch_ms, "venue": state.config.venue_name}`.
     - If PIN fails: increment failure count. If count reaches 5, trigger 60s lockout and return 429 with `Retry-After: 60` and `retry_after_secs: 60`. If count < 5, return 401 Unauthorized with `{"ok": false, "error": "invalid_pin", "detail": "Incorrect 6-digit operator PIN.", "remaining_attempts": remaining}`.
2. Requirement R6: Comprehensive Automated Tests for B.O.S.S. Server:
   - Add `tower = { version = "0.5", features = ["util"] }` under `[dev-dependencies]` in `boss/Cargo.toml`.
   - Refactor router construction into `pub fn build_router(state: SharedState) -> axum::Router` in `boss/src/routes.rs` so tests can test the exact route tree, and update `boss/src/main.rs` accordingly.
   - In `boss/src/state.rs`: Add unit tests for `PinAttemptTracker` (success reset, fail counting, lockout trigger, expiry).
   - In `boss/src/routes.rs`: Add comprehensive tests using `tower::ServiceExt::oneshot`:
     - F5 PIN rate limiting: valid PIN (200), bad PIN decrements remaining_attempts (401), 5th bad PIN triggers 429 with Retry-After header, locked out requests return 429, valid PIN resets failure count, multi-client isolation (client A lockout does not affect client B).
     - F6 Command plane: enqueuing commands (`/api/v1/cabinet/:node_id/reboot`, `/api/v1/credits/add`), polling commands (`GET /api/v1/cabinet/:node_id/commands`) returns commands in FIFO order with Dispatched status, second poll returns empty list, acknowledging command (`POST /api/v1/cabinet/:node_id/commands/ack`) updates status to Acknowledged / Failed with detail, error cases: 404 on unknown command ID, 403 on mismatched node ID.
3. Verification:
   - Run `cargo test -p boss-server` and verify that all existing tests + all new tests pass 100%.
   - Run `cargo test --workspace` to ensure no regressions in other crates.
   - STRICT CONSTRAINT: Do NOT run any image build commands (`make image-x86`, `mksquashfs`, Docker rootfs exports). Keep scope strictly on coding and testing.

Exclusive file ownership:
- `boss/Cargo.toml`
- `boss/src/state.rs`
- `boss/src/routes.rs`
- `boss/src/main.rs`
Do NOT modify files outside this list!

Write your handoff report to:
`d:\data\dev\beercade\.agents\teamwork\worker_m2\handoff.md`
Report your status and results back to the orchestrator via `send_message`.
