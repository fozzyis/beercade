//! Venue-wide UI theme for the kiosk, `/admin`, `/tv` and A.S.T.R.O. overlays.
//!
//! The active theme id is persisted to `BOSS_UI_THEME_FILE`
//! (default `/data/beercade/state/ui_theme`) so it survives restarts and OTA
//! updates, and every change is pushed to open screens on the `ui` SSE channel.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;

use crate::auth::RequireRpcToken;
use crate::state::{SharedState, SseChannel};

/// Theme ids the kiosk bundle ships tokens for. Keep in sync with
/// `kiosk_ui/src/lib/theme/themes.ts`.
pub const AVAILABLE_THEMES: &[&str] = &["taproom", "vector", "bigsky", "pinball", "classic"];
pub const DEFAULT_THEME: &str = "taproom";

fn theme_file() -> String {
    std::env::var("BOSS_UI_THEME_FILE").unwrap_or_else(|_| "/data/beercade/state/ui_theme".to_string())
}

fn is_known(theme: &str) -> bool {
    AVAILABLE_THEMES.contains(&theme)
}

/// Initial theme: persisted file, then `BOSS_UI_THEME`, then the default.
pub fn load_initial() -> String {
    let from_file = std::fs::read_to_string(theme_file())
        .ok()
        .map(|s| s.trim().to_string());
    let from_env = std::env::var("BOSS_UI_THEME").ok().map(|s| s.trim().to_string());
    from_file
        .into_iter()
        .chain(from_env)
        .find(|t| is_known(t))
        .unwrap_or_else(|| DEFAULT_THEME.to_string())
}

#[derive(Deserialize)]
pub struct SetThemeRequest {
    pub theme: String,
}

/// `GET /api/v1/ui/theme` — public, so patron screens can theme themselves.
pub async fn get_theme(State(state): State<SharedState>) -> impl IntoResponse {
    let theme = state.ui_theme.read().await.clone();
    Json(json!({ "theme": theme, "available": AVAILABLE_THEMES }))
}

/// `POST /api/v1/ui/theme` — operator only.
pub async fn set_theme(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Json(req): Json<SetThemeRequest>,
) -> impl IntoResponse {
    let theme = req.theme.trim().to_string();
    if !is_known(&theme) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unknown_theme", "available": AVAILABLE_THEMES })),
        );
    }

    *state.ui_theme.write().await = theme.clone();
    let path = theme_file();
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    if let Err(e) = tokio::fs::write(&path, format!("{theme}\n")).await {
        tracing::warn!(error = %e, "could not persist UI theme; it will reset on restart");
    }
    state.publish(SseChannel::Ui, &json!({ "kind": "theme_changed", "theme": theme }));

    (StatusCode::OK, Json(json!({ "theme": theme })))
}
