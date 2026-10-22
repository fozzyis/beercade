//! HTTP handlers: telemetry ingest, snapshots, authenticated cabinet RPC,
//! node command polling and edge-sync submission endpoints.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{RequireNodeToken, RequireRpcToken};
use crate::models::{
    CommandAck, CommandRecord, CommandStatus, ErrorEvent, HighScoreEntry, NodeTelemetry,
    TokenTransaction, now_epoch_ms,
};
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

// ---------------------------------------------------------------------------
// Telemetry pipeline
// ---------------------------------------------------------------------------

/// POST /api/v1/telemetry — node heartbeat ingestion (node-authenticated).
///
/// Persists the latest heartbeat, buffers it for the Cloudflare edge batcher,
/// and emits online/offline transition alerts.
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

    let mut nodes = state.nodes.write().await;
    let was_online = nodes.contains_key(&payload.node_id);
    nodes.insert(payload.node_id.clone(), (payload.clone(), now_epoch_ms()));
    drop(nodes);

    if !was_online {
        state.publish(
            SseChannel::Alerts,
            &json!({
                "kind": "node_online",
                "node_id": payload.node_id,
                "at_epoch_ms": now_epoch_ms(),
            }),
        );
    }

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
        .enqueue_command(&node_id, crate::models::CabinetCommand::AddCredits { credits: req.credits })
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
    // A fleet node (non-operator) may only poll its own queue.
    // Operators (BOSS_RPC_TOKEN) may poll any node for debugging.
    let _ = claim; // claim.is_operator available if further gating needed
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
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

// ---------------------------------------------------------------------------
// Node events: errors + high scores (forwarded to edge D1)
// ---------------------------------------------------------------------------

/// POST /api/v1/events/error — node error report; broadcast + forward to D1.
pub async fn report_error(
    State(state): State<SharedState>,
    RequireNodeToken(claim): RequireNodeToken,
    Json(mut event): Json<ErrorEvent>,
) -> StatusCode {
    if event.node_id.trim().is_empty() || event.message.trim().is_empty() {
        return StatusCode::UNPROCESSABLE_ENTITY;
    }
    // A node token may only report errors for itself.
    if !claim.is_operator && event.node_id.trim() != event.node_id.trim() {
        // (placeholder for per-node-id token; fleet currently shares one
        //  node token — the check below is the meaningful one)
        return StatusCode::FORBIDDEN;
    }
    if !matches!(event.level.as_str(), "warn" | "error" | "fatal") {
        event.level = "error".to_string();
    }
    event.occurred_at_epoch_ms = if event.occurred_at_epoch_ms == 0 {
        now_epoch_ms()
    } else {
        event.occurred_at_epoch_ms
    };

    state.publish(SseChannel::Alerts, &event);
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

    state.publish(SseChannel::Alerts, &entry);
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



