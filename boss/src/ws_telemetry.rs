use axum::extract::ws::{WebSocket, WebSocketUpgrade, Message};
use axum::response::IntoResponse;
use axum::extract::State;
use futures_util::StreamExt;
use serde_json::json;
use crate::state::{SharedState, SseChannel, EdgeItem};
use crate::models::{NodeTelemetry, now_epoch_ms};
use crate::auth::RequireNodeToken;
use crate::plugins::PluginEvent;

pub async fn ws_telemetry_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    RequireNodeToken(_claim): RequireNodeToken,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: SharedState) {
    let mut node_id_opt: Option<String> = None;

    while let Some(msg) = socket.next().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(text) => {
                    if let Ok(payload) = serde_json::from_str::<NodeTelemetry>(&text) {
                        if let Err(reason) = payload.validate() {
                            tracing::warn!(node_id = %payload.node_id, %reason, "rejected invalid telemetry via WS");
                            continue;
                        }

                        let node_id = payload.node_id.clone();
                        node_id_opt = Some(node_id.clone());
                        let now = now_epoch_ms();
                        
                        let mut nodes = state.nodes.write().await;
                        let was_online = nodes.contains_key(&node_id);
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
                Message::Ping(ping) => {
                    let _ = socket.send(Message::Pong(ping)).await;
                }
                _ => {}
            }
        } else {
            break; // connection error
        }
    }

    if let Some(node_id) = node_id_opt {
        tracing::debug!(node_id = %node_id, "WebSocket telemetry disconnected");
        // We do not immediately emit node_offline. 
        // The venue_broadcaster loop in main.rs will naturally emit offline 
        // if no heartbeat comes via HTTP or WS within STALENESS_MS.
    }
}
