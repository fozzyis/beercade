# E2E Test Infra: B.E.E.R.C.A.D.E. RC1 Blockers

## Test Philosophy
- Opaque-box, requirement-driven. Derived from `ORIGINAL_REQUEST.md` and user-facing contracts.
- Strictly ZERO image builds or external OS packaging (no `make image-x86`, `mksquashfs`, Docker).
- Zero external dependencies: Uses standard library RAII `FakeRoot` (`std::env::temp_dir()`) and in-memory Axum listener (`127.0.0.1:0`).

## Feature Inventory & Test Matrix
| # | Feature | Tier 1 (Feature) | Tier 2 (Boundary) | Tier 3 (Cross-Feature) | Tier 4 (Real-World) |
|---|---------|:----------------:|:-----------------:|:----------------------:|:-------------------:|
| F1 | Mandatory OTA SHA-256 (R1) | 5 tests | 5 tests | Pairwise with Network/FS | Real-World Flashing Scenario |
| F2 | Atomic Credit Persistence (R2) | 5 tests | 5 tests | Pairwise with Rapid Coin Ingest | Power-Loss Crash Recovery |
| F3 | Remove Static IP Conflicts (R3) | 5 tests | 5 tests | Pairwise with Multi-Cabinet DHCP | Dual-Cabinet LAN Discovery |
| F4 | Clean Thermal Contract (R4) | 5 tests | 5 tests | Pairwise with BOSS Heartbeat Ingest | Sensorless VM Heartbeat |
| F5 | Operator PIN Rate Limiting (R5) | 5 tests | 5 tests | Pairwise with Multi-Client Origins | Brute-Force Attack Defense |
| F6 | Command Dispatcher & ACK (R6) | 5 tests | 5 tests | Pairwise with Credit Add & Game Launch | Operator Remote Command Loop |

## Test Architecture
- **Test Runner**: `cargo test` across workspace (`--workspace` or per-crate `-p`).
- **Pass/Fail Semantics**: 100% test pass with exit code 0.
- **Directory Layout**:
  - `shared/crates/beercade-shared/src/models.rs`: Unit tests for telemetry validation, serde, command models.
  - `boss/src/routes.rs` & `boss/tests/`: Integration & unit tests for operator PIN rate limiting, command queue/ACK, telemetry ingestion.
  - `astro/daemon/src/main.rs`: Unit tests for atomic credit persistence, recovery from corrupt/truncated files, rapid counter updates, command execution.
  - `astro/daemon/src/ota.rs`: Unit tests for OTA staging rejection on missing, empty, or invalid SHA-256, and cleanup.
  - `astro/rootfs/`: Config verification asserting absence of static IP `192.168.100.2`.

## Real-World Application Scenarios (Tier 4)
| # | Scenario | Features Exercised | Expected Outcome |
|---|----------|--------------------|------------------|
| 1 | Corrupt Credit Recovery After Power Interruption | F2, F6 | Daemon boots, recovers intact coin records from `.bak`, ignores corrupted `.json`, accepts new coins |
| 2 | Malicious/Incomplete OTA Update Attempt | F1 | Payload rejected immediately, disk space recovered, node remains stable on active slot |
| 3 | LAN Brute-Force Attack on Operator PIN | F5 | Attacker locked out at 5th attempt with 429 & Retry-After; legitimate client on separate IP unaffected |
| 4 | Sensorless Node Normal Heartbeat Cycle | F4 | Station sends telemetry with `None` thermals; BOSS accepts 200 OK, no 422 rejected telemetry |
| 5 | Full Remote Supervisory Command Roundtrip | F6, F2 | Admin enqueues `AddCredits`; node polls, executes atomic persistence, returns ACK; BOSS marks Acknowledged |
