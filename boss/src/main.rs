//! B.O.S.S. — B.E.E.R.C.A.D.E. Operations & Supervisory System daemon.
//!
//! Central telemetry hub, cabinet control plane (RPC), Hybrid Event-Hook + UI
//! Panel Plugin Engine, and Cloudflare edge sync bridge for the ASTRO cabinet fleet.

mod auth;
pub mod catalogue;
pub mod discovery;
mod edge;
mod models;
mod netplay_relay;
mod ota_sync;
mod plugins;
mod routes;
mod sse;
mod state;
mod ui_theme;
mod ws_telemetry;

use std::sync::Arc;
use std::time::Duration;

use tower_http::services::{ServeDir, ServeFile};

use plugins::PluginEvent;
use state::{AppState, Config};

/// Period between venue-wide SSE broadcasts on the `venue` channel.
const BROADCAST_INTERVAL: Duration = Duration::from_secs(1);

/// Every second, publish the aggregated venue report and detect node
/// online/offline transitions for the alerts channel and plugin bus.
async fn venue_broadcaster(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(BROADCAST_INTERVAL);
    let mut previously_online: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        interval.tick().await;

        let currently_online = state.online_node_ids(routes::STALENESS_MS).await;

        let went_offline: Vec<String> = previously_online
            .difference(&currently_online)
            .cloned()
            .collect();
        if !went_offline.is_empty() {
            let now = models::now_epoch_ms();
            state.publish(
                state::SseChannel::Alerts,
                &serde_json::json!({
                    "kind": "node_offline",
                    "node_ids": went_offline,
                    "at_epoch_ms": now,
                }),
            );
            state
                .plugins
                .dispatch_event(
                    PluginEvent::NodeOffline {
                        node_ids: went_offline,
                        at_epoch_ms: now,
                    },
                    &state,
                )
                .await;
        }
        previously_online = currently_online;

        // Only build and serialise the full report when a screen is listening.
        if state.has_subscribers(state::SseChannel::Venue) {
            let report = state.build_venue_report(routes::STALENESS_MS).await;
            state.publish(state::SseChannel::Venue, &report);
        }
    }
}

/// SvelteKit content-hashes everything under `/_app/immutable/`, so screens may
/// cache those files forever; HTML and API responses are left alone.
async fn cache_immutable_assets(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let immutable = req.uri().path().starts_with("/_app/immutable/");
    let mut res = next.run(req).await;
    if immutable && res.status().is_success() {
        res.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    res
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

    // Load optional out-of-tree JSON plugin manifests
    let plugins_dir = std::env::var("BOSS_PLUGINS_DIR").unwrap_or_else(|_| "/data/beercade/plugins".to_string());
    state
        .plugins
        .load_external_manifests(std::path::Path::new(&plugins_dir))
        .await;

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

    let app = routes::build_router_with_relay(state, relay_state);

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
    // Serve the .br/.gz siblings written at build time when the client accepts them.
    let serve_dir = ServeDir::new(&static_dir)
        .precompressed_br()
        .precompressed_gzip()
        .fallback(ServeFile::new(index_file).precompressed_br().precompressed_gzip());
    let app = app
        .fallback_service(serve_dir)
        .layer(axum::middleware::from_fn(cache_immutable_assets));

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
