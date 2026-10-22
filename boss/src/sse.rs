//! Multiplexed SSE hub: merges per-channel broadcast buses into one
//! `text/event-stream` response, tagging every frame with its channel name.

use std::convert::Infallible;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::extract::{Query, State};
use futures::stream::{Stream, select_all};
use tokio::sync::broadcast;

use crate::models::now_epoch_ms;
use crate::state::{SharedState, SseChannel};

#[derive(Debug, Default, serde::Deserialize)]
pub struct StreamParams {
    /// Comma-separated channel list, e.g. `?channels=venue,alerts`.
    #[serde(default)]
    pub channels: Option<String>,
}

/// Build one infinite sub-stream per requested channel, then merge them.
/// Every event is emitted as `event: <channel>` plus a monotonically
/// increasing `id:` so clients can resume after reconnects.
pub async fn telemetry_sse(
    State(state): State<SharedState>,
    Query(params): Query<StreamParams>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let requested: Vec<SseChannel> = params
        .channels
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .filter_map(SseChannel::parse)
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| SseChannel::ALL.to_vec());

    let mut substreams: Vec<std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>> =
        Vec::with_capacity(requested.len());

    for channel in requested {
        let rx: broadcast::Receiver<String> = state.buses[&channel].subscribe();
        let name = channel.as_str();
        substreams.push(Box::pin(channel_stream(name, rx)));
    }

    Sse::new(select_all(substreams)).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
}

fn channel_stream(
    name: &'static str,
    mut rx: broadcast::Receiver<String>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let mut counter: u64 = 0;
        loop {
            match rx.recv().await {
                Ok(data) => {
                    counter += 1;
                    yield Ok(Event::default()
                        .event(name)
                        .id(counter.to_string())
                        .data(data));
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    // Client fell behind; inform it and keep streaming live data.
                    let notice = serde_json::json!({
                        "channel": name,
                        "skipped": skipped,
                        "at_epoch_ms": now_epoch_ms(),
                    });
                    if let Ok(json) = serde_json::to_string(&notice) {
                        yield Ok(Event::default()
                            .event("lag")
                            .data(json));
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}