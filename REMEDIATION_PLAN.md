# B.E.E.R.C.A.D.E. Remediation Plan

This plan addresses all critical and high-priority issues identified in the architecture review. The work will be executed concurrently by two specialized subagents.

## Track 1: Systems & Backend (Rust Team)
**Targeting:** `astro-daemon`, `BOSS` (backend core), and Deployment config.

1.  **Reliability: A/B Boot Swap Bug (C2)**
    *   **Location:** `astro-daemon/src/ota.rs`
    *   **Action:** Refactor `swap_bootloader_target` to use a safe swapping mechanism (e.g., using a temporary placeholder like `<SLOT_A_TEMP>`) so that the slot strings don't overwrite each other during sequential replacement.
2.  **Security: OTA Integrity Check (C1)**
    *   **Location:** `astro-daemon/src/ota.rs` and `BOSS/src/ota_sync.rs`
    *   **Action:** 
        *   Update `BOSS` upload handler to compute and store a SHA-256 hash (as a `.sha256` file) when an OTA payload is received.
        *   Update `BOSS` download handler to serve this hash via an `X-OTA-SHA256` header.
        *   Update `astro-daemon` to compute the SHA-256 hash of the downloaded payload in the staging area and compare it against the header *before* invoking `dd` to flash the inactive slot.
3.  **Operational Risk: Hardcoded Secrets (C3)**
    *   **Location:** `BOSS/docker-compose.yml`
    *   **Action:** Remove the `:-beercade_node_secret` and `:-beercade_rpc_secret` fallbacks to enforce strict failure if secrets aren't provided via environment variables in production.
4.  **Quality Gate: Testing (H1/H2)**
    *   **Location:** `BOSS/src/*.rs`
    *   **Action:** Write unit and basic integration tests for core modules (`auth`, `ota_sync`, `netplay_relay`).

## Track 2: Frontend & Tooling (Web Team)
**Targeting:** `website` (SvelteKit) and cross-boundary schemas.

1.  **Data Integrity: Schema Drift (M1)**
    *   **Location:** `BOSS/src/models.rs` and `website/src/lib/types/telemetry.ts`
    *   **Action:** Introduce `typeshare` (or a custom schema extraction script). Annotate the Rust models in `BOSS/src/models.rs` and configure a build step (e.g., in a `package.json` script or `Makefile`) to automatically generate the TypeScript types, replacing the manual mirroring.
2.  **UX: Mock Payment Endpoint (H3)**
    *   **Location:** `website/src/routes/api/checkout/token-pack/+server.ts`
    *   **Action:** Integrate the official `stripe` Node.js SDK. Replace the mock checkout logic with a real Stripe Checkout Session creation, expecting a `STRIPE_SECRET_KEY` in the environment, and return the session URL for the frontend to redirect the user to.
