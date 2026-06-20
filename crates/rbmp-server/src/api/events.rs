use std::convert::Infallible;
use axum::{extract::State, response::Sse};
use axum::response::sse::{Event, KeepAlive};
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::warn;
use crate::state::AppState;

/// SSE endpoint — streams RibEvents as JSON server-sent events.
/// Connect at: GET /api/events
/// Event names: route_change, peer_up, peer_down, speaker_up, speaker_down, stats
pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(ev) => {
                let event_name = match &ev.payload {
                    rbmp_rib::event::RibEventPayload::RouteChange(_)  => "route_change",
                    rbmp_rib::event::RibEventPayload::PeerUp { .. }   => "peer_up",
                    rbmp_rib::event::RibEventPayload::PeerDown { .. } => "peer_down",
                    rbmp_rib::event::RibEventPayload::SpeakerUp { .. } => "speaker_up",
                    rbmp_rib::event::RibEventPayload::SpeakerDown { .. } => "speaker_down",
                    rbmp_rib::event::RibEventPayload::Stats { .. }    => "stats",
                    rbmp_rib::event::RibEventPayload::EndOfRib { .. } => "eor",
                };
                match serde_json::to_string(&ev) {
                    Ok(json) => Some(Ok(Event::default().event(event_name).data(json))),
                    Err(e) => {
                        warn!(error = %e, "Failed to serialize RibEvent for SSE");
                        None
                    }
                }
            }
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                warn!(%n, "SSE client lagged — {n} events dropped");
                Some(Ok(Event::default().event("lag").data(format!("{{\"dropped\":{n}}}"))))
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
