//! Bearer-token and Dual-Mode PIN authentication for B.O.S.S. RPC / control endpoints.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub use beercade_shared::auth::{constant_time_eq, NodeClaim};
use crate::state::SharedState;

/// Extractor that fails closed unless the request presents
/// `Authorization: Bearer <BOSS_RPC_TOKEN>` or an active operator session token
/// minted via `POST /api/v1/auth/pin`.
pub struct RequireRpcToken;

#[axum::async_trait]
impl FromRequestParts<SharedState> for RequireRpcToken {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let presented = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .unwrap_or("");

        if state.is_valid_operator_token(presented).await {
            Ok(RequireRpcToken)
        } else {
            tracing::warn!(path = %parts.uri.path(), "rejected unauthorised RPC request");
            Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "unauthorized",
                    "hint": "Provide 'Authorization: Bearer <BOSS_RPC_TOKEN>' or unlock via operator PIN."
                })),
            )
                .into_response())
        }
    }
}

/// Extractor for node-facing ingest endpoints.
///
/// Accepts either `BOSS_NODE_TOKEN` (fleet nodes), `BOSS_RPC_TOKEN`, or an
/// active operator session token.
pub struct RequireNodeToken(pub NodeClaim);

#[axum::async_trait]
impl FromRequestParts<SharedState> for RequireNodeToken {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let presented = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .unwrap_or("");

        if state.is_valid_operator_token(presented).await {
            return Ok(RequireNodeToken(NodeClaim { is_operator: true }));
        }
        if constant_time_eq(presented.as_bytes(), state.config.node_token.as_bytes()) {
            return Ok(RequireNodeToken(NodeClaim { is_operator: false }));
        }

        tracing::warn!(path = %parts.uri.path(), "rejected unauthorised node request");
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "unauthorized",
                "hint": "Provide 'Authorization: Bearer <BOSS_NODE_TOKEN>'."
            })),
        )
            .into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq_delegation() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secret1"));
    }
}
