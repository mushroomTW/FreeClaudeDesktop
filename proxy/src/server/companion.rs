use axum::{
    Json,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use free_claude_core::AdminRpcRequest;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, oneshot};

type PendingRequests =
    Arc<tokio::sync::Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>;

pub(crate) struct ActiveCompanion {
    pub(crate) tx: mpsc::UnboundedSender<ProxyToCompanionMessage>,
}

pub(crate) struct ProxyToCompanionMessage {
    pub(crate) request_id: String,
    pub(crate) payload: String,
    pub(crate) response_tx: oneshot::Sender<Result<Value, String>>,
}

#[derive(Clone, Default)]
pub struct CompanionState {
    active: Arc<tokio::sync::Mutex<Option<ActiveCompanion>>>,
}

impl CompanionState {
    #[cfg(test)]
    pub(crate) fn active(&self) -> &tokio::sync::Mutex<Option<ActiveCompanion>> {
        &self.active
    }
}

/// 將管理端 RPC 轉送給目前連線的 Companion。
pub(crate) async fn forward_request(state: &CompanionState, request: AdminRpcRequest) -> Response {
    let companion_tx = {
        let active = state.active.lock().await;
        active.as_ref().map(|companion| companion.tx.clone())
    };

    let Some(companion_tx) = companion_tx else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Companion offline" })),
        )
            .into_response();
    };

    let request_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
    let mut payload = serde_json::to_value(&request).unwrap_or(Value::Null);
    if let Some(object) = payload.as_object_mut() {
        object.insert("requestId".to_string(), Value::String(request_id.clone()));
    }

    let (response_tx, response_rx) = oneshot::channel();
    if companion_tx
        .send(ProxyToCompanionMessage {
            request_id,
            payload: payload.to_string(),
            response_tx,
        })
        .is_err()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Companion offline" })),
        )
            .into_response();
    }

    match response_rx.await {
        Ok(Ok(result)) => (StatusCode::OK, Json(json!({ "result": result }))).into_response(),
        Ok(Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Companion disconnected" })),
        )
            .into_response(),
    }
}

/// 升級並接管 Companion WebSocket 連線。
pub async fn handle_companion_websocket(
    State(state): State<CompanionState>,
    websocket: WebSocketUpgrade,
) -> impl IntoResponse {
    websocket.on_upgrade(move |socket| handle_companion_session(state, socket))
}

async fn handle_companion_session(state: CompanionState, socket: WebSocket) {
    let (tx, mut rx) = mpsc::unbounded_channel::<ProxyToCompanionMessage>();
    {
        let mut active = state.active.lock().await;
        *active = Some(ActiveCompanion { tx });
    }

    let pending_requests: PendingRequests = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let (mut ws_sink, mut ws_stream) = socket.split();

    loop {
        tokio::select! {
            message = rx.recv() => {
                match message {
                    Some(message) => {
                        pending_requests
                            .lock()
                            .await
                            .insert(message.request_id.clone(), message.response_tx);
                        if ws_sink.send(Message::Text(message.payload.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            websocket_message = ws_stream.next() => {
                match websocket_message {
                    Some(Ok(Message::Text(text))) => {
                        resolve_pending_response(&pending_requests, &text).await;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if ws_sink.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    {
        let mut active = state.active.lock().await;
        *active = None;
    }
    let mut pending = pending_requests.lock().await;
    for (_, response) in pending.drain() {
        let _ = response.send(Err("Companion disconnected".to_string()));
    }
}

async fn resolve_pending_response(pending_requests: &PendingRequests, text: &str) {
    let Ok(response) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(request_id) = response.get("requestId").and_then(Value::as_str) else {
        return;
    };
    let Some(sender) = pending_requests.lock().await.remove(request_id) else {
        return;
    };

    let result = if let Some(error) = response.get("error").and_then(Value::as_str) {
        Err(error.to_string())
    } else if let Some(error) = response.get("error") {
        Err(error.to_string())
    } else if let Some(result) = response.get("result") {
        Ok(result.clone())
    } else {
        Err("Invalid WS RPC format".to_string())
    };
    let _ = sender.send(result);
}
