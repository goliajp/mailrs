use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use super::WebState;

#[derive(Deserialize)]
pub(super) struct WsQuery {
    pub token: Option<String>,
}

pub(super) async fn ws_events(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<Arc<WebState>>,
) -> Result<impl IntoResponse, StatusCode> {
    // authentication is mandatory
    let token = query.token.as_deref().ok_or(StatusCode::UNAUTHORIZED)?;
    match state.sessions.get(token) {
        Some(session) if session.created_at.elapsed() < super::SESSION_TTL => {}
        _ => return Err(StatusCode::UNAUTHORIZED),
    }
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state)))
}

async fn handle_ws(socket: WebSocket, state: Arc<WebState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_bus.subscribe();

    // forward events to websocket
    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // drain incoming messages (keep-alive pongs handled by axum)
    let recv_task = tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
