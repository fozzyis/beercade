//! Cloudflare edge sync: batches telemetry/errors/transactions/scores and
//! pushes them to the SvelteKit Worker ingest endpoint over the tunnel.

use std::time::Duration;

use crate::models::{EdgeBatchEnvelope, now_epoch_ms};
use crate::state::{AppState, EdgeItem};

/// Flush cadence: fast enough for near-real-time analytics, sparse enough to
/// stay well inside Cloudflare Worker request limits.
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
/// Give up on an oversized backlog after this many consecutive failures.
const MAX_CONSECUTIVE_FAILURES: u32 = 10;

/// Long-running task that drains `AppState::edge_buffer` into the edge worker.
pub async fn run_edge_forwarder(state: std::sync::Arc<AppState>) {
    let Some(url) = state.config.edge_ingest_url.clone() else {
        tracing::info!("EDGE_INGEST_URL not set - cloud edge sync disabled (local-only mode)");
        return;
    };
    if state.config.edge_ingest_token.is_empty() {
        tracing::warn!("EDGE_INGEST_TOKEN not set - cloud edge sync disabled");
        return;
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            tracing::error!(%err, "failed to build HTTP client - edge sync disabled");
            return;
        }
    };

    tracing::info!(%url, "cloud edge sync active");

    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut consecutive_failures: u32 = 0;

    loop {
        interval.tick().await;

        let items: Vec<EdgeItem> = {
            let mut buf = state.edge_buffer.lock().await;
            buf.drain(..).collect()
        };
        if items.is_empty() {
            continue;
        }

        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            tracing::error!(
                dropped = items.len(),
                "edge sync failing repeatedly - dropping batch to bound memory"
            );
            continue;
        }

        let mut envelope = EdgeBatchEnvelope {
            batch_id: uuid::Uuid::new_v4().to_string(),
            venue: state.config.venue_name.clone(),
            generated_at_epoch_ms: now_epoch_ms(),
            nodes: Vec::with_capacity(items.len()),
            errors: Vec::new(),
            transactions: Vec::new(),
            scores: Vec::new(),
        };
        for item in items {
            match item {
                EdgeItem::Telemetry(t) => envelope.nodes.push(t),
                EdgeItem::Error(e) => envelope.errors.push(e),
                EdgeItem::Transaction(t) => envelope.transactions.push(t),
                EdgeItem::Score(s) => envelope.scores.push(s),
            }
        }

        match client
            .post(&url)
            .bearer_auth(&state.config.edge_ingest_token)
            .json(&envelope)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                consecutive_failures = 0;
                tracing::debug!(
                    batch_id = %envelope.batch_id,
                    nodes = envelope.nodes.len(),
                    errors = envelope.errors.len(),
                    transactions = envelope.transactions.len(),
                    scores = envelope.scores.len(),
                    "edge batch accepted"
                );
            }
            Ok(resp) => {
                consecutive_failures += 1;
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(%status, batch_id = %envelope.batch_id, %body, "edge batch rejected");
                requeue_on_reject(state.clone(), envelope).await;
            }
            Err(err) => {
                consecutive_failures += 1;
                tracing::warn!(%err, batch_id = %envelope.batch_id, "edge batch request failed");
                requeue_on_reject(state.clone(), envelope).await;
            }
        }
    }
}

/// Put rejected items back at the head of the buffer so transient edge
/// outages don't lose data (bounded by `buffer_edge_item`).
async fn requeue_on_reject(state: std::sync::Arc<AppState>, envelope: EdgeBatchEnvelope) {
    let mut buf = state.edge_buffer.lock().await;
    let mut items: Vec<EdgeItem> = Vec::new();
    for t in envelope.nodes {
        items.push(EdgeItem::Telemetry(t));
    }
    for e in envelope.errors {
        items.push(EdgeItem::Error(e));
    }
    for t in envelope.transactions {
        items.push(EdgeItem::Transaction(t));
    }
    for s in envelope.scores {
        items.push(EdgeItem::Score(s));
    }
    items.extend(buf.drain(..));
    if items.len() > 4_096 {
        items.drain(..items.len() - 4_096);
    }
    *buf = items;
}