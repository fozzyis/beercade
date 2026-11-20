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
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use beercade_shared::crypto::{derive_discovery_key, verify_hmac_sha256_hex};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{Notify, RwLock};

use crate::DaemonSharedState;

pub const DEFAULT_DISCOVERY_PORT: u16 = 8990;
const DISCOVERY_MAX_SKEW_SECS: i64 = 30;

fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_valid_boss_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

#[derive(Clone)]
pub struct DiscoveryState {
    pub daemon_state: Arc<RwLock<DaemonSharedState>>,
    pub reconnect_notify: Arc<Notify>,
    pub discovery_port: u16,
    pub discovery_key: String,
}

impl DiscoveryState {
    async fn resolve_signing_key(&self) -> Option<[u8; 32]> {
        let configured = self.discovery_key.trim();
        if !configured.is_empty() {
            return Some(derive_discovery_key(configured));
        }
        let st = self.daemon_state.read().await;
        let token = st.boss_token.trim();
        if token.is_empty() {
            None
        } else {
            Some(derive_discovery_key(token))
        }
    }
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
    headers: HeaderMap,
    Json(payload): Json<BossConnectPayload>,
) -> impl IntoResponse {
    let clean_boss_url = payload.boss_url.trim().trim_end_matches('/').to_string();
    if clean_boss_url.is_empty() || !is_valid_boss_url(&clean_boss_url) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_boss_url",
                "message": "boss_url must be a non-empty http:// or https:// URL"
            })),
        )
            .into_response();
    }

    let Some(key) = state.resolve_signing_key().await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "discovery_unprovisioned",
                "message": "Discovery key/token not provisioned; rejecting directive"
            })),
        )
            .into_response();
    };

    let ts = headers
        .get("X-Beercade-Timestamp")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<i64>().ok());
    let sig = headers
        .get("X-Beercade-Signature")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);

    let (Some(ts), Some(sig)) = (ts, sig) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "missing_signature",
                "message": "X-Beercade-Timestamp and X-Beercade-Signature required"
            })),
        )
            .into_response();
    };

    if (now_epoch_secs() - ts).abs() > DISCOVERY_MAX_SKEW_SECS {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "stale_signature",
                "message": "Discovery directive timestamp outside allowed skew window"
            })),
        )
            .into_response();
    }

    let token_str = payload.token.as_deref().unwrap_or("");
    let signed_msg = format!("{ts}\n{clean_boss_url}\n{token_str}");
    if !verify_hmac_sha256_hex(&key, signed_msg.as_bytes(), sig) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "invalid_signature",
                "message": "Invalid discovery HMAC signature"
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
        "[✓] Discovery RPC: Verified connect directive to BOSS at {} for node {}",
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

/// Validates and applies a signed UDP `BEERCADE_PROBE` or `BEERCADE_BOSS_BEACON` payload.
/// Returns `true` when authenticated and fresh.
pub async fn process_udp_directive(
    state: &DiscoveryState,
    kind: &str,
    raw_text: &str,
) -> bool {
    let Some(json_idx) = raw_text.find('{') else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw_text[json_idx..]) else {
        return false;
    };
    let Some(boss_url) = v.get("boss_url").and_then(|u| u.as_str()) else {
        return false;
    };
    let clean = boss_url.trim().trim_end_matches('/').to_string();
    if clean.is_empty() || !is_valid_boss_url(&clean) {
        return false;
    }
    let Some(ts) = v.get("ts").and_then(|t| t.as_i64()) else {
        return false;
    };
    let Some(sig) = v.get("sig").and_then(|s| s.as_str()) else {
        return false;
    };
    if (now_epoch_secs() - ts).abs() > DISCOVERY_MAX_SKEW_SECS {
        return false;
    }
    let Some(key) = state.resolve_signing_key().await else {
        return false;
    };
    let msg = format!("{kind}|{ts}|{clean}");
    if !verify_hmac_sha256_hex(&key, msg.as_bytes(), sig) {
        return false;
    }

    {
        let mut st = state.daemon_state.write().await;
        st.boss_url = clean;
    }
    state.reconnect_notify.notify_waiters();
    true
}

/// Spawns the HTTP discovery listener and UDP broadcast responder.
pub fn spawn_discovery_server(
    daemon_state: Arc<RwLock<DaemonSharedState>>,
    reconnect_notify: Arc<Notify>,
    discovery_port: u16,
) {
    let discovery_key = std::env::var("ASTRO_DISCOVERY_KEY").unwrap_or_default();
    let discovery_state = DiscoveryState {
        daemon_state: daemon_state.clone(),
        reconnect_notify: reconnect_notify.clone(),
        discovery_port,
        discovery_key,
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
                if !process_udp_directive(&udp_state, "BEERCADE_PROBE", &text).await {
                    tracing::warn!(%src, "Ignored unauthenticated or stale BEERCADE_PROBE");
                    continue;
                }

                // Send announce reply to authenticated probe
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
                if !process_udp_directive(&udp_state, "BEERCADE_BOSS_BEACON", &text).await {
                    tracing::warn!(%src, "Ignored unauthenticated or stale BEERCADE_BOSS_BEACON");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use beercade_shared::crypto::hmac_sha256_hex;

    fn test_discovery_state(secret: &str) -> DiscoveryState {
        let mut st = crate::tests::sample_daemon_state(
            std::env::temp_dir().join("astro-discovery-test/credits.json"),
        );
        st.boss_url = "http://boss-initial:8080".to_string();
        st.boss_token = secret.to_string();
        DiscoveryState {
            daemon_state: Arc::new(RwLock::new(st)),
            reconnect_notify: Arc::new(Notify::new()),
            discovery_port: DEFAULT_DISCOVERY_PORT,
            discovery_key: String::new(),
        }
    }

    async fn spawn_test_discovery_server(state: DiscoveryState) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = build_discovery_router(state);
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn test_boss_connect_rejects_unauthenticated_and_stale() {
        let state = test_discovery_state("shared-node-secret");
        let base = spawn_test_discovery_server(state.clone()).await;
        let client = reqwest::Client::new();

        // 1. Missing signature headers -> 401 and boss_url untouched
        let res = client
            .post(format!("{base}/api/v1/boss/connect"))
            .json(&serde_json::json!({ "boss_url": "http://attacker:8080", "token": "evil" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(state.daemon_state.read().await.boss_url, "http://boss-initial:8080");

        // 2. Stale timestamp (> 30s) -> 401
        let stale_ts = now_epoch_secs() - 120;
        let msg = format!("{stale_ts}\nhttp://10.0.0.5:8080\nnew-tok");
        let sig = hmac_sha256_hex(&derive_discovery_key("shared-node-secret"), msg.as_bytes());
        let res2 = client
            .post(format!("{base}/api/v1/boss/connect"))
            .header("X-Beercade-Timestamp", stale_ts.to_string())
            .header("X-Beercade-Signature", sig)
            .json(&serde_json::json!({ "boss_url": "http://10.0.0.5:8080", "token": "new-tok" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res2.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(state.daemon_state.read().await.boss_url, "http://boss-initial:8080");
    }

    #[tokio::test]
    async fn test_boss_connect_accepts_valid_hmac() {
        let state = test_discovery_state("shared-node-secret");
        let base = spawn_test_discovery_server(state.clone()).await;
        let client = reqwest::Client::new();

        let ts = now_epoch_secs();
        let boss_url = "http://192.168.100.10:8080";
        let tok = "rotated-token";
        let msg = format!("{ts}\n{boss_url}\n{tok}");
        let sig = hmac_sha256_hex(&derive_discovery_key("shared-node-secret"), msg.as_bytes());

        let res = client
            .post(format!("{base}/api/v1/boss/connect"))
            .header("X-Beercade-Timestamp", ts.to_string())
            .header("X-Beercade-Signature", sig)
            .json(&serde_json::json!({ "boss_url": boss_url, "token": tok }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let st = state.daemon_state.read().await;
        assert_eq!(st.boss_url, boss_url);
        assert_eq!(st.boss_token, tok);
    }

    #[tokio::test]
    async fn test_udp_directive_rejects_forged_and_accepts_signed() {
        let state = test_discovery_state("shared-node-secret");

        // Unsigned BEERCADE_BOSS_BEACON ignored
        assert!(
            !process_udp_directive(
                &state,
                "BEERCADE_BOSS_BEACON",
                r#"BEERCADE_BOSS_BEACON:{"boss_url":"http://attacker:8080"}"#,
            )
            .await
        );
        assert_eq!(state.daemon_state.read().await.boss_url, "http://boss-initial:8080");

        // Signed BEERCADE_BOSS_BEACON updates boss_url
        let ts = now_epoch_secs();
        let url = "http://192.168.100.10:8080";
        let sig = hmac_sha256_hex(
            &derive_discovery_key("shared-node-secret"),
            format!("BEERCADE_BOSS_BEACON|{ts}|{url}").as_bytes(),
        );
        let payload = format!(
            r#"BEERCADE_BOSS_BEACON:{{"boss_url":"{url}","ts":{ts},"sig":"{sig}"}}"#
        );
        assert!(process_udp_directive(&state, "BEERCADE_BOSS_BEACON", &payload).await);
        assert_eq!(state.daemon_state.read().await.boss_url, url);
    }
}
