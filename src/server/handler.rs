use crate::config::get_launcher_settings;
use crate::conversion::request_converter::anthropic_to_openai_request;
use crate::conversion::response_converter::{
    normalize_chat_completions_url, normalize_messages_url, openai_to_anthropic_response,
    prepare_proxy_body,
};
use crate::crypto::unprotect_secret;
use crate::server::streaming::start_sse_stream_conversion;
use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

static ASYNC_CLIENT: OnceLock<Client> = OnceLock::new();

fn async_client() -> &'static Client {
    ASYNC_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(crate::constants::HTTP_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

pub async fn handle_root() -> impl IntoResponse {
    "FreeClaudeLauncher API proxy is running"
}

pub async fn handle_launcher_show() -> impl IntoResponse {
    super::LAUNCHER_SHOW_REQUESTED.store(true, std::sync::atomic::Ordering::Release);

    #[cfg(target_os = "windows")]
    {
        let tid = super::TRAY_THREAD_ID.load(std::sync::atomic::Ordering::Acquire);
        if tid != 0 {
            unsafe {
                winapi::um::winuser::PostThreadMessageW(
                    tid,
                    winapi::um::winuser::WM_USER + 1,
                    0,
                    0,
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(thread) = super::TRAY_THREAD.get() {
            thread.unpark();
        }
    }

    "ok"
}

pub async fn handle_proxy(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    // Debug: log all request headers to understand what Claude Desktop sends
    for (name, value) in &headers {
        tracing::debug!("[req header] {}: {:?}", name, value);
    }
    if let Some(origin) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        tracing::info!("[req header] Origin: {}", origin);
    } // 1. Validate authorization
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok());
    let x_api_key_header = headers.get("x-api-key").and_then(|h| h.to_str().ok());
    let is_authorized =
        x_api_key_header.is_some() || super::is_valid_proxy_authorization(auth_header);
    if !is_authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized" })),
        )
            .into_response();
    }
    // 2. Load settings
    let Some(settings) = get_launcher_settings() else {
        tracing::error!("<- 錯誤: Launcher 尚未配置");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Launcher has not been configured yet." })),
        )
            .into_response();
    };

    let body_str = String::from_utf8_lossy(&body);
    let is_openai_format = !settings.real_base_url.contains("api.anthropic.com")
        && !settings.real_base_url.contains("openrouter.ai");

    let req_model = match serde_json::from_str::<Value>(&body_str) {
        Ok(v) => v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        Err(_) => "unknown".to_string(),
    };

    // 3. Connection Probe interception (empty messages + tiny max_tokens)
    //
    // 真正的「連線健康檢查」訊息長相：`messages` 是空陣列（人類對話不可能丟空訊息）。
    // 不能單看 `max_tokens <= 5`，因為 Anthropic 官方認可 `max_tokens=1` 多選題
    // 預填用法；如果只看 max_tokens，會把使用者的正式請求吃掉、攔下、改回假的「.」，
    // 造成切換模型時「訊息送出但什麼都沒發生」的視覺感（即重複無效呼叫）。
    //
    // 因此必須三條件同時成立才視為 probe：
    //   1. `messages` 解析為空 array
    //   2. `max_tokens` 很小（避免誤吃沒有 body 但有 tools 的請求）
    //   3. body 極短（背景 ping 不會帶大 system 與 tools schemas）
    let probe_decision = match serde_json::from_str::<Value>(&body_str) {
        Ok(v) => {
            let max_tokens = v.get("max_tokens").and_then(Value::as_u64).unwrap_or(9999);
            let stream = v.get("stream").and_then(Value::as_bool).unwrap_or(false);
            let messages_empty = v
                .get("messages")
                .and_then(Value::as_array)
                .map(|arr| arr.is_empty())
                .unwrap_or(false);
            let has_user_content = v
                .get("messages")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter().any(|m| {
                        m.get("role").and_then(Value::as_str) == Some("user")
                            && m.get("content").map(|c| !c.is_null()).unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            Some((max_tokens, stream, messages_empty, has_user_content))
        }
        Err(_) => None,
    };

    if let Some((max_tokens, is_probe_stream, messages_empty, has_user_content)) = probe_decision {
        // 只要帶有任何 user 訊息就不是 probe，避免誤吃真實請求。
        if !has_user_content && messages_empty && max_tokens <= 5 && body_str.len() < 400 {
            tracing::info!(
                "-> [探測攔截] 繞過 Claude 檢查，自動回傳成功回應 (model: {})",
                req_model
            );
            if is_probe_stream {
                let msg_id = format!(
                    "msg_probe_{}",
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_millis()
                );

                // Construct events
                let events = vec![
                format!(
                    "event: message_start\ndata: {}\n\n",
                    json!({
                        "type": "message_start",
                        "message": {
                            "id": msg_id,
                            "type": "message",
                            "role": "assistant",
                            "content": [],
                            "model": req_model,
                            "stop_reason": null,
                            "usage": { "input_tokens": 1, "output_tokens": 0 }
                        }
                    })
                ),
                format!(
                    "event: content_block_start\ndata: {}\n\n",
                    json!({
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": { "type": "text", "text": "" }
                    })
                ),
                format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": { "type": "text_delta", "text": "." }
                    })
                ),
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n".to_string(),
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
            ];

                let (tx, rx) =
                    tokio::sync::mpsc::channel::<Result<Bytes, std::convert::Infallible>>(10);
                tokio::spawn(async move {
                    for event in events {
                        let _ = tx.send(Ok(Bytes::from(event))).await;
                    }
                });

                let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
                let body = axum::body::Body::from_stream(stream);

                return axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/event-stream; charset=utf-8")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .body(body)
                    .unwrap();
            } else {
                let msg_id = format!(
                    "msg_probe_{}",
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_millis()
                );
                let probe_res = json!({
                    "id": msg_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "."
                        }
                    ],
                    "model": req_model,
                    "stop_reason": "end_turn",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                });
                return (StatusCode::OK, Json(probe_res)).into_response();
            }
        } // 內層 probe 條件 (messages 空 + 沒 user 內容) 結束
    }

    // 4. Request format conversion
    let (proxy_body, is_stream) = if is_openai_format {
        match anthropic_to_openai_request(&body_str, &settings) {
            Ok(res) => res,
            Err(error) => {
                tracing::error!("<- 錯誤: 轉換請求格式失敗: {:?}", error);
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
            }
        }
    } else {
        (prepare_proxy_body(&body_str, &settings), false)
    };

    let target_url = if is_openai_format {
        match normalize_chat_completions_url(&settings.real_base_url) {
            Ok(url) => url,
            Err(error) => {
                tracing::error!("<- 錯誤: 無效的 Gateway URL: {:?}", error);
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
            }
        }
    } else {
        match normalize_messages_url(&settings.real_base_url) {
            Ok(url) => url,
            Err(error) => {
                tracing::error!("<- 錯誤: 無效的 Gateway URL: {:?}", error);
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
            }
        }
    };

    let api_key = match unprotect_secret(&settings.real_api_key) {
        Ok(key) => key,
        Err(error) => {
            tracing::error!("<- 錯誤: 解密 API key 失敗: {:?}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    tracing::info!("-> 轉發請求至: {}", target_url);
    tracing::debug!(
        "-> 轉發 Body[{}]: first 4 hex bytes={:02x?}",
        proxy_body.len(),
        proxy_body
            .as_bytes()
            .iter()
            .take(4)
            .copied()
            .collect::<Vec<u8>>()
    );

    // 5. Build Upstream request
    let mut upstream_req = async_client().post(&target_url).body(proxy_body);
    for (name, value) in &headers {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "content-type" | "accept" | "user-agent" | "accept-encoding" | "connection"
        ) || lower.starts_with("anthropic-")
        {
            upstream_req = upstream_req.header(name.clone(), value.clone());
        }
    }

    if !api_key.is_empty() {
        upstream_req = if settings.real_auth_scheme == "x-api-key" {
            upstream_req.header("x-api-key", api_key)
        } else {
            upstream_req.bearer_auth(api_key)
        };
    }

    // 6. Send request
    match upstream_req.send().await {
        Ok(response) => {
            let status = response.status();
            let status_u16 = status.as_u16();

            if is_openai_format && is_stream {
                tracing::info!("<- 上游回應狀態碼(流式): {}", status_u16);
                if !status.is_success() {
                    let text = response.text().await.unwrap_or_default();
                    tracing::error!("<- 上游流式錯誤狀態碼: {}, Body: {}", status_u16, text);
                    return (status, Json(json!({ "error": text }))).into_response();
                }

                let rx = start_sse_stream_conversion(response, req_model);
                let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
                let body = axum::body::Body::from_stream(stream);

                axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/event-stream; charset=utf-8")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .body(body)
                    .unwrap()
            } else {
                tracing::info!("<- 上游回應狀態碼: {}", status_u16);
                if is_openai_format && status.is_success() {
                    let response_text = match response.text().await {
                        Ok(t) => t,
                        Err(e) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(json!({ "error": e.to_string() })),
                            )
                                .into_response();
                        }
                    };
                    match openai_to_anthropic_response(&response_text, &req_model) {
                        Ok(anthropic_res) => (StatusCode::OK, Json(anthropic_res)).into_response(),
                        Err(e) => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": e.to_string() })),
                        )
                            .into_response(),
                    }
                } else if is_openai_format {
                    let text = response.text().await.unwrap_or_default();
                    tracing::error!("<- 上游錯誤 Body: {}", text);
                    let err_json: Value =
                        serde_json::from_str(&text).unwrap_or(json!({ "error": text }));
                    (status, Json(err_json)).into_response()
                } else {
                    // Passthrough raw Anthropic response headers and body
                    let mut res_builder = axum::response::Response::builder().status(status);
                    for (name, value) in response.headers() {
                        res_builder = res_builder.header(name.clone(), value.clone());
                    }
                    let body = axum::body::Body::from_stream(response.bytes_stream());
                    res_builder.body(body).unwrap()
                }
            }
        }
        Err(error) => {
            tracing::error!("<- 轉發錯誤: {:?}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Proxy forwarding error: {error}") })),
            )
                .into_response()
        }
    }
}
