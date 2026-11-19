//! A.S.T.R.O. Inbound Discovery & Node Control Service.
//!
//! Exposes:
//! 1. HTTP Discovery & Connect API (default TCP port 8990):
//!    - GET /api/v1/node/info (and /api/v1/status): Returns station identity, hardware, mode.
//!    - POST /api/v1/boss/connect: Direct connect command from BOSS Admin Console to point
//!      ASTRO to the supervisor and immediately reconnect.
//! 2. UDP Discovery Responder (default UDP port 8990):
//!    - Responds to `BEERCADE_PROBE` broadcasts across virtual and physical LANs.
//!    - Listens for `BEERCADE_BOSS_BEACON` announcements.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{Notify, RwLock};

use crate::DaemonSharedState;

pub const DEFAULT_DISCOVERY_PORT: u16 = 8990;

#[derive(Clone)]
pub struct DiscoveryState {
    pub daemon_state: Arc<RwLock<DaemonSharedState>>,
    pub reconnect_notify: Arc<Notify>,
    pub discovery_port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeInfoResponse {
    pub node_id: String,
    pub status: String,
    pub mode: serde_json::Value,
    pub hardware: String,
    pub soc: String,
    pub boss_url: String,
    pub discovery_port: u16,
    pub credits: beercade_shared::models::CreditCounters,
}

#[derive(Debug, Deserialize)]
pub struct BossConnectPayload {
    pub boss_url: String,
    pub token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BossConnectResponse {
    pub status: String,
    pub node_id: String,
    pub connected_to: String,
    pub message: String,
}

async fn node_info_handler(State(state): State<DiscoveryState>) -> Json<NodeInfoResponse> {
    let st = state.daemon_state.read().await;
    let (hardware, soc) = beercade_hal::Platform::detect().hardware_info();
    let mode_val = serde_json::to_value(&st.mode).unwrap_or_default();

    Json(NodeInfoResponse {
        node_id: st.node_id.clone(),
        status: "online".to_string(),
        mode: mode_val,
        hardware: hardware.to_string(),
        soc: soc.to_string(),
        boss_url: st.boss_url.clone(),
        discovery_port: state.discovery_port,
        credits: st.credits.clone(),
    })
}

async fn boss_connect_handler(
    State(state): State<DiscoveryState>,
    Json(payload): Json<BossConnectPayload>,
) -> impl IntoResponse {
    let clean_boss_url = payload.boss_url.trim().trim_end_matches('/').to_string();
    if clean_boss_url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "empty_boss_url",
                "message": "boss_url must not be empty"
            })),
        )
            .into_response();
    }

    let node_id = {
        let mut st = state.daemon_state.write().await;
        st.boss_url = clean_boss_url.clone();
        if let Some(token) = payload.token {
            if !token.trim().is_empty() {
                st.boss_token = token;
            }
        }
        st.node_id.clone()
    };

    println!(
        "[✓] Discovery RPC: Received connect directive to BOSS at {} for node {}",
        clean_boss_url, node_id
    );

    // Notify the telemetry client loop to immediately reconnect
    state.reconnect_notify.notify_waiters();

    (
        StatusCode::OK,
        Json(serde_json::json!(BossConnectResponse {
            status: "ok".to_string(),
            node_id,
            connected_to: clean_boss_url,
            message: "Directive applied. Immediate reconnection triggered.".to_string(),
        })),
    )
        .into_response()
}

pub fn build_discovery_router(state: DiscoveryState) -> Router {
    Router::new()
        .route("/api/v1/node/info", get(node_info_handler))
        .route("/api/v1/status", get(node_info_handler))
        .route("/api/v1/boss/connect", post(boss_connect_handler))
        .with_state(state)
}

/// Spawns the HTTP discovery listener and UDP broadcast responder.
pub fn spawn_discovery_server(
    daemon_state: Arc<RwLock<DaemonSharedState>>,
    reconnect_notify: Arc<Notify>,
    discovery_port: u16,
) {
    let discovery_state = DiscoveryState {
        daemon_state: daemon_state.clone(),
        reconnect_notify: reconnect_notify.clone(),
        discovery_port,
    };

    // 1. HTTP Discovery Service
    let http_state = discovery_state.clone();
    tokio::spawn(async move {
        let app = build_discovery_router(http_state);
        let bind_addr = SocketAddr::from(([0, 0, 0, 0], discovery_port));
        match TcpListener::bind(bind_addr).await {
            Ok(listener) => {
                println!(
                    "[✓] ASTRO Inbound Discovery Service listening on http://0.0.0.0:{}",
                    discovery_port
                );
                if let Err(e) = axum::serve(listener, app).await {
                    eprintln!("[!] ASTRO Discovery HTTP server error: {e}");
                }
            }
            Err(e) => {
                eprintln!("[!] Failed to bind ASTRO Discovery HTTP port {discovery_port}: {e}");
            }
        }
    });

    // 2. UDP Broadcast Discovery Responder
    let udp_state = discovery_state.clone();
    tokio::spawn(async move {
        let bind_addr = SocketAddr::from(([0, 0, 0, 0], discovery_port));
        let socket = match UdpSocket::bind(bind_addr).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[!] Failed to bind ASTRO Discovery UDP port {discovery_port}: {e}");
                return;
            }
        };

        println!(
            "[✓] ASTRO UDP Discovery Responder listening on udp://0.0.0.0:{}",
            discovery_port
        );

        let mut buf = [0u8; 1024];
        loop {
            let (len, src) = match socket.recv_from(&mut buf).await {
                Ok(res) => res,
                Err(_) => continue,
            };

            let text = String::from_utf8_lossy(&buf[..len]);

            if text.starts_with("BEERCADE_PROBE") {
                // If the probe included a boss_url directive, extract it
                if let Some(json_idx) = text.find('{') {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text[json_idx..]) {
                        if let Some(boss_url) = v.get("boss_url").and_then(|u| u.as_str()) {
                            let clean = boss_url.trim().trim_end_matches('/').to_string();
                            if !clean.is_empty() {
                                {
                                    let mut st = udp_state.daemon_state.write().await;
                                    st.boss_url = clean;
                                }
                                udp_state.reconnect_notify.notify_waiters();
                            }
                        }
                    }
                }

                // Send announce reply
                let node_id = {
                    let st = udp_state.daemon_state.read().await;
                    st.node_id.clone()
                };

                let reply = serde_json::json!({
                    "event": "BEERCADE_NODE_ANNOUNCE",
                    "node_id": node_id,
                    "discovery_port": discovery_port,
                    "status": "online"
                })
                .to_string();

                let _ = socket.send_to(reply.as_bytes(), src).await;
            } else if text.starts_with("BEERCADE_BOSS_BEACON") {
                if let Some(json_idx) = text.find('{') {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text[json_idx..]) {
                        if let Some(boss_url) = v.get("boss_url").and_then(|u| u.as_str()) {
                            let clean = boss_url.trim().trim_end_matches('/').to_string();
                            if !clean.is_empty() {
                                {
                                    let mut st = udp_state.daemon_state.write().await;
                                    st.boss_url = clean;
                                }
                                udp_state.reconnect_notify.notify_waiters();
                            }
                        }
                    }
                }
            }
        }
    });
}
