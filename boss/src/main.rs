//! B.O.S.S. — B.E.E.R.C.A.D.E. Operations & Supervisory System daemon.
//!
//! Central telemetry hub, cabinet control plane (RPC) and Cloudflare edge
//! sync bridge for the ASTRO cabinet fleet.

mod auth;
mod edge;
mod models;
mod netplay_relay;
mod routes;
mod sse;
mod state;
mod ws_telemetry;
mod ota_sync;

use std::sync::Arc;
use std::time::Duration;

use axum::{routing::get, Router};
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use state::{AppState, Config};

/// Period between venue-wide SSE broadcasts on the `venue` channel.
const BROADCAST_INTERVAL: Duration = Duration::from_secs(1);

/// Every second, publish the aggregated venue report and detect node
/// online/offline transitions for the alerts channel.
async fn venue_broadcaster(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(BROADCAST_INTERVAL);
    let mut previously_online: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        interval.tick().await;

        let report = state.build_venue_report(routes::STALENESS_MS).await;

        let currently_online: std::collections::HashSet<String> =
            report.nodes.iter().map(|n| n.node_id.clone()).collect();

        let went_offline: Vec<&String> = previously_online.difference(&currently_online).collect();
        if !went_offline.is_empty() {
            state.publish(
                state::SseChannel::Alerts,
                &serde_json::json!({
                    "kind": "node_offline",
                    "node_ids": went_offline,
                    "at_epoch_ms": models::now_epoch_ms(),
                }),
            );
        }
        previously_online = currently_online;

        state.publish(state::SseChannel::Venue, &report);
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    let state = Arc::new(AppState::new(config));

    // Ensure OTA sync directory exists
    let sync_dir = std::env::var("BOSS_SYNC_DIR").unwrap_or_else(|_| "/data/beercade/sync".to_string());
    if let Err(e) = tokio::fs::create_dir_all(&sync_dir).await {
        tracing::warn!("Could not create sync dir {}: {}", sync_dir, e);
    }

    tokio::spawn(venue_broadcaster(state.clone()));
    tokio::spawn(edge::run_edge_forwarder(state.clone()));

    // Netplay discovery relay + telemetry arbiter (UDP forwarding plane).
    let relay_state = Arc::new(netplay_relay::RelayState::new());
    let relay_port: u16 = std::env::var("BOSS_NETPLAY_UDP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9750);
    match netplay_relay::bind_udp_relay(
        relay_state.clone(),
        &format!("0.0.0.0:{relay_port}"),
    )
    .await
    {
        Ok(addr) => println!("[✓] Netplay relay listening on udp://{addr}"),
        Err(e) => eprintln!("[!] Netplay relay disabled: {e}"),
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Health check for Cloudflare Tunnel
        .route("/api/health", get(|| async { "ok" }))
        // Telemetry pipeline
        .route("/api/v1/telemetry", axum::routing::post(routes::ingest_telemetry))
        .route("/api/telemetry", get(routes::get_telemetry_snapshot))
        .route("/api/telemetry/stream", get(sse::telemetry_sse))
        .route("/api/v1/ws/telemetry", get(ws_telemetry::ws_telemetry_handler))
        .route("/api/v1/sync/upload", axum::routing::post(ota_sync::payload_upload_handler))
        .route("/api/v1/sync/:filename", get(ota_sync::payload_download_handler))
        // Authenticated cabinet RPC
        .route(
            "/api/v1/cabinet/:node_id/reboot",
            axum::routing::post(routes::cabinet_reboot),
        )
        .route(
            "/api/v1/cabinet/:node_id/launch_game",
            axum::routing::post(routes::cabinet_launch_game),
        )
        .route("/api/v1/credits/add", axum::routing::post(routes::credits_add))
        .route("/api/v1/commands/:command_id", get(routes::command_status))
        // Node command plane
        .route("/api/v1/cabinet/:node_id/commands", get(routes::poll_commands))
        .route(
            "/api/v1/cabinet/:node_id/commands/ack",
            axum::routing::post(routes::ack_command),
        )
        // Node events forwarded to the edge D1 store
        .route("/api/v1/events/error", axum::routing::post(routes::report_error))
        .route("/api/v1/scores", axum::routing::post(routes::submit_score))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
        // Netplay relay HTTP endpoints (own state; merged post-with_state).
        .merge(netplay_relay::router(relay_state));

    let static_dir = std::env::var("BOSS_STATIC_DIR").unwrap_or_else(|_| {
        if std::path::Path::new("/usr/share/boss/kiosk_ui").exists() {
            "/usr/share/boss/kiosk_ui".to_string()
        } else if std::path::Path::new("./kiosk_ui/build").exists() {
            "./kiosk_ui/build".to_string()
        } else if std::path::Path::new("./BOSS/kiosk_ui/build").exists() {
            "./BOSS/kiosk_ui/build".to_string()
        } else {
            "./kiosk_ui/build".to_string()
        }
    });

    let index_file = format!("{}/index.html", static_dir);
    let serve_dir = ServeDir::new(&static_dir).fallback(ServeFile::new(index_file));
    let app = app.fallback_service(serve_dir);

    let host = std::env::var("BOSS_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("BOSS_PORT").unwrap_or_else(|_| "8989".to_string());
    let bind_addr = format!("{}:{}", host, port);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    println!("===============================================================");
    println!("   B.O.S.S. Central Server Active on http://{}", bind_addr);
    println!("   Serving Kiosk UI from: {}", static_dir);
    println!("===============================================================");
    axum::serve(listener, app).await.unwrap();
}
