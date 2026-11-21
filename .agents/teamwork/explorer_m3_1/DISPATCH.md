# Dispatch: Explorer M3-1 (A.S.T.R.O. OTA Hash Verification)
Target: Investigate Requirement R1 (Mandatory SHA-256 OTA verification & cleanup) in astro/daemon/src/ota.rs.
See ORIGINAL_REQUEST.md, PROJECT.md, TEST_INFRA.md.

## 2026-10-27T23:35:24Z
[Message] sender=bd770878-29b7-48ec-a1e1-b5265681a707 priority=MESSAGE_PRIORITY_HIGH
You are Explorer M3-1 for Milestone 3 of B.E.E.R.C.A.D.E.
Working directory: d:\data\dev\beercade\.agents\teamwork\explorer_m3_1

MANDATORY: Read the authoritative user request at:
d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md
Also read:
d:\data\dev\beercade\.agents\teamwork\PROJECT.md
d:\data\dev\beercade\.agents\teamwork\TEST_INFRA.md

Your focus:
Investigate Requirement R1 (Mandatory SHA-256 OTA Verification & Cleanup) in `astro/daemon/src/ota.rs`.
Specifically investigate:
1. Current implementation of OTA update download, staging, and verification in `astro/daemon/src/ota.rs`.
2. How the `X-OTA-SHA256` HTTP header is currently handled (is it optional, ignored, or present).
3. How SHA-256 verification is performed (hashing algorithm, crate dependencies such as `sha2`).
4. Rejection and cleanup requirements:
   - If header is missing, empty, or hash does not match computed payload SHA-256: immediately reject, remove/delete any staged files (`.tmp` or target partition/file), and return an explicit error.
5. Existing test coverage in `astro/daemon/src/ota.rs` or `astro/daemon/src/main.rs`.
6. How to write comprehensive unit tests covering:
   - Staging succeeds when hash matches
   - Staging rejected when `X-OTA-SHA256` is missing
   - Staging rejected when `X-OTA-SHA256` is empty
   - Staging rejected when `X-OTA-SHA256` checksum is invalid / mismatched
   - Verification that staged files are cleanly deleted upon rejection.

Write your findings to d:\data\dev\beercade\.agents\teamwork\explorer_m3_1\analysis.md and d:\data\dev\beercade\.agents\teamwork\explorer_m3_1\handoff.md.
When finished, notify the orchestrator using send_message.
Remember: DO NOT modify any source code files or run image builds. Explore and analyze only.
