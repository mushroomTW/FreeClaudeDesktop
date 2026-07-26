use free_claude_core::AdminRpcRequest;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// 執行 `companion_daemon` 對應的處理流程。
pub async fn companion_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::var("FREECLAUDE_PROXY_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);
    let addr = format!("ws://127.0.0.1:{}/companion", port);
    let mut backoff = Duration::from_millis(100);

    loop {
        tracing::info!("Companion daemon connecting to {}", addr);
        match connect_async(&addr).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Companion daemon connected successfully.");
                backoff = Duration::from_millis(100);
                let (ws_sink, mut ws_stream) = ws_stream.split();

                let ws_sink = std::sync::Arc::new(tokio::sync::Mutex::new(ws_sink));

                while let Some(msg) = ws_stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            let ws_sink_clone = ws_sink.clone();
                            tokio::spawn(async move {
                                let response = match handle_message(&text).await {
                                    Ok(res) => res,
                                    Err(err) => json!({
                                        "error": err
                                    }),
                                };
                                let mut sink = ws_sink_clone.lock().await;
                                let _ = sink.send(Message::Text(response.to_string().into())).await;
                            });
                        }
                        Ok(Message::Ping(payload)) => {
                            let mut sink = ws_sink.lock().await;
                            let _ = sink.send(Message::Pong(payload)).await;
                        }
                        Ok(Message::Close(_)) => {
                            break;
                        }
                        Err(e) => {
                            tracing::error!("WebSocket error: {:?}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to connect to companion WS: {:?}. Retrying...", e);
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, Duration::from_secs(5));
            }
        }
    }
}

/// 處理 `handle_message` 對應的請求。
async fn handle_message(text: &str) -> Result<Value, String> {
    let req_val: Value = serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
    let request_id = req_val
        .get("requestId")
        .and_then(|v| v.as_str())
        .ok_or("Missing requestId")?;

    match handle_rpc_logic(&req_val).await {
        Ok(result) => Ok(json!({
            "requestId": request_id,
            "result": result
        })),
        Err(err) => Ok(json!({
            "requestId": request_id,
            "error": err
        })),
    }
}

/// 處理 `handle_rpc_logic` 對應的請求。
async fn handle_rpc_logic(req_val: &Value) -> Result<Value, String> {
    let settings = free_claude_core::get_launcher_settings().ok_or("Launcher not configured")?;
    let rpc_req: AdminRpcRequest =
        serde_json::from_value(req_val.clone()).map_err(|e| e.to_string())?;
    let result = match rpc_req {
        AdminRpcRequest::GetStatus => {
            json!({
                "proxy": { "status": "ok", "port": settings.active_port },
                "settings": free_claude_core::to_public_config(&settings),
            })
        }
        AdminRpcRequest::DetectClaude => {
            let path = free_claude_core::detect_claude_path();
            json!({ "path": path.map(|p| p.display().to_string()) })
        }
        AdminRpcRequest::LaunchClaude => {
            let custom_path = settings.custom_claude_path.as_deref().map(Path::new);
            let path = free_claude_core::launch_claude(custom_path).map_err(|e| e.to_string())?;
            json!({ "path": path.display().to_string() })
        }
        AdminRpcRequest::RestoreSettings => {
            free_claude_core::restore_official_config().map_err(|e| e.to_string())?;
            json!({ "restored": true })
        }
        AdminRpcRequest::SyncFromOfficial => {
            free_claude_core::resync_from_official().map_err(|e| e.to_string())?;
            json!({ "synced": true })
        }
        AdminRpcRequest::ResetMirrorProfile => {
            free_claude_core::reset_mirror_profile().map_err(|e| e.to_string())?;
            json!({ "reset": true })
        }
        AdminRpcRequest::FetchModels => {
            return Err("FetchModels should be handled by proxy directly".to_string());
        }
        AdminRpcRequest::ApplySettings {
            base_url,
            auth_scheme,
            api_key,
        } => {
            let mut settings = free_claude_core::get_launcher_settings().unwrap_or_default();
            settings.real_base_url = base_url;
            settings.real_auth_scheme = auth_scheme;
            if let Some(key) = api_key
                && !key.is_empty()
            {
                settings.real_api_key =
                    free_claude_core::protect_secret(&key).map_err(|e| e.to_string())?;
            }
            free_claude_core::save_launcher_settings(&settings).map_err(|e| e.to_string())?;
            free_claude_core::to_public_config(&settings)
        }
    };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    /// 驗證 `test_handle_message_success_or_expected_failure` 的行為符合預期。
    async fn test_handle_message_success_or_expected_failure() {
        let input = json!({
            "requestId": "test-req-123",
            "token": "some-token",
            "method": "GetStatus"
        })
        .to_string();

        let res = handle_message(&input).await.unwrap();
        assert_eq!(res["requestId"], "test-req-123");
        // 無論後續邏輯是否因為 token 不對或 Launcher 沒配置而失敗，
        // 都必須有包含 requestId 的回應，要麼是 error，要麼是 result。
        assert!(res.get("error").is_some() || res.get("result").is_some());
    }

    #[tokio::test]
    /// 驗證 `test_handle_message_invalid_json` 的行為符合預期。
    async fn test_handle_message_invalid_json() {
        let input = "invalid json";
        let res = handle_message(input).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    /// 驗證 `test_handle_message_missing_request_id` 的行為符合預期。
    async fn test_handle_message_missing_request_id() {
        let input = json!({
            "token": "some-token",
            "method": "GetStatus"
        })
        .to_string();
        let res = handle_message(&input).await;
        assert!(res.is_err());
    }
}
