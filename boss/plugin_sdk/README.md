# B.O.S.S. Hybrid Plugin SDK (`boss/plugin_sdk`)

Official developer SDK, JSON Schema, TypeScript definitions, and reference templates for building plugins for **B.O.S.S.** (*Beercade Operational Supervisory System*) within the **B.E.E.R.C.A.D.E.** ecosystem.

---

## 1. Architecture Overview

B.O.S.S. uses a **Hybrid Event-Hook + UI Panel Architecture** supporting two deployment models:

1. **Out-of-Tree Declarative & Webhook Plugins (Zero Recompile)**:
   - Register live via the `/admin` Management Portal (**+ WEBHOOK**) or drop a `*.json` manifest into `BOSS_PLUGINS_DIR` (`/var/lib/beercade/plugins`).
   - B.O.S.S. automatically fans out subscribed `PluginEvent` payloads (`telemetry_heartbeat`, `node_online`, `node_offline`, `error_reported`, `high_score`, `credit_transaction`, `command_updated`, or `*`) to your `webhook_url` and proxies operator/kiosk actions (`POST /api/v1/plugins/:id/action`).

2. **In-Tree Rust Native Plugins (`BossPlugin` Trait)**:
   - Implement the async `BossPlugin` trait in [`boss/src/plugins.rs`](../src/plugins.rs) for sub-millisecond hardware/LAN integrations (Square POS, Multi-Zone DMX/WLED/sACN/Hue lighting, Tournament & TV Signage engine, RFID wristbands, Alerting sinks).

---

## 2. SDK Contents

| File | Description |
| :--- | :--- |
| [`boss-plugin.schema.json`](./boss-plugin.schema.json) | JSON Schema (Draft 2020-12) for validating `manifest.json` files in VS Code / CI |
| [`index.d.ts`](./index.d.ts) | TypeScript SDK definitions for `PluginManifest`, `PluginEvent`, webhook deliveries, and RPC actions |
| [`examples/custom-marquee-plugin/manifest.json`](./examples/custom-marquee-plugin/manifest.json) | Sample plugin manifest subscribing to `high_score` and `credit_transaction` |
| [`examples/custom-marquee-plugin/webhook_server.mjs`](./examples/custom-marquee-plugin/webhook_server.mjs) | Zero-dependency Node.js webhook receiver & custom action handler |

---

## 3. Quick Start: Deploying an Out-of-Tree Plugin

### Option A — Register Live via the `/admin` Management Portal
1. Open `http://<boss-ip>:8090/admin` (or unlock via the 5-tap corner gesture + 6-digit PIN `898900` on the kiosk).
2. Navigate to **PLUGINS** → click **+ WEBHOOK**.
3. Enter your plugin ID, name, subscribed events, and webhook URL (e.g. `http://10.10.20.99:9100/boss-webhook`).

### Option B — Drop a Manifest into `BOSS_PLUGINS_DIR`
Copy your manifest JSON file into `/var/lib/beercade/plugins/my-plugin.json`:

```json
{
  "$schema": "./boss-plugin.schema.json",
  "id": "custom-marquee-sync",
  "name": "Custom LED Marquee & Score Ticker",
  "version": "1.0.0",
  "category": "webhook_custom",
  "description": "Pushes live A.S.T.R.O. game titles and high scores to overhead RGB matrix marquees.",
  "author": "Venue Engineering",
  "enabled": true,
  "builtin": false,
  "subscribed_events": ["high_score", "credit_transaction"],
  "capabilities": ["rgb_matrix", "score_banner"],
  "ui_panel": {
    "tab_label": "RGB Marquees",
    "icon": "sparkles",
    "summary_metric_key": "dispatched_count"
  },
  "webhook_url": "http://127.0.0.1:9100/boss-webhook"
}
```

---

## 4. Event Bus Payloads (`PluginEvent`)

Whenever an event occurs on any **O.R.I.O.N.** node running **A.S.T.R.O.** or on **B.O.S.S.**, subscribed webhook plugins receive an HTTP `POST` request with the following JSON envelope:

```json
{
  "plugin_id": "custom-marquee-sync",
  "dispatched_at_epoch_ms": 1761350400000,
  "event": {
    "kind": "high_score",
    "data": {
      "node_id": "ASTRO-01",
      "active_core": "fbneo_libretro",
      "active_rom": "sfiii3.zip",
      "game_name": "Street Fighter III: 3rd Strike",
      "score": 984200,
      "initials": "DGO",
      "achieved_at_epoch_ms": 1761350400000
    }
  }
}
```
