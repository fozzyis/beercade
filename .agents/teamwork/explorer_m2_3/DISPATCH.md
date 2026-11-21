# Dispatch: Explorer M2-3 (B.O.S.S. Test Architecture)
Target: Investigate test harness and test structure for B.O.S.S. server PIN rate limiting and Command Plane tests (R6).
See ORIGINAL_REQUEST.md, PROJECT.md, TEST_INFRA.md.

## 2026-10-27T23:00:44Z
Sender: bd770878-29b7-48ec-a1e1-b5265681a707
Content:
You are Explorer M2-3 for Milestone 2 of B.E.E.R.C.A.D.E.
Working directory: d:\data\dev\beercade\.agents\teamwork\explorer_m2_3

MANDATORY: Read the authoritative user request at:
d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md
Also read:
d:\data\dev\beercade\.agents\teamwork\PROJECT.md
d:\data\dev\beercade\.agents\teamwork\TEST_INFRA.md

Your focus:
Investigate test infrastructure and architecture for `boss-server` (Requirement R6).
Specifically investigate:
1. Existing test suite in `boss` (`cargo test -p boss-server`, `boss/src/routes.rs` tests, `boss/src/plugins.rs` tests, `boss/tests/` if any).
2. How the Axum app router (`boss/src/routes.rs::build_router` or `app()`) can be invoked in unit/integration tests using `tower::ServiceExt::oneshot` and `http::Request`.
3. How to construct test requests with fake client IP / `ConnectInfo<SocketAddr>` or headers if required by rate limiting.
4. Test cases required by `TEST_INFRA.md` for F5 (PIN rate limit) and F6 (Command queue/ACK):
   - Correct PIN returns 200 and token
   - Incorrect PIN returns 401 with remaining_attempts
   - 5th failed attempt triggers 429 Too Many Requests with Retry-After header
   - Subsequent requests within 60s return 429
   - Successful unlock resets the failure counter
   - Multi-client isolation (client A locked out does not block client B)
   - Command enqueue, queue FIFO order, poll, acknowledge, and invalid ACK error handling
5. Exact test organization (e.g. in `boss/src/routes.rs` tests module or `boss/tests/api_tests.rs`).

Write your findings to d:\data\dev\beercade\.agents\teamwork\explorer_m2_3\analysis.md and d:\data\dev\beercade\.agents\teamwork\explorer_m2_3\handoff.md.
When finished, notify the orchestrator using send_message.
Remember: DO NOT modify any source code files or run image builds. Explore and analyze only.
