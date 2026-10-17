use free_claude_core::DashboardRpcRequest;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{
    Message,
    client::IntoClientRequest,
    http::header::{AUTHORIZATION, HeaderValue},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

/// 依環境變數解析要連線的 Proxy companion 位址。
fn companion_address() -> String {
    let port = std::env::var("FREECLAUDE_PROXY_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);
    format!("ws://127.0.0.1:{}/companion", port)
}

/// 讀取 Proxy 驗證 token；尚未設定時回傳 `None`，由呼叫端退避重試。
fn companion_token() -> Option<String> {
    match free_claude_core::get_launcher_settings() {
        Some(settings) if !settings.gateway.proxy_auth_token.trim().is_empty() => {
            Some(settings.gateway.proxy_auth_token)
        }
        _ => None,
    }
}

/// 將 token 轉為 `Authorization: Bearer` 標頭值。
fn bearer_header(token: &str) -> Result<HeaderValue, Box<dyn std::error::Error>> {
    Ok(format!("Bearer {token}").parse::<HeaderValue>()?)
}

/// 等待退避時間後，回傳下一次使用的退避（上限 5 秒）。
async fn sleep_backoff(backoff: Duration) -> Duration {
    tokio::time::sleep(backoff).await;
    (backoff * 2).min(Duration::from_secs(5))
}

/// 已連線的 WebSocket 訊息種類。
enum CompanionMessage {
    /// 文字訊息，需要非同步處理後回覆。
    Reply(String),
    /// Ping，需要以 Pong 回應相同內容。
    Ping(Vec<u8>),
    /// 連線應結束（收到 Close 或發生錯誤）。
    Stop,
    /// 其他訊息（Pong、Binary 等）直接忽略。
    Ignore,
}

/// 將收到的 WebSocket 訊息分類；錯誤一律記錄並結束連線。
fn classify_message(
    msg: Result<Message, tokio_tungstenite::tungstenite::Error>,
) -> CompanionMessage {
    match msg {
        Ok(Message::Text(text)) => CompanionMessage::Reply(text.to_string()),
        Ok(Message::Ping(payload)) => CompanionMessage::Ping(payload.to_vec()),
        Ok(Message::Close(_)) => CompanionMessage::Stop,
        Err(e) => {
            tracing::error!("WebSocket error: {:?}", e);
            CompanionMessage::Stop
        }
        _ => CompanionMessage::Ignore,
    }
}

/// 執行已連線的訊息迴圈，直到連線結束或發生錯誤。
async fn run_companion_session(ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>) {
    let (ws_sink, mut ws_stream) = ws_stream.split();
    let ws_sink = std::sync::Arc::new(tokio::sync::Mutex::new(ws_sink));

    while let Some(msg) = ws_stream.next().await {
        match classify_message(msg) {
            CompanionMessage::Reply(text) => {
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
            CompanionMessage::Ping(payload) => {
                let mut sink = ws_sink.lock().await;
                let _ = sink.send(Message::Pong(payload.into())).await;
            }
            CompanionMessage::Stop => break,
            CompanionMessage::Ignore => {}
        }
    }
}

/// 執行 `companion_daemon` 對應的處理流程。
pub async fn companion_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let addr = companion_address();
    let mut backoff = Duration::from_millis(100);

    loop {
        tracing::info!("Companion daemon connecting to {}", addr);
        let Some(token) = companion_token() else {
            tracing::error!("Companion daemon is missing the Proxy auth token");
            backoff = sleep_backoff(backoff).await;
            continue;
        };
        let mut request = match addr.clone().into_client_request() {
            Ok(request) => request,
            Err(error) => return Err(error.into()),
        };
        let authorization = match bearer_header(&token) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        request.headers_mut().insert(AUTHORIZATION, authorization);

        match connect_async(request).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Companion daemon connected successfully.");
                backoff = Duration::from_millis(100);
                run_companion_session(ws_stream).await;
            }
            Err(e) => {
                tracing::warn!("Failed to connect to companion WS: {:?}. Retrying...", e);
                backoff = sleep_backoff(backoff).await;
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
    let rpc_req: DashboardRpcRequest =
        serde_json::from_value(req_val.clone()).map_err(|e| e.to_string())?;
    let result = match rpc_req {
        DashboardRpcRequest::GetStatus => {
            json!({
                "proxy": { "status": "ok", "port": settings.desktop.active_port },
                "settings": free_claude_core::to_public_config(&settings),
            })
        }
        DashboardRpcRequest::DetectClaude => {
            let path = free_claude_core::detect_claude_path();
            json!({ "path": path.map(|p| p.display().to_string()) })
        }
        DashboardRpcRequest::LaunchClaude => {
            let custom_path = settings
                .desktop
                .custom_claude_path
                .as_deref()
                .map(Path::new);
            let path = free_claude_core::launch_claude(custom_path).map_err(|e| e.to_string())?;
            json!({ "path": path.display().to_string() })
        }
        DashboardRpcRequest::RestoreSettings => {
            free_claude_core::restore_official_config().map_err(|e| e.to_string())?;
            json!({ "restored": true })
        }
        DashboardRpcRequest::SyncFromOfficial => {
            free_claude_core::resync_from_official().map_err(|e| e.to_string())?;
            json!({ "synced": true })
        }
        DashboardRpcRequest::ResetMirrorProfile => {
            free_claude_core::reset_mirror_profile().map_err(|e| e.to_string())?;
            json!({ "reset": true })
        }
        DashboardRpcRequest::FetchModels => {
            return Err("FetchModels should be handled by proxy directly".to_string());
        }
        DashboardRpcRequest::ApplySettings {
            base_url,
            auth_scheme,
            api_key,
        } => {
            let mut settings = free_claude_core::get_launcher_settings().unwrap_or_default();
            settings.gateway.real_base_url = base_url;
            settings.gateway.real_auth_scheme = auth_scheme;
            if let Some(key) = api_key
                && !key.is_empty()
            {
                settings.gateway.real_api_key =
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

    /// 需要讀寫環境變數的測試必須序號執行。
    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn set_env(key: &str, value: &str) {
        // SAFETY: 由 TEST_LOCK 保證同一時間只有一個測試改動環境變數。
        unsafe { std::env::set_var(key, value) };
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        // SAFETY: 由 TEST_LOCK 保證同一時間只有一個測試改動環境變數。
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    #[tokio::test]
    /// 驗證 companion 位址會採用環境變數的埠號，並在無效時回到預設值。
    async fn companion_address_uses_env_port_with_fallback() {
        let _guard = TEST_LOCK.lock().await;
        let previous = std::env::var_os("FREECLAUDE_PROXY_PORT");

        set_env("FREECLAUDE_PROXY_PORT", "4123");
        assert_eq!(companion_address(), "ws://127.0.0.1:4123/companion");

        // 非數字與超出 u16 範圍的值都會回到預設埠。
        set_env("FREECLAUDE_PROXY_PORT", "not-a-port");
        assert_eq!(companion_address(), "ws://127.0.0.1:3000/companion");

        set_env("FREECLAUDE_PROXY_PORT", "70000");
        assert_eq!(companion_address(), "ws://127.0.0.1:3000/companion");

        restore_env("FREECLAUDE_PROXY_PORT", previous);
        assert_eq!(companion_address(), "ws://127.0.0.1:3000/companion");
    }

    #[test]
    /// 驗證 token 會被包成 Bearer 標頭，且無法解析的值會回傳錯誤。
    fn bearer_header_formats_and_rejects_invalid_token() {
        let header = bearer_header("secret-token").unwrap();
        assert_eq!(header.to_str().unwrap(), "Bearer secret-token");
        // 換行字元無法放入 HTTP 標頭值。
        assert!(bearer_header("bad\nvalue").is_err());
    }

    #[tokio::test]
    /// 驗證退避時間每次加倍。
    async fn sleep_backoff_doubles_the_delay() {
        let next = sleep_backoff(Duration::from_millis(1)).await;
        assert_eq!(next, Duration::from_millis(2));
    }

    #[test]
    /// 驗證 WebSocket 訊息會被分類成對應的處理方式。
    fn classify_message_maps_every_kind() {
        use tokio_tungstenite::tungstenite::Error;

        assert!(matches!(
            classify_message(Ok(Message::Text("hi".into()))),
            CompanionMessage::Reply(text) if text == "hi"
        ));
        assert!(matches!(
            classify_message(Ok(Message::Ping(vec![1, 2].into()))),
            CompanionMessage::Ping(payload) if payload == vec![1, 2]
        ));
        assert!(matches!(
            classify_message(Ok(Message::Close(None))),
            CompanionMessage::Stop
        ));
        // Pong、Binary 等其他訊息直接忽略。
        assert!(matches!(
            classify_message(Ok(Message::Binary(vec![1].into()))),
            CompanionMessage::Ignore
        ));
        assert!(matches!(
            classify_message(Err(Error::ConnectionClosed)),
            CompanionMessage::Stop
        ));
    }

    #[tokio::test]
    /// 驗證已連線的訊息迴圈會忽略非請求訊息、回覆錯誤結果並在關閉時結束。
    async fn run_companion_session_handles_messages_until_close() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            // Binary 會被忽略，不會產生任何回應。
            ws.send(Message::Binary(vec![1, 2, 3].into()))
                .await
                .unwrap();
            // 無效 JSON 會讓 handle_message 失敗，改回傳 error 物件。
            ws.send(Message::Text("not json".into())).await.unwrap();

            let mut reply = None;
            while let Some(message) = ws.next().await {
                match message.unwrap() {
                    Message::Text(text) => {
                        reply = Some(text.to_string());
                        break;
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            ws.close(None).await.ok();
            reply
        });

        let (client, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/companion"))
            .await
            .unwrap();
        run_companion_session(client).await;

        let reply = server.await.unwrap().expect("應該收到一則回應");
        assert!(reply.contains("Invalid JSON"), "回應應帶錯誤訊息：{reply}");
    }

    #[tokio::test]
    /// 驗證常駐迴圈會帶著 token 連線、跑完工作階段後持續重試，直到被取消。
    async fn companion_daemon_connects_then_retries_until_cancelled() {
        let _guard = TEST_LOCK.lock().await;
        let temp_dir = std::env::temp_dir().join(format!(
            "fc_test_companion_daemon_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let old_local = std::env::var_os("LOCALAPPDATA");
        let old_home = std::env::var_os("HOME");
        let old_xdg_config = std::env::var_os("XDG_CONFIG_HOME");
        let old_port = std::env::var_os("FREECLAUDE_PROXY_PORT");
        set_env("LOCALAPPDATA", &temp_dir.to_string_lossy());
        set_env("HOME", &temp_dir.to_string_lossy());
        set_env("XDG_CONFIG_HOME", &temp_dir.to_string_lossy());

        // 準備一份帶有 Proxy token 的 Launcher 設定。
        let mut settings = free_claude_core::get_launcher_settings().unwrap_or_default();
        settings.gateway.proxy_auth_token = "companion-test-token".to_string();
        free_claude_core::save_launcher_settings(&settings).unwrap();
        assert_eq!(companion_token().as_deref(), Some("companion-test-token"));

        // 以本機最小 WebSocket 伺服器模擬 Proxy companion。
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text("not json".into())).await.unwrap();
            let reply = ws.next().await.map(|message| message.unwrap());
            ws.close(None).await.ok();
            matches!(reply, Some(Message::Text(_)))
        });
        set_env("FREECLAUDE_PROXY_PORT", &port.to_string());

        // 常駐迴圈不會自行結束，只能由逾時取消。
        let outcome = tokio::time::timeout(Duration::from_millis(1500), companion_daemon()).await;
        assert!(outcome.is_err(), "常駐迴圈不應自行結束");
        assert!(server.await.unwrap(), "應收到 RPC 錯誤回應");

        restore_env("FREECLAUDE_PROXY_PORT", old_port);
        restore_env("LOCALAPPDATA", old_local);
        restore_env("HOME", old_home);
        restore_env("XDG_CONFIG_HOME", old_xdg_config);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
