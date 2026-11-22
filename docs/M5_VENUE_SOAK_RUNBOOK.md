# M5 24-Hour Mixed-Arch Venue Soak & Acceptance Runbook

> **Milestone Gate (M5):** Verify a live mixed-architecture venue stack (`x86_64` + `aarch64` A.S.T.R.O. cabinets → B.O.S.S. `:8080` + UDP `:9750` → `POST /api/v1/edge/ingest` → Cloudflare D1/KV) across 24 hours with **0 dropped sessions**, verified D1 edge telemetry parity, multi-cabinet UDP `:9750` netplay relay/P2P exercise, and a live A/B OTA update + rollback round-trip.

---

## 1. Pre-Flight Build & Hyper-V / Bare-Metal Verification

1. **Build bootable images** (on Linux or Windows Docker Desktop from Git Bash):
   ```bash
   make image-x86          # out/beercade-astro-x86_64.{raw,raw.zst,vhdx}
   make image-boss-x86     # out/beercade-boss-x86_64.{raw,raw.zst,vhdx}
   make image-boss-rpi     # out/beercade-boss-universal-aarch64.raw
   bash shared/buildfarm/aarch64/build_image.sh astro
   ```
2. **Run Hyper-V Gen2 VHDX verification** (Windows elevated PowerShell):
   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File shared/tests/verify_hyperv_x86_64.ps1 -Payload astro
   powershell -NoProfile -ExecutionPolicy Bypass -File shared/tests/verify_hyperv_x86_64.ps1 -Payload boss
   ```
   Also supported via QEMU/OVMF on `.raw` or `.vhdx`:
   ```bash
   bash shared/tests/verify_qemu_x86_64.sh boss out/beercade-boss-x86_64.vhdx
   ```
3. **Spot-check real PC NIC/GPU firmware & Vulkan readiness** on the `x86_64` cabinet & B.O.S.S. box:
   ```bash
   ip link show
   dmesg | grep -iE "firmware|amdgpu|i915|xe|nvidia|r8169|iwlwifi"
   vulkaninfo --summary
   ```

---

## 2. Cloudflare D1/KV & Appliance Token Provisioning

1. Replace the placeholder IDs in `website/wrangler.jsonc` (`database_id`, `kv_namespaces[].id`) with real Cloudflare D1 & KV IDs, then apply schema migrations:
   ```bash
   npx --prefix website wrangler d1 migrations apply beercade-db --remote
   npx --prefix website wrangler pages secret put EDGE_INGEST_TOKEN
   ```
2. Configure matching tokens in `/data/beercade/boss.env` on B.O.S.S. (never commit real `.env`/`.local` secrets):
   ```ini
   BOSS_NODE_TOKEN=<strong-node-token>
   BOSS_RPC_TOKEN=<strong-rpc-token>
   BOSS_OPERATOR_PIN=<6-digit-pin>
   EDGE_INGEST_URL=https://<your-pages-domain>/api/v1/edge/ingest
   EDGE_INGEST_TOKEN=<same-edge-token>
   ```
3. Configure `/data/beercade/astro.env` on both the `x86_64` and `aarch64` A.S.T.R.O. cabinets:
   ```ini
   BOSS_ENDPOINT=http://boss.lan:8080
   BOSS_NODE_TOKEN=<same-node-token>
   ```

---

## 3. Execute the 24-Hour Mixed-Arch Venue Soak

1. **Pre-check locally with the self-contained M5 smoke soak:**
   ```bash
   python3 shared/tests/m5_venue_soak.py --mode smoke --duration-secs 15
   ```
2. **Start the 24-hour venue orchestrator** against live B.O.S.S. + Cloudflare edge + `x86_64` & `aarch64` cabinets:
   ```bash
   python3 shared/tests/m5_venue_soak.py \
     --mode venue \
     --duration-secs 86400 \
     --boss-url http://boss.lan:8080 \
     --netplay-port 9750 \
     --nodes CAB-X86-01,CAB-ARM-01 \
     --node-token "$BOSS_NODE_TOKEN" \
     --rpc-token "$BOSS_RPC_TOKEN" \
     --edge-latest-url "https://<your-pages-domain>/api/v1/edge/latest"
   ```
   Checkpoints are written every 5 minutes to `out/m5-soak-report.json`.

---

## 4. UDP `:9750` Multi-Cabinet Rollback Netplay Verification

Configure two cabinets (e.g., `CAB-X86-01` as host, `CAB-ARM-01` as peer) in `/data/beercade/astro.env` (note `RollbackSession` currently uses `ASTRO_NETPLAY_PEER` directly alongside `boss-server` `:9750` relay monitoring):
```ini
# On CAB-X86-01 (player 0 / host)
ASTRO_NETPLAY_SESSION=9001
ASTRO_NETPLAY_PLAYER=0
ASTRO_NETPLAY_HOST=1
ASTRO_NETPLAY_PORT=9750
ASTRO_NETPLAY_PEER=<CAB-ARM-01-IP>:9750

# On CAB-ARM-01 (player 1 / peer)
ASTRO_NETPLAY_SESSION=9001
ASTRO_NETPLAY_PLAYER=1
ASTRO_NETPLAY_HOST=0
ASTRO_NETPLAY_PORT=9750
ASTRO_NETPLAY_PEER=<CAB-X86-01-IP>:9750
```
Verify relay health & zero unrecovered desyncs at `GET http://boss.lan:8080/api/v1/netplay/sessions`.

---

## 5. Live A/B OTA Update & Failed-Boot Rollback Round-Trip

1. **Upload an updated rootfs SquashFS payload** (`.sqsh` extracted or generated from buildfarm) to B.O.S.S.:
   ```bash
   curl -fsS -X POST "http://boss.lan:8080/api/v1/sync/upload" \
     -H "Authorization: Bearer $BOSS_RPC_TOKEN" \
     -H "X-Payload-Name: beercade-astro-x86_64-m5.sqsh" \
     --data-binary @out/root.sqsh
   ```
   Verify `GET /api/v1/sync` lists the payload and `X-OTA-SHA256` matches `sha256sum out/root.sqsh`.
2. **Healthy OTA round-trip (`ROOT_A -> ROOT_B`):** Trigger OTA apply on the test cabinet (`astro-daemon` downloads to `/data/beercade/sync/staging.sqsh`, checks SHA-256 + `statvfs`, writes inactive slot, sets `beercade-slot-b+3.conf` / `tryboot.txt` / U-Boot `upgrade_available=1`, reboots). Confirm `beercade-boot-check.service` blesses slot B (`beercade-slot-b.conf`, active slot B in `GET /api/telemetry`).
3. **Rollback verification (`ROOT_B -> ROOT_A`):** Stage a deliberately broken service unit or non-booting test overlay to inactive slot A (or simulate failed health check so `beercade-boot-check.sh` does not bless the trial boot and exhausts `+3` tries / triggers `altbootcmd`). Confirm the bootloader automatically falls back to healthy slot B and the cabinet re-registers with B.O.S.S.

---

## 6. M5 Acceptance Criteria

| Gate | Pass Requirement | Evidence Artifact |
| :--- | :--- | :--- |
| **Telemetry & Edge Parity** | Continuous `x86_64` + `aarch64` heartbeats across 24 h; `machine_analytics` D1 row counts and `Option<f32>` thermal nulls match B.O.S.S. | `out/m5-soak-report.json` + Cloudflare D1 query |
| **Patron Sessions (`BRC-XXXX`)** | `dropped_sessions == 0` across `StartSession` → `SessionExpired` grace → `UpdateSession` → `EndSession` cycles | `out/m5-soak-report.json` |
| **Netplay (`:9750`)** | Multi-cabinet rollback session active without panic or unrecovered desync | `GET /api/v1/netplay/sessions` + `astro-daemon` journal |
| **A/B OTA + Rollback** | `X-OTA-SHA256` verified staging, healthy slot switch blessed, and failed-boot fallback restored | `beercade-boot-check` logs + `root=PARTLABEL=ROOT_*` |
