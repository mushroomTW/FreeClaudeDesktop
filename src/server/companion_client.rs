use crate::server::handler::AdminRpcRequest;
use crate::{AppError, AppResult};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>;

fn generate_request_id() -> String {
    let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    id.to_string()
}

struct PendingRequest {
    _request_id: String,
    payload: String,
}

pub struct CompanionClient {
    addr: String,
    token: String,
    request_tx: mpsc::UnboundedSender<PendingRequest>,
    pending_requests: PendingRequests,
    _bg_handle: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for CompanionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompanionClient")
            .field("addr", &self.addr)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl CompanionClient {
    /// Create a new CompanionClient and start the background connection loop.
    pub fn new(addr: String, token: String) -> Self {
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));

        let pending_requests_clone = pending_requests.clone();
        let addr_clone = addr.clone();
        let bg_handle = tokio::spawn(async move {
            run_background_loop(addr_clone, request_rx, pending_requests_clone).await;
        });

        Self {
            addr,
            token,
            request_tx,
            pending_requests,
            _bg_handle: bg_handle,
        }
    }

    /// Send a request with a customized timeout duration.
    pub async fn send_request_with_timeout(
        &self,
        request: AdminRpcRequest,
        timeout_duration: Duration,
    ) -> AppResult<Value> {
        let request_id = generate_request_id();
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        let mut body = serde_json::to_value(&request).map_err(AppError::Json)?;
        if let Some(obj) = body.as_object_mut() {
            obj.insert("requestId".to_string(), Value::String(request_id.clone()));
            obj.insert("token".to_string(), Value::String(self.token.clone()));
        } else {
            let mut pending = self.pending_requests.lock().await;
            pending.remove(&request_id);
            return Err(AppError::Launcher(
                "Request is not a JSON object".to_string(),
            ));
        }

        if tracing::enabled!(tracing::Level::DEBUG) {
            let mut log_val = body.clone();
            if let Some(obj) = log_val.as_object_mut() {
                if obj.contains_key("token") {
                    obj.insert("token".to_string(), Value::String("[REDACTED]".to_string()));
                }
            }
            tracing::debug!("Sending companion request: {}", log_val);
        }

        let payload = body.to_string();
        if self
            .request_tx
            .send(PendingRequest {
                _request_id: request_id.clone(),
                payload,
            })
            .is_err()
        {
            let mut pending = self.pending_requests.lock().await;
            pending.remove(&request_id);
            return Err(AppError::Launcher(
                "Background task is not running".to_string(),
            ));
        }

        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(Ok(val))) => Ok(val),
            Ok(Ok(Err(err_msg))) => Err(AppError::Launcher(err_msg)),
            Ok(Err(_)) => Err(AppError::Launcher(
                "Connection lost before response was received".to_string(),
            )),
            Err(_) => {
                let mut pending = self.pending_requests.lock().await;
                pending.remove(&request_id);
                Err(AppError::Launcher("RPC request timed out".to_string()))
            }
        }
    }

    /// Send a request with the default 5-second timeout.
    pub async fn send_request(&self, request: AdminRpcRequest) -> AppResult<Value> {
        self.send_request_with_timeout(request, Duration::from_secs(5))
            .await
    }

    /// RPC call: GetStatus
    pub async fn get_status(&self) -> AppResult<Value> {
        self.send_request(AdminRpcRequest::GetStatus).await
    }

    /// RPC call: DetectClaude
    pub async fn detect_claude(&self) -> AppResult<Value> {
        self.send_request(AdminRpcRequest::DetectClaude).await
    }

    /// RPC call: ApplySettings
    pub async fn apply_settings(
        &self,
        base_url: String,
        auth_scheme: String,
        api_key: Option<String>,
    ) -> AppResult<Value> {
        self.send_request(AdminRpcRequest::ApplySettings {
            base_url,
            auth_scheme,
            api_key,
        })
        .await
    }

    /// RPC call: LaunchClaude
    pub async fn launch_claude(&self) -> AppResult<Value> {
        self.send_request(AdminRpcRequest::LaunchClaude).await
    }

    /// RPC call: RestoreSettings
    pub async fn restore_settings(&self) -> AppResult<Value> {
        self.send_request(AdminRpcRequest::RestoreSettings).await
    }
}

async fn handle_incoming_text(
    text: &str,
    pending_requests: &PendingRequests,
) -> Result<(), String> {
    let response: Value = serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
    let request_id = response
        .get("requestId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing or invalid requestId field".to_string())?;

    let mut pending = pending_requests.lock().await;
    if let Some(tx) = pending.remove(request_id) {
        if let Some(err) = response.get("error") {
            let err_str = err.as_str().unwrap_or("unknown error").to_string();
            let _ = tx.send(Err(err_str));
        } else if let Some(result) = response.get("result") {
            let _ = tx.send(Ok(result.clone()));
        } else {
            let _ = tx.send(Err(
                "Response missing both result and error fields".to_string()
            ));
        }
    }
    Ok(())
}

async fn fail_all_pending_requests(pending_requests: &PendingRequests, reason: &str) {
    let mut pending = pending_requests.lock().await;
    for (_id, tx) in pending.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
}

async fn run_background_loop(
    addr: String,
    mut request_rx: mpsc::UnboundedReceiver<PendingRequest>,
    pending_requests: PendingRequests,
) {
    let mut backoff = Duration::from_millis(100);

    loop {
        tracing::info!("Connecting to companion WS at {}", addr);
        match connect_async(&addr).await {
            Ok((ws_stream, _response)) => {
                tracing::info!("Successfully connected to companion WS");
                backoff = Duration::from_millis(100);

                let (mut ws_sink, mut ws_stream) = ws_stream.split();

                loop {
                    tokio::select! {
                        ws_msg = ws_stream.next() => {
                            match ws_msg {
                                Some(Ok(Message::Text(text))) => {
                                    if let Err(e) = handle_incoming_text(&text, &pending_requests).await {
                                        tracing::warn!("Failed to handle incoming WS message: {:?}", e);
                                    }
                                }
                                Some(Ok(Message::Binary(_))) => {}
                                Some(Ok(Message::Ping(payload))) => {
                                    if let Err(e) = ws_sink.send(Message::Pong(payload)).await {
                                        tracing::error!("Failed to send Pong: {:?}", e);
                                        break;
                                    }
                                }
                                Some(Ok(Message::Pong(_))) => {}
                                Some(Ok(Message::Close(_))) => {
                                    tracing::info!("WebSocket connection closed by server");
                                    break;
                                }
                                Some(Ok(_)) => {}
                                Some(Err(e)) => {
                                    tracing::error!("WebSocket error: {:?}", e);
                                    break;
                                }
                                None => {
                                    tracing::info!("WebSocket stream EOF reached");
                                    break;
                                }
                            }
                        }
                        req = request_rx.recv() => {
                            match req {
                                Some(pending_req) => {
                                    if let Err(e) = ws_sink.send(Message::Text(pending_req.payload.into())).await {
                                        tracing::error!("Failed to send request over WS: {:?}", e);
                                        break;
                                    }
                                }
                                None => {
                                    tracing::info!("Request channel closed, shutting down background task");
                                    return;
                                }
                            }
                        }
                    }
                }

                fail_all_pending_requests(&pending_requests, "Connection lost").await;
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to connect to companion WS: {:?}. Retrying in {:?}",
                    e,
                    backoff
                );
                fail_all_pending_requests(&pending_requests, "Connection lost").await;

                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, Duration::from_secs(5));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn start_mock_ws_server(
        port_tx: oneshot::Sender<u16>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        port_tx.send(port).unwrap();

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    break;
                }
                conn = listener.accept() => {
                    let Ok((stream, _)) = conn else { continue; };
                    tokio::spawn(async move {
                        let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                            Ok(ws) => ws,
                            Err(_) => return,
                        };
                        let (mut write, mut read) = ws_stream.split();
                        while let Some(msg) = read.next().await {
                            let Ok(msg) = msg else { break; };
                            if let Message::Text(text) = msg {
                                let req: Value = serde_json::from_str(&text).unwrap();
                                let request_id = req.get("requestId").and_then(|v| v.as_str()).unwrap().to_string();
                                let token = req.get("token").and_then(|v| v.as_str()).unwrap();
                                let method = req.get("method").and_then(|v| v.as_str()).unwrap();

                                if token != "secret_token" {
                                    let res = json!({
                                        "requestId": request_id,
                                        "error": "unauthorized"
                                    });
                                    let _ = write.send(Message::Text(res.to_string().into())).await;
                                    continue;
                                }

                                match method {
                                    "GetStatus" => {
                                        let res = json!({
                                            "requestId": request_id,
                                            "result": {
                                                "proxy": { "status": "ok", "port": 3000 },
                                                "settings": { "realBaseUrl": "http://127.0.0.1" }
                                            }
                                        });
                                        let _ = write.send(Message::Text(res.to_string().into())).await;
                                    }
                                    "DetectClaude" => {
                                        let res = json!({
                                            "requestId": request_id,
                                            "result": { "path": "mock_path" }
                                        });
                                        let _ = write.send(Message::Text(res.to_string().into())).await;
                                    }
                                    "ApplySettings" => {
                                        let base_url = req.get("baseUrl").and_then(|v| v.as_str()).unwrap();
                                        if base_url == "http://timeout" {
                                            continue;
                                        }
                                        if base_url == "http://disconnect" {
                                            break;
                                        }
                                        let res = json!({
                                            "requestId": request_id,
                                            "result": { "realBaseUrl": base_url }
                                        });
                                        let _ = write.send(Message::Text(res.to_string().into())).await;
                                    }
                                    "LaunchClaude" => {
                                        let res = json!({
                                            "requestId": request_id,
                                            "result": { "path": "mock_launch_path" }
                                        });
                                        let _ = write.send(Message::Text(res.to_string().into())).await;
                                    }
                                    "RestoreSettings" => {
                                        let res = json!({
                                            "requestId": request_id,
                                            "result": { "restored": true }
                                        });
                                        let _ = write.send(Message::Text(res.to_string().into())).await;
                                    }
                                    _ => {
                                        let res = json!({
                                            "requestId": request_id,
                                            "error": "unknown_method"
                                        });
                                        let _ = write.send(Message::Text(res.to_string().into())).await;
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }
    }

    #[tokio::test]
    async fn test_companion_client_rpc_calls() {
        let (port_tx, port_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        tokio::spawn(start_mock_ws_server(port_tx, shutdown_rx));
        let port = port_rx.await.unwrap();

        let client = CompanionClient::new(
            format!("ws://127.0.0.1:{}", port),
            "secret_token".to_string(),
        );

        // Test GetStatus
        let status = client.get_status().await.unwrap();
        assert_eq!(status["proxy"]["status"], "ok");

        // Test DetectClaude
        let path_info = client.detect_claude().await.unwrap();
        assert_eq!(path_info["path"], "mock_path");

        // Test ApplySettings
        let settings = client
            .apply_settings("http://new-gateway".to_string(), "bearer".to_string(), None)
            .await
            .unwrap();
        assert_eq!(settings["realBaseUrl"], "http://new-gateway");

        // Test LaunchClaude
        let launch = client.launch_claude().await.unwrap();
        assert_eq!(launch["path"], "mock_launch_path");

        // Test RestoreSettings
        let restore = client.restore_settings().await.unwrap();
        assert_eq!(restore["restored"], true);

        // Cleanup
        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn test_companion_client_unauthorized() {
        let (port_tx, port_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        tokio::spawn(start_mock_ws_server(port_tx, shutdown_rx));
        let port = port_rx.await.unwrap();

        let client = CompanionClient::new(
            format!("ws://127.0.0.1:{}", port),
            "wrong_token".to_string(),
        );

        let res = client.get_status().await;
        assert!(res.is_err());
        let err = res.err().unwrap().to_string();
        assert!(err.contains("unauthorized"));

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn test_companion_client_timeout() {
        let (port_tx, port_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        tokio::spawn(start_mock_ws_server(port_tx, shutdown_rx));
        let port = port_rx.await.unwrap();

        let client = CompanionClient::new(
            format!("ws://127.0.0.1:{}", port),
            "secret_token".to_string(),
        );

        let res = client
            .send_request_with_timeout(
                AdminRpcRequest::ApplySettings {
                    base_url: "http://timeout".to_string(),
                    auth_scheme: "bearer".to_string(),
                    api_key: None,
                },
                Duration::from_millis(50),
            )
            .await;
        assert!(res.is_err());
        let err = res.err().unwrap().to_string();
        assert!(
            err.contains("timed out") || err.contains("Timeout"),
            "Expected timeout error, got: {}",
            err
        );

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn test_companion_client_reconnect() {
        let (port_tx, port_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        tokio::spawn(start_mock_ws_server(port_tx, shutdown_rx));
        let port = port_rx.await.unwrap();

        let client = CompanionClient::new(
            format!("ws://127.0.0.1:{}", port),
            "secret_token".to_string(),
        );

        // 1. Check status first (connection works)
        let status = client.get_status().await.unwrap();
        assert_eq!(status["proxy"]["status"], "ok");

        // 2. Trigger disconnect on server side
        let _res = client
            .apply_settings("http://disconnect".to_string(), "bearer".to_string(), None)
            .await;

        // Wait a short time for the client background loop to detect disconnect and start reconnecting
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 3. Check status again, it should automatically reconnect and succeed!
        let mut success = false;
        for _ in 0..10 {
            if let Ok(status) = client.get_status().await {
                assert_eq!(status["proxy"]["status"], "ok");
                success = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert!(
            success,
            "Client failed to reconnect and handle subsequent request"
        );

        let _ = shutdown_tx.send(());
    }
}
