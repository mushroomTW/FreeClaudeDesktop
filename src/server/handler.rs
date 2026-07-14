use crate::config_service::{load_runtime_settings, unprotect_runtime_api_key};
use crate::conversion::request_converter::anthropic_to_openai_request;
use crate::conversion::response_converter::{
    normalize_chat_completions_url, normalize_messages_url,
    normalize_models_response_with_overrides, openai_to_anthropic_response, prepare_proxy_body,
    rewrite_stale_model_request,
};
use crate::optimization;
use crate::server::streaming::{ReasoningReplayMode, start_sse_stream_conversion};
use axum::{
    Json,
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use reqwest::Client;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime};

const MAX_UPSTREAM_ERROR_BYTES: usize = 64 * 1024;

fn is_model_gone_or_invalid_error(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("has reached its end of life")
        || lower.contains("no longer available")
        || lower.contains("invalid model name")
        || lower.contains("invalid_model")
        || lower.contains("model_not_found")
        || lower.contains("call /v1/models")
        || lower.contains("model not found")
        || lower.contains("degraded function cannot be invoked")
}

fn may_retry_stale_model(output_started: bool, retry_available: bool, error: &str) -> bool {
    !output_started && retry_available && is_model_gone_or_invalid_error(error)
}

fn request_diagnostic(body: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let max_tokens = value.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
    let stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tools = value.get("tools").is_some();
    let system_type = match value.get("system") {
        Some(Value::String(_)) => "text",
        Some(Value::Array(_)) => "array",
        Some(_) => "other",
        None => "none",
    };
    Some(format!(
        "msgs={messages}, max_tokens={max_tokens}, stream={stream}, tools={tools}, system={system_type}, body_len={}",
        body.len()
    ))
}

fn build_upstream_request(
    client: &Client,
    target_url: &str,
    body: String,
    headers: &HeaderMap,
    api_key: &str,
    auth_scheme: &str,
) -> crate::AppResult<reqwest::RequestBuilder> {
    let mut request = client.post(target_url).body(body);
    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "content-type" | "accept" | "user-agent" | "accept-encoding" | "connection"
        ) || lower.starts_with("anthropic-")
        {
            request = request.header(name.clone(), value.clone());
        }
    }

    crate::server::apply_gateway_auth(request, auth_scheme, api_key, target_url)
}

fn copy_safe_response_headers(source: &reqwest::header::HeaderMap, target: &mut HeaderMap) {
    for (name, value) in source {
        if !matches!(
            name.as_str().to_ascii_lowercase().as_str(),
            "connection" | "content-length" | "transfer-encoding" | "content-encoding"
        ) {
            target.insert(name.clone(), value.clone());
        }
    }
}

async fn read_bounded_error(response: reqwest::Response) -> String {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = MAX_UPSTREAM_ERROR_BYTES.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if bytes.len() == MAX_UPSTREAM_ERROR_BYTES {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn refresh_settings_for_retry(
    settings: &crate::Settings,
    api_key: &str,
) -> Option<crate::Settings> {
    let raw = crate::server::models_endpoint::fetch_models_list_async(
        &settings.real_base_url,
        api_key,
        &settings.real_auth_scheme,
    )
    .await
    .ok()?;
    let normalized = normalize_models_response_with_overrides(
        raw,
        &settings.model_reasoning_overrides,
        &settings.model_1m_overrides,
    )
    .ok()?;
    let mut refreshed = settings.clone();
    refreshed.real_model_routes = normalized.routes;
    refreshed.real_model_reasoning_efforts = normalized.reasoning_effort_routes;
    refreshed.discovered_models = normalized
        .data
        .into_iter()
        .map(|model| model.provider_model_id)
        .collect();
    Some(refreshed)
}

pub async fn handle_root() -> impl IntoResponse {
    "FreeClaudeDesktop API proxy is running"
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

pub async fn handle_healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod healthz_tests {
    use super::*;

    #[tokio::test]
    async fn healthz_returns_ok_status() {
        assert_eq!(handle_healthz().await.0, json!({ "status": "ok" }));
    }
}

pub async fn handle_proxy(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    // Debug: log request headers without leaking local/upstream credentials.
    for (name, value) in &headers {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(lower.as_str(), "authorization" | "x-api-key" | "cookie") {
            tracing::debug!("[req header] {}: <redacted>", name);
        } else {
            tracing::debug!("[req header] {}: {:?}", name, value);
        }
    }
    if let Some(origin) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        tracing::info!("[req header] Origin: {}", origin);
    }

    // 1. Load settings for the configured proxy token.
    let settings = match load_runtime_settings().await {
        Ok(Some(settings)) => settings,
        Ok(None) => {
            tracing::error!("<- 錯誤: Launcher 尚未配置");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Launcher has not been configured yet." })),
            )
                .into_response();
        }
        Err(error) => {
            tracing::error!("<- 錯誤: 讀取 Launcher 設定失敗: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    // 2. Validate authorization
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok());
    let x_api_key_header = headers.get("x-api-key").and_then(|h| h.to_str().ok());
    let is_authorized = super::is_authorized_proxy_request(
        auth_header,
        x_api_key_header,
        &settings.proxy_auth_token,
    );
    if !is_authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized" })),
        )
            .into_response();
    }

    let body_str = String::from_utf8_lossy(&body);

    // 3. Try local optimizations (quota mock, prefix detection, etc.)
    if let Some(response) = optimization::try_optimizations(&body_str, &settings).await {
        return response;
    }

    if let Some(diagnostic) = request_diagnostic(&body_str) {
        tracing::info!("[未攔截請求] {diagnostic}");
    }

    // 4. Determine transport type
    let is_anthropic_native = settings.transport_type == "anthropic_messages"
        || (settings.transport_type.is_empty()
            && settings.real_base_url.contains("api.anthropic.com"));

    let is_openai_format = !is_anthropic_native;

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

    let api_key = match unprotect_runtime_api_key(settings.real_api_key.clone()).await {
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
    tracing::debug!("-> 轉發 Body 長度: {} bytes", proxy_body.len());

    // 5. Build Upstream request
    let upstream_req = match build_upstream_request(
        crate::server::http_client(),
        &target_url,
        proxy_body.clone(),
        &headers,
        &api_key,
        &settings.real_auth_scheme,
    ) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    // 6. Send request
    match upstream_req.send().await {
        Ok(response) => {
            let status = response.status();
            let status_u16 = status.as_u16();

            if is_openai_format && is_stream {
                tracing::info!("<- 上游回應狀態碼(流式): {}", status_u16);
                if !status.is_success() {
                    let text = read_bounded_error(response).await;
                    tracing::error!("<- 上游流式錯誤狀態碼: {}", status_u16);
                    if may_retry_stale_model(false, true, &text) {
                        let retry_settings = refresh_settings_for_retry(&settings, &api_key)
                            .await
                            .unwrap_or_else(|| settings.clone());
                        if let Some(rewrite) =
                            rewrite_stale_model_request(&proxy_body, &retry_settings, &req_model)
                        {
                            tracing::warn!(
                                "[model fallback] model error ({}), retrying {} with {}",
                                status_u16,
                                req_model,
                                rewrite.fallback_model
                            );
                            if let Ok(request) = build_upstream_request(
                                crate::server::http_client(),
                                &target_url,
                                rewrite.updated_body.to_string(),
                                &headers,
                                &api_key,
                                &retry_settings.real_auth_scheme,
                            ) {
                                if let Ok(retry) = request.send().await {
                                    if retry.status().is_success() {
                                        let reasoning_mode =
                                            match settings.reasoning_replay_mode.as_str() {
                                                "inline" => Some(ReasoningReplayMode::Inline),
                                                "separate" => Some(ReasoningReplayMode::Separate),
                                                _ => None,
                                            };
                                        let rx = start_sse_stream_conversion(
                                            retry,
                                            req_model,
                                            reasoning_mode,
                                        );
                                        let stream =
                                            tokio_stream::wrappers::ReceiverStream::new(rx);
                                        return axum::response::Response::builder()
                                            .status(StatusCode::OK)
                                            .header(
                                                "Content-Type",
                                                "text/event-stream; charset=utf-8",
                                            )
                                            .header("Cache-Control", "no-cache")
                                            .header("Connection", "keep-alive")
                                            .body(axum::body::Body::from_stream(stream))
                                            .unwrap();
                                    }
                                }
                            }
                        }
                    }
                    return (status, Json(json!({ "error": text }))).into_response();
                }

                let reasoning_mode = match settings.reasoning_replay_mode.as_str() {
                    "inline" => Some(ReasoningReplayMode::Inline),
                    "separate" => Some(ReasoningReplayMode::Separate),
                    _ => None,
                };
                let rx = start_sse_stream_conversion(response, req_model, reasoning_mode);
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
                    let text = read_bounded_error(response).await;
                    tracing::error!("<- 上游錯誤狀態碼: {}", status_u16);
                    if may_retry_stale_model(false, true, &text) {
                        let retry_settings = refresh_settings_for_retry(&settings, &api_key)
                            .await
                            .unwrap_or_else(|| settings.clone());
                        if let Some(rewrite) =
                            rewrite_stale_model_request(&proxy_body, &retry_settings, &req_model)
                        {
                            tracing::warn!(
                                "[model fallback] model error ({}), retrying {} with {}",
                                status_u16,
                                req_model,
                                rewrite.fallback_model
                            );
                            let retry_req = build_upstream_request(
                                crate::server::http_client(),
                                &target_url,
                                rewrite.updated_body.to_string(),
                                &headers,
                                &api_key,
                                &retry_settings.real_auth_scheme,
                            );

                            if let Ok(retry_req) = retry_req {
                                if let Ok(retry_response) = retry_req.send().await {
                                    if retry_response.status().is_success() {
                                        let retry_text =
                                            retry_response.text().await.unwrap_or_default();
                                        if let Ok(anthropic_res) =
                                            openai_to_anthropic_response(&retry_text, &req_model)
                                        {
                                            return (StatusCode::OK, Json(anthropic_res))
                                                .into_response();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let err_json: Value =
                        serde_json::from_str(&text).unwrap_or(json!({ "error": text }));
                    (status, Json(err_json)).into_response()
                } else {
                    // Passthrough raw Anthropic response headers and body
                    let headers_to_forward = response.headers().clone();
                    if status.is_success() {
                        let mut output = axum::response::Response::new(
                            axum::body::Body::from_stream(response.bytes_stream()),
                        );
                        *output.status_mut() = status;
                        copy_safe_response_headers(&headers_to_forward, output.headers_mut());
                        return output;
                    }
                    let text = read_bounded_error(response).await;
                    if may_retry_stale_model(false, true, &text) {
                        let retry_settings = refresh_settings_for_retry(&settings, &api_key)
                            .await
                            .unwrap_or_else(|| settings.clone());
                        if let Some(rewrite) =
                            rewrite_stale_model_request(&proxy_body, &retry_settings, &req_model)
                        {
                            tracing::warn!(
                                "[model fallback] model error ({}), retrying {} with {}",
                                status_u16,
                                req_model,
                                rewrite.fallback_model
                            );
                            let retry_req = build_upstream_request(
                                crate::server::http_client(),
                                &target_url,
                                rewrite.updated_body.to_string(),
                                &headers,
                                &api_key,
                                &retry_settings.real_auth_scheme,
                            );

                            if let Ok(retry_req) = retry_req {
                                if let Ok(retry_response) = retry_req.send().await {
                                    let mut res_builder = axum::response::Response::builder()
                                        .status(retry_response.status());
                                    for (name, value) in retry_response.headers() {
                                        res_builder =
                                            res_builder.header(name.clone(), value.clone());
                                    }
                                    let body = axum::body::Body::from_stream(
                                        retry_response.bytes_stream(),
                                    );
                                    return res_builder.body(body).unwrap();
                                }
                            }
                        }
                    }

                    let mut res_builder = axum::response::Response::builder().status(status);
                    for (name, value) in &headers_to_forward {
                        res_builder = res_builder.header(name.clone(), value.clone());
                    }
                    res_builder.body(axum::body::Body::from(text)).unwrap()
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

#[cfg(test)]
mod tests;
