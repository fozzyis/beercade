use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde_json::json;
use std::path::Path as FsPath;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use beercade_shared::crypto::Sha256Hasher;
use beercade_shared::WsServerMessage;

use crate::auth::RequireRpcToken;
use crate::models::now_epoch_ms;
use crate::state::{SharedState, SseChannel};

/// Staging directory shared by the upload gateway and the node sync endpoint.
const SYNC_DIR: &'static str = "/data/beercade/sync";

fn sync_dir() -> String {
    std::env::var("BOSS_SYNC_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| SYNC_DIR.to_string())
}

/// Hard ceiling for a single OTA payload. Enforced on both the `Content-Length`
/// header and every streamed chunk so a malicious (or misbehaving) client can
/// never fill `/data` (disk-fill DoS).
const MAX_PAYLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024; // 4 GiB

/// Normalise a node-supplied filename so it can never escape the sync dir.
fn sanitize_filename(name: &str) -> String {
    name.replace('/', "_").replace('\\', "_")
}

/// Resolve the payload ceiling, allowing operators to lower it per-deploy.
fn max_payload_bytes() -> u64 {
    std::env::var("BOSS_MAX_PAYLOAD_BYTES")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(MAX_PAYLOAD_BYTES)
}

/// GET /api/v1/sync/{filename} — stream a staged SquashFS payload to a node.
///
/// Served straight off disk with an explicit `Content-Length` so the daemon can
/// budget free space *before* it begins writing its staging area, and streamed
/// via `ReaderStream` so huge images are never buffered in memory.
pub async fn payload_download_handler(Path(filename): Path<String>) -> Response {
    let safe = sanitize_filename(&filename);
    let path_str = format!("{}/{}", sync_dir(), safe);
    let path = FsPath::new(&path_str);

    match File::open(path).await {
        Err(e) => {
            tracing::warn!(path = %path_str, %e, "OTA payload requested but not found");
            (StatusCode::NOT_FOUND, Json(json!({
                "error": "not_found",
                "filename": safe,
            })))
                .into_response()
        }
        Ok(file) => {
            let size = file
                .metadata()
                .await
                .map(|m| m.len())
                .unwrap_or(0);

            let mut resp = Response::builder()
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, size)
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", safe),
                );

            let sha256_path = format!("{}.sha256", path_str);
            if let Ok(sha256_hex) = tokio::fs::read_to_string(&sha256_path).await {
                if !sha256_hex.trim().is_empty() {
                    resp = resp.header("X-OTA-SHA256", sha256_hex.trim());
                }
            }

            // 1 MiB read buffer keeps disk I/O efficient for multi-GB images.
            let stream = ReaderStream::with_capacity(file, 1024 * 1024);
            resp.body(Body::from_stream(stream))
                .unwrap_or_else(|e| {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("build sync response: {e}"))
                        .into_response()
                })
        }
    }
}

/// POST /api/v1/sync/upload — store an OTA payload under the sync directory.
///
/// The `Content-Length` header is enforced strictly, and re-checked as the body
/// streams, so callers cannot exhaust `/data` with an unbounded upload (or one
/// that lies about its size).
pub async fn payload_upload_handler(
    State(state): State<SharedState>,
    _auth: RequireRpcToken,
    headers: HeaderMap,
    request: Request<Body>,
) -> Response {
    let max_payload = max_payload_bytes();

    // Strict pre-flight gate: require an honest, in-range Content-Length.
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(0);

    if content_length == 0 {
        return (StatusCode::LENGTH_REQUIRED, Json(json!({
            "error": "length_required",
            "hint": "Provide a Content-Length header.",
        })))
            .into_response();
    }
    if content_length > max_payload {
        tracing::warn!(
            content_length,
            max_payload,
            "rejected oversized OTA payload (Content-Length exceeded limit)"
        );
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({
            "error": "payload_too_large",
            "limit_bytes": max_payload,
        })))
            .into_response();
    }

    let filename = request
        .headers()
        .get("x-payload-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("payload.bin")
        .to_string();

    // Sanitize filename roughly to prevent path traversal.
    let safe_filename = sanitize_filename(&filename);

    let dir = sync_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let path_str = format!("{}/{}", dir, safe_filename);
    let path = FsPath::new(&path_str);

    let mut file = match File::create(path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Failed to create OTA file {}: {}", path_str, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": "internal_error",
            })))
                .into_response();
        }
    };

    let mut data_stream = request.into_body().into_data_stream();

    let mut total_bytes = 0;
    let mut too_large = false;
    let mut hasher = Sha256Hasher::new();
    while let Some(chunk) = data_stream.next().await {
        match chunk {
            Ok(bytes) => {
                hasher.update(&bytes);
                total_bytes += bytes.len();
                if total_bytes as u64 > max_payload {
                    too_large = true;
                    break;
                }
                if let Err(e) = file.write_all(&bytes).await {
                    tracing::error!("Error writing OTA chunk to {}: {}", path_str, e);
                    drop(file);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                        "error": "write_failed",
                    })))
                        .into_response();
                }
            }
            Err(e) => {
                tracing::error!("Error reading OTA chunk from network: {}", e);
                drop(file);
                return (StatusCode::BAD_REQUEST, Json(json!({
                    "error": "read_failed",
                })))
                    .into_response();
            }
        }
    }

    if too_large {
        // Release the handle, then remove the partial image so no junk lingers.
        drop(file);
        let _ = tokio::fs::remove_file(path).await;
        tracing::warn!(
            total_bytes,
            max_payload,
            "streamed OTA payload exceeded limit; discarded partial upload"
        );
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({
            "error": "payload_too_large",
            "limit_bytes": max_payload,
        })))
            .into_response();
    }

    tracing::info!("Received payload '{}', {} bytes.", safe_filename, total_bytes);

    let hash_hex = hasher.finalize_hex();
    let sha256_path_str = format!("{}.sha256", path_str);
    if let Err(e) = tokio::fs::write(&sha256_path_str, &hash_hex).await {
        tracing::error!("Failed to write sha256 file for {}: {}", path_str, e);
    }

    // Notify connected nodes about the new payload over SSE and WS mux.
    state.publish(
        SseChannel::Commands,
        &json!({
            "kind": "payload_available",
            "filename": safe_filename,
            "sha256": hash_hex,
            "size_bytes": total_bytes,
            "at_epoch_ms": now_epoch_ms(),
        }),
    );
    state
        .broadcast_ws(WsServerMessage::PayloadAvailable {
            filename: safe_filename.clone(),
            sha256: Some(hash_hex),
            size_bytes: Some(total_bytes as u64),
        })
        .await;

    (StatusCode::OK, Json(json!({
        "status": "ok",
        "filename": safe_filename,
        "size_bytes": total_bytes,
    })))
        .into_response()
}

/// GET /api/v1/sync — list staged SquashFS OTA payloads in `/data/beercade/sync`.
pub async fn list_payloads_handler(_auth: RequireRpcToken) -> Json<serde_json::Value> {
    let sync_dir = sync_dir();
    let mut payloads = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&sync_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".sha256") {
                continue;
            }
            if let Ok(meta) = entry.metadata().await {
                if meta.is_file() {
                    let sha_path = format!("{}/{}.sha256", sync_dir, name);
                    let sha256 = tokio::fs::read_to_string(&sha_path)
                        .await
                        .ok()
                        .map(|s| s.trim().to_string());
                    payloads.push(json!({
                        "filename": name,
                        "size_bytes": meta.len(),
                        "sha256": sha256,
                    }));
                }
            }
        }
    }
    Json(json!({ "sync_dir": sync_dir, "payloads": payloads }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("valid.bin"), "valid.bin");
        assert_eq!(sanitize_filename("foo/bar.bin"), "foo_bar.bin");
        assert_eq!(sanitize_filename("foo\\bar.bin"), "foo_bar.bin");
        assert_eq!(sanitize_filename("../bar.bin"), ".._bar.bin");
    }
}
