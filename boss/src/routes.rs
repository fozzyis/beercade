//! HTTP handlers: telemetry ingest, snapshots, authenticated cabinet RPC,
//! Dual-Mode operator PIN authentication, Hybrid Plugin Engine endpoints,
//! node command polling, and edge-sync submission endpoints.

use std::net::SocketAddr;
use tokio::io::AsyncSeekExt;

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{constant_time_eq, RequireNodeToken, RequireRpcToken};
use crate::models::{
    CabinetCommand, CabinetEvent, CommandAck, CommandRecord, CommandStatus, ErrorEvent, GameRef, HighScoreEntry, NodeTelemetry,
    TokenTransaction, now_epoch_ms,
};
use crate::plugins::{PluginEvent, PluginManifest};
use crate::state::{EdgeItem, PinFailureResult, SharedState, SseChannel};

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

#[derive(Deserialize)]
pub struct ForceConnectRequest {
    pub target: String,
    #[serde(default)]
    pub boss_url: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct FleetDiscoverRequest {
    #[serde(default)]
    pub subnet: Option<String>,
    #[serde(default)]
    pub boss_url: Option<String>,
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

fn extract_client_ip(
    headers: &HeaderMap,
    connect_info: Option<&ConnectInfo<SocketAddr>>,
) -> String {
    if let Some(cf_ip) = headers
        .get("CF-Connecting-IP")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return cf_ip.to_string();
    }

    if let Some(xff) = headers.get("X-Forwarded-For").and_then(|v| v.to_str().ok()) {
        if let Some(first_ip) = xff.split(',').next().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return first_ip.to_string();
        }
    }

    if let Some(real_ip) = headers
        .get("X-Real-IP")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return real_ip.to_string();
    }

    if let Some(ci) = connect_info {
        return ci.0.ip().to_string();
    }

    "127.0.0.1".to_string()
}

/// POST /api/v1/auth/pin — exchange the 6-digit physical kiosk operator PIN
/// for a short-lived operator session token accepted by `RequireRpcToken`.
///
/// Implements brute-force rate limiting: 5 failed attempts locks out the client
/// IP for 60 seconds.
pub async fn verify_operator_pin(
    State(state): State<SharedState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Json(req): Json<PinUnlockRequest>,
) -> Response {
    let client_ip = extract_client_ip(&headers, connect_info.as_ref());
    let mut tracker = state.pin_tracker.lock().await;

    if let Some(retry_after_secs) = tracker.check_lockout(&client_ip) {
        tracing::warn!(ip = %client_ip, retry_after_secs, "Rejected locked-out operator PIN attempt");
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, retry_after_secs.to_string())],
            Json(json!({
                "ok": false,
                "error": "rate_limited",
                "detail": format!("Too many failed PIN attempts. Locked out for {} seconds.", retry_after_secs),
                "retry_after_secs": retry_after_secs,
            })),
        )
            .into_response();
    }

    let candidate = req.pin.trim();
    if constant_time_eq(candidate.as_bytes(), state.config.operator_pin.as_bytes()) {
        tracker.record_success(&client_ip);
        drop(tracker);

        let (token, expires_at_epoch_ms) = state.mint_operator_session().await;
        tracing::info!(ip = %client_ip, "Operator kiosk session unlocked via PIN");
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
        let failure_result = tracker.record_failure(&client_ip);
        drop(tracker);

        match failure_result {
            PinFailureResult::LockedOut { retry_after_secs } => {
                tracing::warn!(ip = %client_ip, retry_after_secs, "Operator PIN attempts exceeded threshold; locking out");
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(axum::http::header::RETRY_AFTER, retry_after_secs.to_string())],
                    Json(json!({
                        "ok": false,
                        "error": "rate_limited",
                        "detail": format!("Too many failed PIN attempts. Locked out for {} seconds.", retry_after_secs),
                        "retry_after_secs": retry_after_secs,
                    })),
                )
                    .into_response()
            }
            PinFailureResult::Invalid { remaining_attempts } => {
                tracing::warn!(ip = %client_ip, remaining_attempts, "Rejected invalid operator PIN attempt");
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "ok": false,
                        "error": "invalid_pin",
                        "detail": "Incorrect 6-digit operator PIN.",
                        "remaining_attempts": remaining_attempts,
                    })),
                )
                    .into_response()
            }
        }
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

// ---------------------------------------------------------------------------
// Node Discovery & Fleet Rescue Handlers
// ---------------------------------------------------------------------------

pub async fn fleet_force_connect(
    State(state): State<SharedState>,
    RequireRpcToken: RequireRpcToken,
    Json(payload): Json<ForceConnectRequest>,
) -> impl IntoResponse {
    match crate::discovery::force_connect_node(&payload.target, payload.boss_url.as_deref(), &state).await {
        Ok(node) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "message": format!("Successfully connected to ASTRO node {}", node.node_id),
                "node": node
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "error": e
            })),
        )
            .into_response(),
    }
}

pub async fn fleet_discover(
    State(state): State<SharedState>,
    RequireRpcToken: RequireRpcToken,
    Json(payload): Json<FleetDiscoverRequest>,
) -> impl IntoResponse {
    let report = crate::discovery::scan_subnets_and_arp(
        payload.boss_url.as_deref(),
        payload.subnet.as_deref(),
        &state,
    )
    .await;
    (StatusCode::OK, Json(json!(report))).into_response()
}

pub async fn fleet_interfaces(
    RequireRpcToken: RequireRpcToken,
) -> impl IntoResponse {
    let interfaces = crate::discovery::get_network_interfaces();
    let default_boss_url = crate::discovery::default_boss_callback_url();
    Json(json!({
        "interfaces": interfaces,
        "default_boss_url": default_boss_url
    }))
}

// ---------------------------------------------------------------------------
// Game Catalogue, File Delivery, and Cabinet Events
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum RangeSpec {
    Full,
    Partial { start: u64, end: u64, len: u64 },
    NotSatisfiable,
}

pub fn parse_range_header(header_val: &str, total_size: u64) -> RangeSpec {
    if total_size == 0 {
        return RangeSpec::Full;
    }
    let trimmed = header_val.trim();
    let bytes_range = match trimmed.strip_prefix("bytes=") {
        Some(s) => s.trim(),
        None => return RangeSpec::Full,
    };
    let range_str = bytes_range.split(',').next().unwrap_or("").trim();
    if let Some((start_str, end_str)) = range_str.split_once('-') {
        let start_str = start_str.trim();
        let end_str = end_str.trim();
        if start_str.is_empty() {
            // Suffix range: bytes=-500 (last 500 bytes)
            if let Ok(suffix) = end_str.parse::<u64>() {
                if suffix == 0 {
                    return RangeSpec::NotSatisfiable;
                }
                let len = suffix.min(total_size);
                let start = total_size - len;
                let end = total_size - 1;
                return RangeSpec::Partial { start, end, len };
            }
        } else if end_str.is_empty() {
            // Prefix range: bytes=500- (from 500 to end)
            if let Ok(start) = start_str.parse::<u64>() {
                if start >= total_size {
                    return RangeSpec::NotSatisfiable;
                }
                let end = total_size - 1;
                let len = total_size - start;
                return RangeSpec::Partial { start, end, len };
            }
        } else {
            // Explicit range: bytes=500-999
            if let (Ok(start), Ok(end)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) {
                if start >= total_size || start > end {
                    return RangeSpec::NotSatisfiable;
                }
                let end = end.min(total_size - 1);
                let len = end - start + 1;
                return RangeSpec::Partial { start, end, len };
            }
        }
    }
    RangeSpec::Full
}

/// GET /api/v1/games — list all enabled games in the catalogue.
pub async fn list_catalogue_games(
    State(state): State<SharedState>,
) -> Json<Vec<GameRef>> {
    Json(state.catalogue.read().await.list_enabled())
}

/// GET /api/v1/cabinet/:node_id/games/:game_id/file — node-authenticated game file streaming.
pub async fn cabinet_game_file(
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
    Path((node_id, game_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    // 1. Authorization check: game_id must be in current active session or attract playlist
    if !state.is_game_authorized_for_node(&node_id, &game_id).await {
        tracing::warn!(
            node_id = %node_id,
            game_id = %game_id,
            "game file download forbidden: game not in active session or attract playlist"
        );
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "forbidden",
                "detail": format!("Game '{}' is not authorised for node '{}'", game_id, node_id)
            })),
        )
            .into_response();
    }

    // 2. Lookup game in catalogue
    let game_entry = match state.catalogue.read().await.get(&game_id).cloned() {
        Some(entry) => entry,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "game_not_found",
                    "detail": format!("Game '{}' not found in catalogue", game_id)
                })),
            )
                .into_response();
        }
    };

    // 3. Resolve ROM path on disk
    let path = match state.catalogue.read().await.resolve_rom_path(&game_entry.game.file_name) {
        Some(p) => p,
        None => {
            tracing::error!(file_name = %game_entry.game.file_name, "ROM file not found on disk");
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "file_not_found",
                    "detail": format!("ROM file '{}' not found on storage", game_entry.game.file_name)
                })),
            )
                .into_response();
        }
    };

    // 4. Open file asynchronously
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(err) => {
            tracing::error!(path = %path.display(), error = %err, "failed to open ROM file");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "file_open_failed",
                    "detail": err.to_string()
                })),
            )
                .into_response();
        }
    };

    let total_size = match file.metadata().await {
        Ok(meta) => meta.len(),
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "metadata_failed",
                    "detail": err.to_string()
                })),
            )
                .into_response();
        }
    };

    let session_id = state.get_node_session(&node_id).await.map(|s| s.session_id);

    // 5. Evaluate Range header
    let range_header = headers
        .get(axum::http::header::RANGE)
        .and_then(|v| v.to_str().ok());

    let range_spec = match range_header {
        Some(val) => parse_range_header(val, total_size),
        None => RangeSpec::Full,
    };

    match range_spec {
        RangeSpec::NotSatisfiable => Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(axum::http::header::CONTENT_RANGE, format!("bytes */{}", total_size))
            .body(axum::body::Body::empty())
            .unwrap_or_else(|_| StatusCode::RANGE_NOT_SATISFIABLE.into_response()),
        RangeSpec::Partial { start, end, len } => {
            if let Err(err) = file.seek(std::io::SeekFrom::Start(start)).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "seek_failed", "detail": err.to_string() })),
                )
                    .into_response();
            }
            let stream = tokio_util::io::ReaderStream::new(tokio::io::AsyncReadExt::take(file, len));
            let body = axum::body::Body::from_stream(stream);
            tracing::info!(
                node_id = %node_id,
                game_id = %game_id,
                session_id = ?session_id,
                start,
                end,
                bytes = len,
                "delivering game file partial range"
            );
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
                .header(axum::http::header::ACCEPT_RANGES, "bytes")
                .header(axum::http::header::CONTENT_LENGTH, len.to_string())
                .header(
                    axum::http::header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, total_size),
                )
                .body(body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        RangeSpec::Full => {
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);
            tracing::info!(
                node_id = %node_id,
                game_id = %game_id,
                session_id = ?session_id,
                bytes = total_size,
                "delivering game file complete stream"
            );
            Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
                .header(axum::http::header::ACCEPT_RANGES, "bytes")
                .header(axum::http::header::CONTENT_LENGTH, total_size.to_string())
                .body(body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

/// POST /api/v1/cabinet/:node_id/events — cabinet session and attract event ingest.
pub async fn cabinet_events(
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
    Path(node_id): Path<String>,
    Json(event): Json<CabinetEvent>,
) -> impl IntoResponse {
    tracing::info!(node_id = %node_id, event = ?event, "received cabinet event");

    match &event {
        CabinetEvent::AttractNeeded => {
            let has_session = state.get_node_session(&node_id).await.is_some();
            if !has_session {
                let recent = state.get_node_recent_attract_ids(&node_id).await;
                let games = state.catalogue.read().await.pick_attract_games(&recent);
                if !games.is_empty() {
                    state.set_node_attract_playlist(&node_id, games.clone()).await;
                    state
                        .enqueue_command(
                            &node_id,
                            CabinetCommand::StartAttract {
                                games,
                                secs_per_game: 90,
                            },
                        )
                        .await;
                }
            }
        }
        CabinetEvent::SessionEnded { session_id, files_wiped } => {
            tracing::info!(
                node_id = %node_id,
                session_id = %session_id,
                files_wiped = %files_wiped,
                "session ended, clearing node session and scheduling attract mode"
            );
            state.clear_node_session(&node_id).await;
            let recent = state.get_node_recent_attract_ids(&node_id).await;
            let games = state.catalogue.read().await.pick_attract_games(&recent);
            if !games.is_empty() {
                state.set_node_attract_playlist(&node_id, games.clone()).await;
                state
                    .enqueue_command(
                        &node_id,
                        CabinetCommand::StartAttract {
                            games,
                            secs_per_game: 90,
                        },
                    )
                    .await;
            }
        }
        _ => {}
    }

    state.publish(
        SseChannel::Alerts,
        &json!({
            "kind": "cabinet_event",
            "node_id": node_id,
            "event": &event,
            "at_epoch_ms": now_epoch_ms(),
        }),
    );
    state.publish(
        SseChannel::Ui,
        &json!({
            "kind": "cabinet_event",
            "node_id": node_id,
            "event": &event,
            "at_epoch_ms": now_epoch_ms(),
        }),
    );

    (StatusCode::OK, Json(json!({ "ok": true, "node_id": node_id })))
}

/// Construct the complete Axum Router for B.O.S.S. Server.
pub fn build_router(state: SharedState) -> axum::Router {
    build_router_with_relay(state, std::sync::Arc::new(crate::netplay_relay::RelayState::new()))
}

/// Construct the Axum Router with an existing Netplay Relay state.
pub fn build_router_with_relay(
    state: SharedState,
    relay_state: std::sync::Arc<crate::netplay_relay::RelayState>,
) -> axum::Router {
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    axum::Router::new()
        // Health check for Cloudflare Tunnel
        .route("/api/health", axum::routing::get(|| async { "ok" }))
        // Telemetry pipeline
        .route("/api/v1/telemetry", axum::routing::post(ingest_telemetry))
        .route("/api/telemetry", axum::routing::get(get_telemetry_snapshot))
        .route("/api/telemetry/stream", axum::routing::get(crate::sse::telemetry_sse))
        .route("/api/v1/ws/telemetry", axum::routing::get(crate::ws_telemetry::ws_telemetry_handler))
        // OTA SquashFS staging
        .route("/api/v1/sync", axum::routing::get(crate::ota_sync::list_payloads_handler))
        .route("/api/v1/sync/upload", axum::routing::post(crate::ota_sync::payload_upload_handler))
        .route("/api/v1/sync/:filename", axum::routing::get(crate::ota_sync::payload_download_handler))
        // Dual-Mode Operator Authentication & Overview
        .route("/api/v1/auth/pin", axum::routing::post(verify_operator_pin))
        .route("/api/v1/auth/verify", axum::routing::get(verify_operator_session))
        .route("/api/v1/auth/logout", axum::routing::post(revoke_operator_session))
        .route("/api/v1/admin/overview", axum::routing::get(get_admin_overview))
        // Hybrid Plugin Engine API
        .route("/api/v1/plugins", axum::routing::get(list_plugins))
        .route("/api/v1/plugins/webhooks", axum::routing::post(register_webhook_plugin))
        .route("/api/v1/plugins/:plugin_id/toggle", axum::routing::post(toggle_plugin))
        .route("/api/v1/plugins/:plugin_id/action", axum::routing::post(invoke_plugin_action))
        // Authenticated cabinet RPC
        .route("/api/v1/cabinet/:node_id/reboot", axum::routing::post(cabinet_reboot))
        .route("/api/v1/cabinet/:node_id/ping", axum::routing::post(cabinet_ping))
        .route("/api/v1/cabinet/:node_id/launch_game", axum::routing::post(cabinet_launch_game))
        .route("/api/v1/credits/add", axum::routing::post(credits_add))
        .route("/api/v1/commands/:command_id", axum::routing::get(command_status))
        // Game catalogue & file delivery
        .route("/api/v1/games", axum::routing::get(list_catalogue_games))
        .route("/api/v1/cabinet/:node_id/games/:game_id/file", axum::routing::get(cabinet_game_file))
        .route("/api/v1/cabinet/:node_id/events", axum::routing::post(cabinet_events))
        // Node command plane
        .route("/api/v1/cabinet/:node_id/commands", axum::routing::get(poll_commands))
        .route("/api/v1/cabinet/:node_id/commands/ack", axum::routing::post(ack_command))
        // Node events forwarded to the edge D1 store
        .route("/api/v1/events/error", axum::routing::post(report_error))
        .route("/api/v1/scores", axum::routing::post(submit_score))
        // Node Discovery & Fleet Rescue
        .route("/api/v1/fleet/connect", axum::routing::post(fleet_force_connect))
        .route("/api/v1/fleet/discover", axum::routing::post(fleet_discover))
        .route("/api/v1/fleet/interfaces", axum::routing::get(fleet_interfaces))
        // Venue-wide UI theme
        .route("/api/v1/ui/theme", axum::routing::get(crate::ui_theme::get_theme).post(crate::ui_theme::set_theme))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
        // Netplay relay HTTP endpoints (own state; merged post-with_state).
        .merge(crate::netplay_relay::router(relay_state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;
    use crate::state::{AppState, Config};

    fn test_app() -> (axum::Router, SharedState, String, String) {
        let rpc_token = "test_rpc_secret_token".to_string();
        let node_token = "test_node_secret_token".to_string();
        let config = Config {
            venue_name: "Test Venue Bar".to_string(),
            venue_address: "123 Test St".to_string(),
            rpc_token: rpc_token.clone(),
            node_token: node_token.clone(),
            operator_pin: "898900".to_string(),
            edge_ingest_url: None,
            edge_ingest_token: String::new(),
        };
        let state = Arc::new(AppState::new(config));
        let router = build_router(state.clone());
        (router, state, rpc_token, node_token)
    }

    async fn read_json_body(res: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("failed to read response bytes");
        serde_json::from_slice(&bytes).expect("failed to deserialize response body as JSON")
    }

    // -----------------------------------------------------------------------
    // F5: Operator PIN Brute-Force Rate Limiting Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_f5_pin_valid_returns_200_and_minted_token() {
        let (app, state, _, _) = test_app();

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", "192.168.1.10")
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "898900" })).unwrap()))
            .unwrap();

        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = read_json_body(res).await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["venue"], "Test Venue Bar");

        let token = body["token"].as_str().expect("token should be string");
        assert!(token.starts_with("boss_sess_"));
        assert!(body["expires_at_epoch_ms"].as_u64().unwrap() > 0);

        // Verify minted token is valid for operator RPC
        assert!(state.is_valid_operator_token(token).await);
    }

    #[tokio::test]
    async fn test_f5_pin_bad_pin_decrements_remaining_attempts() {
        let (app, _, _, _) = test_app();
        let client_ip = "192.168.1.20";

        for (attempt, expected_remaining) in [(1, 4), (2, 3), (3, 2), (4, 1)] {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/auth/pin")
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Forwarded-For", client_ip)
                .body(Body::from(serde_json::to_vec(&json!({ "pin": "000000" })).unwrap()))
                .unwrap();

            let res = app.clone().oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "attempt {} failed", attempt);

            let body = read_json_body(res).await;
            assert_eq!(body["ok"], false);
            assert_eq!(body["error"], "invalid_pin");
            assert_eq!(body["remaining_attempts"], expected_remaining);
        }
    }

    #[tokio::test]
    async fn test_f5_pin_fifth_failure_triggers_429_lockout() {
        let (app, _, _, _) = test_app();
        let client_ip = "192.168.1.30";

        for _ in 0..4 {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/auth/pin")
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Forwarded-For", client_ip)
                .body(Body::from(serde_json::to_vec(&json!({ "pin": "000000" })).unwrap()))
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }

        // 5th attempt triggers lockout
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", client_ip)
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "000000" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);

        assert_eq!(
            res.headers().get(header::RETRY_AFTER).unwrap().to_str().unwrap(),
            "60"
        );

        let body = read_json_body(res).await;
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "rate_limited");
        assert_eq!(body["retry_after_secs"], 60);
    }

    #[tokio::test]
    async fn test_f5_pin_subsequent_attempts_locked_out_even_with_valid_pin() {
        let (app, _, _, _) = test_app();
        let client_ip = "192.168.1.40";

        // Exhaust 5 attempts
        for _ in 0..5 {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/auth/pin")
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Forwarded-For", client_ip)
                .body(Body::from(serde_json::to_vec(&json!({ "pin": "000000" })).unwrap()))
                .unwrap();
            let _ = app.clone().oneshot(req).await.unwrap();
        }

        // 6th attempt with CORRECT PIN is blocked because client is locked out
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", client_ip)
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "898900" })).unwrap()))
            .unwrap();

        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);

        let retry_hdr = res.headers().get(header::RETRY_AFTER).unwrap().to_str().unwrap();
        let retry_val: u64 = retry_hdr.parse().unwrap();
        assert!(retry_val > 0 && retry_val <= 60);

        let body = read_json_body(res).await;
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "rate_limited");
    }

    #[tokio::test]
    async fn test_f5_pin_valid_pin_resets_failure_count() {
        let (app, _, _, _) = test_app();
        let client_ip = "192.168.1.50";

        // 3 bad attempts
        for expected in [4, 3, 2] {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/auth/pin")
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Forwarded-For", client_ip)
                .body(Body::from(serde_json::to_vec(&json!({ "pin": "111111" })).unwrap()))
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
            let body = read_json_body(res).await;
            assert_eq!(body["remaining_attempts"], expected);
        }

        // Successful PIN authentication
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", client_ip)
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "898900" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Subsequent bad attempt starts fresh with 4 remaining attempts
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", client_ip)
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "999999" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = read_json_body(res).await;
        assert_eq!(body["remaining_attempts"], 4);
    }

    #[tokio::test]
    async fn test_f5_pin_multi_client_isolation() {
        let (app, _, _, _) = test_app();
        let attacker_ip = "10.10.10.1";
        let legitimate_ip = "10.10.10.2";

        // Attacker exhausts attempts and gets locked out
        for _ in 0..5 {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/auth/pin")
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Forwarded-For", attacker_ip)
                .body(Body::from(serde_json::to_vec(&json!({ "pin": "badpin" })).unwrap()))
                .unwrap();
            let _ = app.clone().oneshot(req).await.unwrap();
        }

        // Attacker is locked out
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", attacker_ip)
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "898900" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);

        // Legitimate client from different IP can log in immediately
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", legitimate_ip)
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "898900" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_f5_pin_ip_header_precedence() {
        let (app, _, _, _) = test_app();

        // 1. CF-Connecting-IP takes precedence over X-Forwarded-For
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("CF-Connecting-IP", "172.20.0.1")
            .header("X-Forwarded-For", "172.20.0.2")
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "bad" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 2. X-Forwarded-For comma separated list takes the first IP
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Forwarded-For", "172.20.0.3, 10.0.0.1")
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "bad" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 3. X-Real-IP is used if CF-Connecting-IP and X-Forwarded-For are absent
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/pin")
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Real-IP", "172.20.0.4")
            .body(Body::from(serde_json::to_vec(&json!({ "pin": "bad" })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // F6: Command Plane Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_f6_command_enqueuing_and_fifo_polling() {
        let (app, _, rpc_token, node_token) = test_app();
        let node_id = "cabinet-redfern-1";

        // 1. Enqueue Reboot command
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({ "delay_secs": 15 })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let reboot_body = read_json_body(res).await;
        let reboot_cmd_id = reboot_body["command_id"].as_str().unwrap().to_string();

        // 2. Enqueue AddCredits command
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/credits/add")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({
                "node_id": node_id,
                "credits": 4
            })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let credits_body = read_json_body(res).await;
        let credits_cmd_id = credits_body["command_id"].as_str().unwrap().to_string();

        // 3. Poll commands for cabinet-redfern-1
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let poll_body = read_json_body(res).await;
        assert_eq!(poll_body["node_id"], node_id);
        let commands = poll_body["commands"].as_array().expect("commands must be array");
        assert_eq!(commands.len(), 2, "must receive exactly 2 commands");

        // Verify FIFO order: reboot first, then add credits
        assert_eq!(commands[0]["id"], reboot_cmd_id);
        assert_eq!(commands[0]["status"], "dispatched");
        assert!(commands[0]["dispatched_at_epoch_ms"].as_u64().unwrap() > 0);

        assert_eq!(commands[1]["id"], credits_cmd_id);
        assert_eq!(commands[1]["status"], "dispatched");
        assert!(commands[1]["dispatched_at_epoch_ms"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_f6_command_polling_drains_queue_second_poll_empty() {
        let (app, _, rpc_token, node_token) = test_app();
        let node_id = "cabinet-redfern-2";

        // Enqueue command
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // First poll drains the queue
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let poll1 = read_json_body(res).await;
        assert_eq!(poll1["commands"].as_array().unwrap().len(), 1);

        // Second poll returns empty array
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let poll2 = read_json_body(res).await;
        assert_eq!(poll2["commands"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_f6_command_ack_success() {
        let (app, _, rpc_token, node_token) = test_app();
        let node_id = "cabinet-redfern-3";

        // 1. Enqueue
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let cmd_id = read_json_body(res).await["command_id"].as_str().unwrap().to_string();

        // 2. Poll
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::empty())
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();

        // 3. Acknowledge success
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/commands/ack", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::from(serde_json::to_vec(&json!({
                "command_id": cmd_id,
                "success": true,
                "detail": { "reboot_initiated": true }
            })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 4. Query command status
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/commands/{}", cmd_id))
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let status_body = read_json_body(res).await;
        assert_eq!(status_body["status"], "acknowledged");
        assert!(status_body["completed_at_epoch_ms"].as_u64().unwrap() > 0);
        assert_eq!(status_body["result"]["reboot_initiated"], true);
    }

    #[tokio::test]
    async fn test_f6_command_ack_failure() {
        let (app, _, rpc_token, node_token) = test_app();
        let node_id = "cabinet-redfern-4";

        // 1. Enqueue
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let cmd_id = read_json_body(res).await["command_id"].as_str().unwrap().to_string();

        // 2. Poll
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::empty())
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();

        // 3. Acknowledge failure
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/commands/ack", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::from(serde_json::to_vec(&json!({
                "command_id": cmd_id,
                "success": false,
                "detail": { "error": "device_busy", "code": 503 }
            })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 4. Query status -> failed
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/commands/{}", cmd_id))
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let status_body = read_json_body(res).await;
        assert_eq!(status_body["status"], "failed");
        assert!(status_body["completed_at_epoch_ms"].as_u64().unwrap() > 0);
        assert_eq!(status_body["result"]["error"], "device_busy");
    }

    #[tokio::test]
    async fn test_f6_command_ack_unknown_command_returns_404() {
        let (app, _, _, node_token) = test_app();
        let node_id = "cabinet-redfern-5";

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/commands/ack", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::from(serde_json::to_vec(&json!({
                "command_id": "00000000-0000-0000-0000-000000000000",
                "success": true,
                "detail": null
            })).unwrap()))
            .unwrap();

        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_f6_command_ack_mismatched_node_returns_403() {
        let (app, _, rpc_token, node_token) = test_app();
        let node_a = "cabinet-owner-node";
        let node_b = "cabinet-impostor-node";

        // Enqueue command for node_a
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_a))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let cmd_id = read_json_body(res).await["command_id"].as_str().unwrap().to_string();

        // Impostor node_b attempts to ACK node_a's command
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/commands/ack", node_b))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::from(serde_json::to_vec(&json!({
                "command_id": cmd_id,
                "success": true,
                "detail": null
            })).unwrap()))
            .unwrap();

        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_f6_command_queue_isolation_between_nodes() {
        let (app, _, rpc_token, node_token) = test_app();
        let node_1 = "cab-iso-1";
        let node_2 = "cab-iso-2";

        // Enqueue command for node_1
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_1))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();

        // Enqueue command for node_2
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_2))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", rpc_token))
            .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();

        // Node 1 poll only gets Node 1 command
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_1))
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let body = read_json_body(res).await;
        let cmds = body["commands"].as_array().unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0]["node_id"], node_1);

        // Node 2 poll only gets Node 2 command
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_2))
            .header(header::AUTHORIZATION, format!("Bearer {}", node_token))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let body = read_json_body(res).await;
        let cmds = body["commands"].as_array().unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0]["node_id"], node_2);
    }

    #[tokio::test]
    async fn test_f6_command_endpoints_require_auth() {
        let (app, _, _, _) = test_app();
        let node_id = "cab-auth-check";

        // Reboot without token -> 401
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/reboot", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // Poll without token -> 401
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_id))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // ACK without token -> 401
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/commands/ack", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&json!({
                "command_id": "test",
                "success": true
            })).unwrap()))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_fleet_discovery_endpoints_auth_and_interfaces() {
        let (app, _state, rpc_token, _node_token) = test_app();

        // 1. Interfaces endpoint without auth -> 401
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/fleet/interfaces")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 2. Interfaces endpoint with RPC auth -> 200
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/fleet/interfaces")
            .header(header::AUTHORIZATION, format!("Bearer {rpc_token}"))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = read_json_body(res).await;
        assert!(body.get("interfaces").is_some());
        assert!(body.get("default_boss_url").is_some());

        // 3. Connect endpoint without auth -> 401
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/fleet/connect")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&json!({
                "target": "127.0.0.1:8990"
            })).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_range_parser_specs() {
        let total = 1000;
        assert_eq!(parse_range_header("bytes=0-499", total), RangeSpec::Partial { start: 0, end: 499, len: 500 });
        assert_eq!(parse_range_header("bytes=500-", total), RangeSpec::Partial { start: 500, end: 999, len: 500 });
        assert_eq!(parse_range_header("bytes=-200", total), RangeSpec::Partial { start: 800, end: 999, len: 200 });
        assert_eq!(parse_range_header("bytes=0-1500", total), RangeSpec::Partial { start: 0, end: 999, len: 1000 });
        assert_eq!(parse_range_header("bytes=1000-", total), RangeSpec::NotSatisfiable);
        assert_eq!(parse_range_header("bytes=500-200", total), RangeSpec::NotSatisfiable);
        assert_eq!(parse_range_header("invalid", total), RangeSpec::Full);
    }

    #[tokio::test]
    async fn test_list_catalogue_games_endpoint() {
        let (app, _, _, _) = test_app();
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/games")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: Vec<GameRef> = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body.len(), 7);
        assert!(body.iter().any(|g| g.game_id == "batman_returns"));
        assert!(body.iter().any(|g| g.game_id == "sfiii3"));
        assert!(body.iter().any(|g| g.game_id == "mvc2"));
    }

    #[tokio::test]
    async fn test_file_delivery_auth_requirement() {
        let (app, _, _, _) = test_app();
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/cabinet/cab-auth/games/batman_returns/file")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_file_delivery_forbidden_when_not_in_session_or_attract() {
        let (app, _, _, node_token) = test_app();
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/cabinet/cab-empty/games/batman_returns/file")
            .header(header::AUTHORIZATION, format!("Bearer {node_token}"))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_file_delivery_streams_when_in_session_and_supports_range() {
        let (app, state, _, node_token) = test_app();
        let node_id = "cab-active-session";
        let game = state.catalogue.read().await.get("batman_returns").unwrap().game.clone();

        // Stage session on state
        state.set_node_session(
            node_id,
            crate::state::ActiveNodeSession {
                session_id: "test-sess-1".into(),
                session_code: "BRC-1234".into(),
                duration_secs: 3600,
                topup_grace_secs: 90,
                games: vec![game],
                expires_at_epoch_ms: now_epoch_ms() + 3600_000,
            },
        ).await;

        // 1. Full content request
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/games/batman_returns/file", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {node_token}"))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::CONTENT_LENGTH).unwrap(),
            "758142"
        );
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes.len(), 758142);

        // 2. Partial Content (Range: bytes=0-99)
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/games/batman_returns/file", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {node_token}"))
            .header(header::RANGE, "bytes=0-99")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            res.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 0-99/758142"
        );
        assert_eq!(res.headers().get(header::CONTENT_LENGTH).unwrap(), "100");
        let partial_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(partial_bytes.len(), 100);
        assert_eq!(&partial_bytes[..], &bytes[0..100]);

        // 3. Unsatisfiable Range
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/games/batman_returns/file", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {node_token}"))
            .header(header::RANGE, "bytes=9000000-")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[tokio::test]
    async fn test_cabinet_events_attract_needed_enqueues_start_attract() {
        let (app, state, _, node_token) = test_app();
        let node_id = "cab-attract-test";

        let event = CabinetEvent::AttractNeeded;
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/events", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {node_token}"))
            .body(Body::from(serde_json::to_vec(&event).unwrap()))
            .unwrap();

        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Verify attract playlist is recorded in state
        let attract_games = state.get_node_attract_playlist(node_id).await;
        assert_eq!(attract_games.len(), 3);
        let systems: std::collections::HashSet<String> = attract_games.iter().map(|g| g.system.clone()).collect();
        assert_eq!(systems.len(), 3, "attract games must come from distinct systems");

        // Verify StartAttract command was queued
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/cabinet/{}/commands", node_id))
            .header(header::AUTHORIZATION, format!("Bearer {node_token}"))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = read_json_body(res).await;
        let cmds = body["commands"].as_array().unwrap();
        assert_eq!(cmds.len(), 1);
        let cmd = &cmds[0]["command"];
        assert_eq!(cmd["type"], "start_attract");
        let cmd_games = cmd["games"].as_array().unwrap();
        assert_eq!(cmd_games.len(), 3);
        assert_eq!(cmd["secs_per_game"], 90);
    }

    #[tokio::test]
    async fn test_cabinet_events_session_ended_clears_session_and_starts_attract() {
        let (app, state, _, node_token) = test_app();
        let node_id = "cab-ended-test";

        // Set active session
        state.set_node_session(
            node_id,
            crate::state::ActiveNodeSession {
                session_id: "sess-to-end".into(),
                session_code: "BRC-9999".into(),
                duration_secs: 1800,
                topup_grace_secs: 90,
                games: Vec::new(),
                expires_at_epoch_ms: now_epoch_ms() + 1800_000,
            },
        ).await;

        assert!(state.get_node_session(node_id).await.is_some());

        // Emit SessionEnded
        let event = CabinetEvent::SessionEnded {
            session_id: "sess-to-end".into(),
            files_wiped: true,
        };
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/cabinet/{}/events", node_id))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {node_token}"))
            .body(Body::from(serde_json::to_vec(&event).unwrap()))
            .unwrap();

        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Verify active session was cleared
        assert!(state.get_node_session(node_id).await.is_none());

        // Verify attract playlist generated
        let attract_games = state.get_node_attract_playlist(node_id).await;
        assert_eq!(attract_games.len(), 3);
    }
}
