# Dispatch for explorer_survey_1

- Role: Codebase & Spec Explorer (A.S.T.R.O. Daemon & Networking)
- Mission: Investigate R1 (OTA hash verification), R2 (Credit persistence & atomic write/recovery), and R3 (static IP removal & DHCP/link-local fallback).
- Project Root: d:\data\dev\beercade
- Authoritative Requirements: d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md
- Output Report: d:\data\dev\beercade\.agents\teamwork\explorer_survey_1\analysis.md
- Handoff Report: d:\data\dev\beercade\.agents\teamwork\explorer_survey_1\handoff.md

## 2026-10-27T13:43:15Z
Sender: 9b504612-81e9-4856-b70d-852ae15f2b33
Priority: MESSAGE_PRIORITY_HIGH

You are explorer_survey_1 (Astro & Network Explorer).
Your working directory is: d:\data\dev\beercade\.agents\teamwork\explorer_survey_1
Read d:\data\dev\beercade\.agents\teamwork\ORIGINAL_REQUEST.md and d:\data\dev\beercade\.agents\teamwork\explorer_survey_1\DISPATCH.md.

Scope & Mission:
Investigate requirements R1, R2, and R3 in the B.E.E.R.C.A.D.E. codebase:
1. R1: OTA Hash Verification in astro/daemon/src/ota.rs
   - How is OTA update payload staging currently implemented?
   - How are headers received and parsed (specifically X-OTA-SHA256)?
   - Where are staged files saved, verified, or cleaned up on error?
   - What error types and response codes are returned?
2. R2: Atomic & Crash-Safe Credit Persistence in astro/daemon/src/main.rs
   - How does persist_credits currently write credit data?
   - What file paths are used? Where is sync_all needed? How should rename be handled?
   - How does load/recovery currently work on startup? What happens if credits.json is truncated, corrupt, or missing?
   - What backup/recovery mechanism should be introduced?
3. R3: Remove Hardcoded Static IP Conflicts
   - Inspect astro/rootfs/etc/systemd/network/10-wired.network and astro/rootfs/usr/local/bin/boss_static_net.sh.
   - Where is 192.168.100.2 hardcoded?
   - What is needed to configure dynamic DHCP assignment with link-local fallback (IPv4LL / systemd-networkd LinkLocalAddressing)?

Constraints:
- You are read-only. DO NOT write or edit source code files.
- STRICTLY FORBIDDEN: Do NOT run image builds (make image-x86, mksquashfs, Docker rootfs exports).
- Write your comprehensive findings to d:\data\dev\beercade\.agents\teamwork\explorer_survey_1\analysis.md
- Write your handoff report to d:\data\dev\beercade\.agents\teamwork\explorer_survey_1\handoff.md
- When done, use send_message to report your completion and key findings to orchestrator.
