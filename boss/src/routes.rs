//! HTTP handlers: telemetry ingest, snapshots, authenticated cabinet RPC,
//! Dual-Mode operator PIN authentication, Hybrid Plugin Engine endpoints,
//! node command polling, and edge-sync submission endpoints.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{constant_time_eq, RequireNodeToken, RequireRpcToken};
use crate::models::{
    CommandAck, CommandRecord, CommandStatus, ErrorEvent, HighScoreEntry, NodeTelemetry,
    TokenTransaction, now_epoch_ms,
};
use crate::plugins::{PluginEvent, PluginManifest};
use crate::state::{EdgeItem, SharedState, SseChannel};

/// Nodes not heard from within this window are considered offline.
pub const STALENESS_MS: u64 = 15_000;

#[derive(Deserialize, Default)]
pub struct RebootRequest {
    #[serde(default)]
    pub delay_secs: Option<u32>,
}

#[derive(Deserialize)]
pub struct LaunchGameRequest {
    pub core: String,
    pub rom: String,
    #[serde(default)]
    pub netplay: bool,
}

#[derive(Deserialize)]
pub struct AddCreditsRequest {
    pub node_id: String,
    pub credits: u32,
}

#[derive(Deserialize)]
pub struct PinUnlockRequest {
    pub pin: String,
}

#[derive(Deserialize)]
pub struct PluginToggleRequest {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct PluginActionRequest {
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

// ---------------------------------------------------------------------------
// Telemetry pipeline
// ---------------------------------------------------------------------------

/// POST /api/v1/telemetry — node heartbeat ingestion (node-authenticated).
///
/// Persists the latest heartbeat, buffers it for the Cloudflare edge batcher,
/// dispatches `PluginEvent::TelemetryHeartbeat`, and emits online/offline alerts.
pub async fn ingest_telemetry(
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
    headers: HeaderMap,
    Json(payload): Json<NodeTelemetry>,
) -> StatusCode {
    if let Err(reason) = payload.validate() {
        tracing::warn!(node_id = %payload.node_id, %reason, "rejected invalid telemetry");
        return StatusCode::UNPROCESSABLE_ENTITY;
    }

    let cf_ip = headers
        .get("CF-Connecting-IP")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("direct");
    tracing::debug!(node_id = %payload.node_id, ip = %cf_ip, "telemetry heartbeat");

    let now = now_epoch_ms();
    let mut nodes = state.nodes.write().await;
    // "Online" means heard from within the staleness window, so a cabinet that
    // dropped off and came back raises node_online again. Entries are kept so
    // /admin can still list offline cabinets.
    let was_online = nodes
        .get(&payload.node_id)
        .is_some_and(|(_, last_seen)| now.saturating_sub(*last_seen) < STALENESS_MS);
    nodes.insert(payload.node_id.clone(), (payload.clone(), now));
    drop(nodes);

    if !was_online {
        state.publish(
            SseChannel::Alerts,
            &json!({
                "kind": "node_online",
                "node_id": payload.node_id,
                "at_epoch_ms": now,
            }),
        );
        state
            .plugins
            .dispatch_event(
                PluginEvent::NodeOnline {
                    node_id: payload.node_id.clone(),
                    at_epoch_ms: now,
                },
                &state,
            )
            .await;
    }

    state
        .plugins
        .dispatch_event(PluginEvent::TelemetryHeartbeat(payload.clone()), &state)
        .await;
    state
        .buffer_edge_item(EdgeItem::Telemetry(payload))
        .await;
    StatusCode::OK
}

/// GET /api/telemetry — current venue snapshot (public, for dashboards).
pub async fn get_telemetry_snapshot(
    State(state): State<SharedState>,
) -> Json<crate::models::VenueTelemetryReport> {
    Json(state.build_venue_report(STALENESS_MS).await)
}

// ---------------------------------------------------------------------------
// Dual-Mode Operator Authentication (LAN Bearer Token + Physical Kiosk PIN)
// ---------------------------------------------------------------------------

/// POST /api/v1/auth/pin — exchange the 6-digit physical kiosk operator PIN
/// for a short-lived operator session token accepted by `RequireRpcToken`.
pub async fn verify_operator_pin(
    State(state): State<SharedState>,
    Json(req): Json<PinUnlockRequest>,
) -> Response {
    let candidate = req.pin.trim();
    if constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes()) {
        let (token, expires_at_epoch_ms) = state.mint_operator_session().await;
        tracing::info!("Operator kiosk session unlocked via PIN");
        (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "token": token,
                "expires_at_epoch_ms": expires_at_epoch_ms,
                "venue": state.config.venue_name,
            })),
        )
            .into_response()
    } else {
        tracing::warn!("Rejected invalid operator PIN attempt");
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "ok": false,
                "error": "invalid_pin",
                "detail": "Incorrect 6-digit operator PIN."
            })),
        )
            .into_response()
    }
}

/// GET /api/v1/auth/verify — check whether the caller's Bearer token is valid.
pub async fn verify_operator_session(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
) -> Json<Value> {
    Json(json!({
        "ok": true,
        "venue": state.config.venue_name,
        "address": state.config.venue_address,
        "checked_at_epoch_ms": now_epoch_ms(),
    }))
}

/// POST /api/v1/auth/logout — immediately revoke a PIN-minted operator session.
pub async fn revoke_operator_session(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Json<Value> {
    if let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
    {
        state.operator_sessions.write().await.remove(token);
    }
    Json(json!({ "ok": true }))
}

/// GET /api/v1/admin/overview — comprehensive supervisory snapshot for `/admin`.
pub async fn get_admin_overview(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
) -> Json<Value> {
    let report = state.build_venue_report(STALENESS_MS).await;
    let now = now_epoch_ms();

    let all_nodes: Vec<Value> = {
        let map = state.nodes.read().await;
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by_key(|(id, _)| (*id).clone());
        entries
            .into_iter()
            .map(|(_, (telemetry, last_seen))| {
                let age_ms = now.saturating_sub(*last_seen);
                json!({
                    "telemetry": telemetry,
                    "last_seen_epoch_ms": last_seen,
                    "age_ms": age_ms,
                    "online": age_ms < STALENESS_MS,
                })
            })
            .collect()
    };

    let recent_commands: Vec<CommandRecord> = {
        let map = state.commands.read().await;
        let mut cmds: Vec<CommandRecord> = map.values().cloned().collect();
        cmds.sort_by(|a, b| b.issued_at_epoch_ms.cmp(&a.issued_at_epoch_ms));
        cmds.truncate(50);
        cmds
    };

    let errors: Vec<ErrorEvent> = state.recent_errors.read().await.iter().cloned().collect();
    let scores: Vec<HighScoreEntry> = state.recent_scores.read().await.iter().cloned().collect();
    let transactions: Vec<TokenTransaction> = state
        .recent_transactions
        .read()
        .await
        .iter()
        .cloned()
        .collect();
    let edge_backlog_len = state.edge_buffer.lock().await.len();
    let plugins = state.plugins.list_descriptors().await;

    Json(json!({
        "venue": report.venue,
        "address": report.address,
        "status": report.status,
        "nodes_online": report.nodes_online,
        "nodes": all_nodes,
        "commands": recent_commands,
        "errors": errors,
        "scores": scores,
        "transactions": transactions,
        "edge_sync": {
            "enabled": state.config.edge_ingest_url.is_some(),
            "endpoint": state.config.edge_ingest_url,
            "buffered_items": edge_backlog_len,
        },
        "plugins": plugins,
        "generated_at_epoch_ms": now,
    }))
}

// ---------------------------------------------------------------------------
// Hybrid Plugin Engine Endpoints
// ---------------------------------------------------------------------------

/// GET /api/v1/plugins — list all registered plugins and their live state.
pub async fn list_plugins(State(state): State<SharedState>) -> Json<Value> {
    let plugins = state.plugins.list_descriptors().await;
    Json(json!({
        "plugins": plugins,
        "generated_at_epoch_ms": now_epoch_ms(),
    }))
}

/// POST /api/v1/plugins/:plugin_id/toggle — enable or disable a plugin.
pub async fn toggle_plugin(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Path(plugin_id): Path<String>,
    Json(req): Json<PluginToggleRequest>,
) -> Response {
    let Some(plugin) = state.plugins.find_by_id(&plugin_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "plugin_not_found", "plugin_id": plugin_id })),
        )
            .into_response();
    };

    plugin.set_enabled(req.enabled).await;
    let manifest = plugin.manifest().await;
    let snapshot = plugin.state_snapshot().await;

    state.publish(
        SseChannel::Plugins,
        &json!({
            "kind": "plugin_toggled",
            "plugin_id": plugin_id,
            "enabled": req.enabled,
            "manifest": manifest,
            "state": snapshot,
            "at_epoch_ms": now_epoch_ms(),
        }),
    );

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "manifest": manifest,
            "state": snapshot,
        })),
    )
        .into_response()
}

/// POST /api/v1/plugins/:plugin_id/action — invoke a plugin action.
///
/// Patron-facing actions (`create_qr_session`, `tap_card`) are permitted from
/// the physical kiosk view; all other administrative actions require a valid
/// operator token (`BOSS_RPC_TOKEN` or PIN session).
pub async fn invoke_plugin_action(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(plugin_id): Path<String>,
    Json(req): Json<PluginActionRequest>,
) -> Response {
    let is_patron_safe = (plugin_id == "pos-payments"
        && matches!(
            req.action.as_str(),
            "create_qr_session"
                | "create_square_checkout"
                | "activate_game_time_pass"
                | "book_timeslot"
                | "submit_fb_order"
        ))
        || (plugin_id == "rfid-peripherals" && req.action == "tap_card");

    if !is_patron_safe {
        let presented = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .unwrap_or("");
        if !state.is_valid_operator_token(presented).await {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "unauthorized",
                    "hint": "Operator authentication required for this plugin action."
                })),
            )
                .into_response();
        }
    }

    let Some(plugin) = state.plugins.find_by_id(&plugin_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "plugin_not_found", "plugin_id": plugin_id })),
        )
            .into_response();
    };

    if !plugin.is_enabled().await {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "plugin_disabled",
                "detail": format!("Plugin '{}' is currently disabled.", plugin_id)
            })),
        )
            .into_response();
    }

    match plugin.invoke_action(&req.action, req.payload, &state).await {
        Ok(result) => {
            let snapshot = plugin.state_snapshot().await;
            state.publish(
                SseChannel::Plugins,
                &json!({
                    "kind": "plugin_action",
                    "plugin_id": plugin_id,
                    "action": req.action,
                    "state": snapshot,
                    "at_epoch_ms": now_epoch_ms(),
                }),
            );
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "plugin_id": plugin_id,
                    "action": req.action,
                    "result": result,
                    "state": snapshot,
                })),
            )
                .into_response()
        }
        Err(detail) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "action_failed",
                "detail": detail,
            })),
        )
            .into_response(),
    }
}

/// POST /api/v1/plugins/webhooks — register a custom webhook-backed plugin manifest.
pub async fn register_webhook_plugin(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Json(manifest): Json<PluginManifest>,
) -> Response {
    if manifest.id.trim().is_empty() || manifest.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_manifest", "detail": "id and name are required." })),
        )
            .into_response();
    }
    state.plugins.register_webhook_plugin(manifest.clone()).await;
    let plugins = state.plugins.list_descriptors().await;
    state.publish(
        SseChannel::Plugins,
        &json!({
            "kind": "plugin_registered",
            "plugin_id": manifest.id,
            "plugins": plugins,
            "at_epoch_ms": now_epoch_ms(),
        }),
    );
    (
        StatusCode::CREATED,
        Json(json!({ "ok": true, "plugins": plugins })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Authenticated cabinet RPC
// ---------------------------------------------------------------------------

/// POST /api/v1/cabinet/:node_id/reboot — queue a controlled reboot.
pub async fn cabinet_reboot(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Path(node_id): Path<String>,
    body: Option<Json<RebootRequest>>,
) -> Json<serde_json::Value> {
    let delay_secs = body
        .and_then(|Json(b)| b.delay_secs)
        .unwrap_or(5)
        .min(600);

    let record = state
        .enqueue_command(&node_id, crate::models::CabinetCommand::Reboot { delay_secs })
        .await;
    tracing::info!(node_id = %node_id, command_id = %record.id, "reboot queued");
    Json(command_accepted(&record))
}

/// POST /api/v1/cabinet/:node_id/ping — queue a round-trip latency/liveness ping.
pub async fn cabinet_ping(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Path(node_id): Path<String>,
) -> Json<serde_json::Value> {
    let seq = now_epoch_ms();
    let record = state
        .enqueue_command(&node_id, crate::models::CabinetCommand::Ping { seq })
        .await;
    tracing::info!(node_id = %node_id, command_id = %record.id, "ping queued");
    Json(command_accepted(&record))
}

/// POST /api/v1/cabinet/:node_id/launch_game — queue a game boot.
pub async fn cabinet_launch_game(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Path(node_id): Path<String>,
    Json(req): Json<LaunchGameRequest>,
) -> Json<serde_json::Value> {
    let core = req.core.trim().to_string();
    let rom = req.rom.trim().to_string();
    if core.is_empty() || rom.is_empty() {
        return Json(json!({
            "error": "invalid_request",
            "detail": "'core' and 'rom' must be non-empty strings."
        }));
    }

    let record = state
        .enqueue_command(
            &node_id,
            crate::models::CabinetCommand::LaunchGame {
                core,
                rom,
                netplay: req.netplay,
            },
        )
        .await;
    tracing::info!(node_id = %node_id, command_id = %record.id, "launch_game queued");
    Json(command_accepted(&record))
}

/// POST /api/v1/credits/add — queue credit injection on a cabinet and record
/// the token transaction for the edge D1 ledger.
pub async fn credits_add(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Json(req): Json<AddCreditsRequest>,
) -> Json<serde_json::Value> {
    if req.credits == 0 || req.credits > 100 {
        return Json(json!({
            "error": "invalid_request",
            "detail": "'credits' must be between 1 and 100."
        }));
    }

    let node_id = req.node_id.trim().to_string();
    let record = state
        .enqueue_command(
            &node_id,
            crate::models::CabinetCommand::AddCredits {
                credits: req.credits,
            },
        )
        .await;

    let balance_after = {
        let nodes = state.nodes.read().await;
        nodes
            .get(&node_id)
            .map(|(t, _)| t.credits.credits_available.saturating_add(req.credits))
            .unwrap_or(req.credits)
    };

    let tx = TokenTransaction {
        node_id: node_id.clone(),
        kind: "add".to_string(),
        credits: req.credits,
        balance_after,
        command_id: Some(record.id.clone()),
        created_at_epoch_ms: now_epoch_ms(),
    };
    state.record_transaction(tx.clone()).await;
    state
        .plugins
        .dispatch_event(PluginEvent::CreditTransaction(tx.clone()), &state)
        .await;
    state.buffer_edge_item(EdgeItem::Transaction(tx)).await;
    tracing::info!(node_id = %node_id, command_id = %record.id, credits = req.credits, "credits queued");

    Json(json!({
        "command_id": record.id,
        "node_id": node_id,
        "credits": req.credits,
        "projected_balance": balance_after,
        "status": "queued",
        "issued_at_epoch_ms": record.issued_at_epoch_ms,
    }))
}

/// GET /api/v1/commands/:command_id — poll a command's lifecycle status.
pub async fn command_status(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Path(command_id): Path<String>,
) -> Json<serde_json::Value> {
    let commands = state.commands.read().await;
    match commands.get(&command_id) {
        Some(record) => Json(serde_json::to_value(record).unwrap_or_else(|_| json!({}))),
        None => Json(json!({ "error": "not_found", "command_id": command_id })),
    }
}

// ---------------------------------------------------------------------------
// Node-side command plane (nodes poll + ack)
// ---------------------------------------------------------------------------

/// GET /api/v1/cabinet/:node_id/commands — drain this node's pending commands.
pub async fn poll_commands(
    State(state): State<SharedState>,
    RequireNodeToken(claim): RequireNodeToken,
    Path(node_id): Path<String>,
) -> Json<serde_json::Value> {
    let _ = claim;
    let mut pending = state.pending_commands.write().await;
    let ids = pending.remove(&node_id).unwrap_or_default();
    drop(pending);

    let mut commands_map = state.commands.write().await;
    let now = now_epoch_ms();
    let mut dispatched: Vec<CommandRecord> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(record) = commands_map.get_mut(id) {
            record.status = CommandStatus::Dispatched;
            record.dispatched_at_epoch_ms = Some(now);
            dispatched.push(record.clone());
        }
    }

    Json(json!({ "node_id": node_id, "commands": dispatched }))
}

/// POST /api/v1/cabinet/:node_id/commands/ack — node reports command outcome.
pub async fn ack_command(
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
    Path(node_id): Path<String>,
    Json(ack): Json<CommandAck>,
) -> StatusCode {
    let now = now_epoch_ms();
    let mut commands = state.commands.write().await;
    if let Some(record) = commands.get_mut(&ack.command_id) {
        if record.node_id != node_id {
            return StatusCode::FORBIDDEN;
        }
        record.status = if ack.success {
            CommandStatus::Acknowledged
        } else {
            CommandStatus::Failed
        };
        record.completed_at_epoch_ms = Some(now);
        record.result = ack.detail.clone();
        let snapshot = record.clone();
        drop(commands);
        state.publish(SseChannel::Commands, &snapshot);
        state
            .plugins
            .dispatch_event(PluginEvent::CommandUpdated(snapshot), &state)
            .await;
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

// ---------------------------------------------------------------------------
// Node events: errors + high scores (forwarded to edge D1 & plugin bus)
// ---------------------------------------------------------------------------

/// POST /api/v1/events/error — node error report; broadcast + forward to D1.
pub async fn report_error(
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
    Json(mut event): Json<ErrorEvent>,
) -> StatusCode {
    if event.node_id.trim().is_empty() || event.message.trim().is_empty() {
        return StatusCode::UNPROCESSABLE_ENTITY;
    }
    if !matches!(event.level.as_str(), "warn" | "error" | "fatal") {
        event.level = "error".to_string();
    }
    event.occurred_at_epoch_ms = if event.occurred_at_epoch_ms == 0 {
        now_epoch_ms()
    } else {
        event.occurred_at_epoch_ms
    };

    state.record_error(event.clone()).await;
    state.publish(SseChannel::Alerts, &event);
    state
        .plugins
        .dispatch_event(PluginEvent::ErrorReported(event.clone()), &state)
        .await;
    state.buffer_edge_item(EdgeItem::Error(event)).await;
    StatusCode::OK
}

/// POST /api/v1/scores — node score submission; broadcast + forward to D1.
pub async fn submit_score(
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
    Json(mut entry): Json<HighScoreEntry>,
) -> StatusCode {
    if entry.node_id.trim().is_empty()
        || entry.active_rom.trim().is_empty()
        || entry.initials.trim().is_empty()
        || entry.initials.chars().count() > 3
    {
        return StatusCode::UNPROCESSABLE_ENTITY;
    }
    entry.achieved_at_epoch_ms = if entry.achieved_at_epoch_ms == 0 {
        now_epoch_ms()
    } else {
        entry.achieved_at_epoch_ms
    };

    state.record_score(entry.clone()).await;
    state.publish(SseChannel::Alerts, &entry);
    state
        .plugins
        .dispatch_event(PluginEvent::HighScore(entry.clone()), &state)
        .await;
    state.buffer_edge_item(EdgeItem::Score(entry)).await;
    StatusCode::OK
}

fn command_accepted(record: &CommandRecord) -> serde_json::Value {
    json!({
        "command_id": record.id,
        "node_id": record.node_id,
        "status": record.status,
        "issued_at_epoch_ms": record.issued_at_epoch_ms,
    })
}
