use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;

use beercade_shared::WsServerMessage;
use crate::auth::RequireNodeToken;
use crate::models::{now_epoch_ms, CommandRecord, CommandStatus, NodeTelemetry};
use crate::plugins::PluginEvent;
use crate::state::{EdgeItem, SharedState, SseChannel};

#[derive(Debug, Default, serde::Deserialize)]
pub struct WsTelemetryQuery {
    #[serde(default)]
    pub node_id: Option<String>,
}

pub async fn ws_telemetry_handler(
    Query(query): Query<WsTelemetryQuery>,
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, query.node_id))
}

async fn register_and_flush(
    state: &SharedState,
    node_id: &str,
    tx: &mpsc::Sender<WsServerMessage>,
) -> u64 {
    let conn_id = state.register_node_ws(node_id, tx.clone()).await;

    // Flush any queued pending_commands so reconnects recover immediately.
    let pending_ids = state
        .pending_commands
        .write()
        .await
        .remove(node_id)
        .unwrap_or_default();
    if !pending_ids.is_empty() {
        let now = now_epoch_ms();
        let flushed: Vec<CommandRecord> = {
            let mut cmds = state.commands.write().await;
            let mut list = Vec::with_capacity(pending_ids.len());
            for id in pending_ids {
                if let Some(record) = cmds.get_mut(&id) {
                    record.status = CommandStatus::Dispatched;
                    record.dispatched_at_epoch_ms = Some(now);
                    list.push(record.clone());
                }
            }
            list
        };
        for record in flushed {
            state.publish(SseChannel::Commands, &record);
            let _ = tx.try_send(WsServerMessage::Command { record });
        }
    }

    // Push current venue UI theme immediately on connect/reconnect.
    let theme = state.ui_theme.read().await.clone();
    let _ = tx.try_send(WsServerMessage::ThemeChanged { theme });

    conn_id
}

async fn handle_socket(
    socket: WebSocket,
    state: SharedState,
    query_node_id: Option<String>,
) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<WsServerMessage>(64);

    let mut registered: Option<(String, u64)> = None;
    if let Some(nid) = query_node_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let conn_id = register_and_flush(&state, nid, &tx).await;
        registered = Some((nid.to_string(), conn_id));
    }

    loop {
        tokio::select! {
            biased;
            outbound = rx.recv() => {
                match outbound {
                    Some(msg) => {
                        if let Ok(text) = serde_json::to_string(&msg) {
                            if sender.send(Message::Text(text)).await.is_err() {
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(payload) = serde_json::from_str::<NodeTelemetry>(&text) {
                            if let Err(reason) = payload.validate() {
                                tracing::warn!(node_id = %payload.node_id, %reason, "rejected invalid telemetry via WS");
                                continue;
                            }

                            let node_id = payload.node_id.clone();
                            if registered.is_none() {
                                let conn_id = register_and_flush(&state, &node_id, &tx).await;
                                registered = Some((node_id.clone(), conn_id));
                            }

                            let now = now_epoch_ms();
                            let mut nodes = state.nodes.write().await;
                            let was_online = nodes.get(&node_id).is_some_and(|(_, last_seen)| {
                                now.saturating_sub(*last_seen) < crate::routes::STALENESS_MS
                            });
                            nodes.insert(node_id.clone(), (payload.clone(), now));
                            drop(nodes);

                            if !was_online {
                                state.publish(
                                    SseChannel::Alerts,
                                    &json!({
                                        "kind": "node_online",
                                        "node_id": node_id,
                                        "at_epoch_ms": now,
                                        "source": "websocket"
                                    }),
                                );
                                state.publish_venue_now().await;
                                state
                                    .plugins
                                    .dispatch_event(
                                        PluginEvent::NodeOnline {
                                            node_id: node_id.clone(),
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
                            state.buffer_edge_item(EdgeItem::Telemetry(payload)).await;
                        }
                    }
                    Some(Ok(Message::Ping(ping))) => {
                        if sender.send(Message::Pong(ping)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    if let Some((node_id, conn_id)) = registered {
        state.unregister_node_ws(&node_id, conn_id).await;
        tracing::debug!(node_id = %node_id, conn_id, "WebSocket telemetry disconnected");
        // We do not immediately emit node_offline.
        // The venue_broadcaster loop in main.rs will naturally emit offline
        // if no heartbeat comes via HTTP or WS within STALENESS_MS.
    }
}
