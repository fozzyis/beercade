//! Venue-wide UI theme for the kiosk, `/admin`, `/tv` and A.S.T.R.O. overlays.
//!
//! The active theme id is persisted to `BOSS_UI_THEME_FILE`
//! (default `/data/beercade/state/ui_theme`) so it survives restarts and OTA
//! updates, and every change is pushed to open screens on the `ui` SSE channel.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;

pub use beercade_shared::theme::{AVAILABLE_THEMES, DEFAULT_THEME, is_known_theme};
use beercade_shared::WsServerMessage;

use crate::auth::RequireRpcToken;
use crate::state::{SharedState, SseChannel};

fn theme_file() -> Option<String> {
    if let Ok(val) = std::env::var("BOSS_UI_THEME_FILE") {
        let trimmed = val.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    if cfg!(test) {
        None
    } else {
        Some("/data/beercade/state/ui_theme".to_string())
    }
}

/// Initial theme: persisted file, then `BOSS_UI_THEME`, then the default.
pub fn load_initial() -> String {
    let from_file = theme_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string());
    let from_env = std::env::var("BOSS_UI_THEME").ok().map(|s| s.trim().to_string());
    from_file
        .into_iter()
        .chain(from_env)
        .find(|t| is_known_theme(t))
        .unwrap_or_else(|| DEFAULT_THEME.to_string())
}

#[derive(Deserialize)]
pub struct SetThemeRequest {
    pub theme: String,
}

/// Visual effects level for screens: `full`, or `lite` (no blur, glows or
/// fixed backgrounds) for weak GPUs such as a Raspberry Pi 4's VideoCore VI.
/// `BOSS_UI_EFFECTS` overrides; otherwise ARM appliances default to `lite`.
pub fn effects_level() -> &'static str {
    match std::env::var("BOSS_UI_EFFECTS").ok().as_deref().map(str::trim) {
        Some("full") => "full",
        Some("lite") => "lite",
        _ if cfg!(target_arch = "aarch64") => "lite",
        _ => "full",
    }
}

/// `GET /api/v1/ui/theme` — public, so patron screens can theme themselves.
pub async fn get_theme(State(state): State<SharedState>) -> impl IntoResponse {
    let theme = state.ui_theme.read().await.clone();
    Json(json!({ "theme": theme, "available": AVAILABLE_THEMES, "effects": effects_level() }))
}

/// `POST /api/v1/ui/theme` — operator only.
pub async fn set_theme(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    Json(req): Json<SetThemeRequest>,
) -> impl IntoResponse {
    let theme = req.theme.trim().to_string();
    if !is_known_theme(&theme) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unknown_theme", "available": AVAILABLE_THEMES })),
        );
    }

    *state.ui_theme.write().await = theme.clone();
    if let Some(path) = theme_file() {
        if let Some(dir) = std::path::Path::new(&path).parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        if let Err(e) = tokio::fs::write(&path, format!("{theme}\n")).await {
            tracing::warn!(error = %e, "could not persist UI theme; it will reset on restart");
        }
    }
    state.publish(SseChannel::Ui, &json!({ "kind": "theme_changed", "theme": theme }));
    state
        .broadcast_ws(WsServerMessage::ThemeChanged {
            theme: theme.clone(),
        })
        .await;

    (StatusCode::OK, Json(json!({ "theme": theme })))
}
