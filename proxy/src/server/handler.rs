use crate::server::streaming::{ReasoningReplayMode, start_sse_stream_conversion};
use axum::{
    Json,
    body::Bytes,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use free_claude_core::Settings;
use free_claude_core::config_service::{load_runtime_settings, unprotect_runtime_api_key};
use free_claude_core::conversion::request_converter::anthropic_to_openai_request;
use free_claude_core::conversion::response_converter::{
    normalize_chat_completions_url, normalize_messages_url, openai_to_anthropic_response,
    prepare_proxy_body,
};
use free_claude_core::optimization;
#[cfg(test)]
use free_claude_core::to_public_config;
use serde_json::{Value, json};
use std::time::Instant;

pub use super::companion::handle_companion_websocket;
#[cfg(test)]
use super::companion::{ActiveCompanion, CompanionState, ProxyToCompanionMessage};
pub use super::dashboard_assets::{
    handle_dashboard_css, handle_dashboard_js, handle_dashboard_page,
};
pub use super::dashboard_settings::{
    DashboardSettingsUpdate, handle_dashboard_rpc, handle_dashboard_settings,
    handle_dashboard_status, update_dashboard_settings,
};
#[cfg(test)]
use super::dashboard_settings::{normalize_custom_claude_path, validate_gateway_url};
#[cfg(test)]
use super::messages_probe::MAX_UPSTREAM_ERROR_PREVIEW_CHARS;
use super::messages_probe::{
    invalid_openai_response, is_short_connection_probe, non_stream_probe_response,
    request_diagnostic, try_probe_response,
};
use super::model_retry::try_stale_model_retry;
#[cfg(test)]
use super::model_retry::{is_model_gone_or_invalid_error, may_retry_stale_model};
use super::upstream::{build_upstream_request, copy_safe_response_headers, read_bounded_error};
use tokio::sync::mpsc;

#[cfg(test)]
use free_claude_core::DashboardRpcRequest;

/// 執行 `reasoning_mode_from` 對應的處理流程。
fn reasoning_mode_from(settings: &Settings) -> Option<ReasoningReplayMode> {
    match settings.models.reasoning_replay_mode.as_str() {
        "inline" => Some(ReasoningReplayMode::Inline),
        "separate" => Some(ReasoningReplayMode::Separate),
        _ => None,
    }
}

/// 執行 `sse_stream_response` 對應的處理流程。
fn sse_stream_response(
    rx: mpsc::Receiver<Result<Bytes, std::convert::Infallible>>,
) -> axum::response::Response {
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(axum::body::Body::from_stream(stream))
        .unwrap()
}

/// 建立不含提示詞、工具參數或驗證資訊的 API 請求摘要。
fn api_request_summary(body: &str) -> Value {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message_count = parsed
        .as_ref()
        .and_then(|value| value.get("messages"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let tool_count = parsed
        .as_ref()
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    json!({
        "model": parsed
            .as_ref()
            .and_then(|value| value.get("model"))
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        "stream": parsed
            .as_ref()
            .and_then(|value| value.get("stream"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "maxTokens": parsed
            .as_ref()
            .and_then(|value| value.get("max_tokens"))
            .and_then(Value::as_u64),
        "messageCount": message_count,
        "toolCount": tool_count,
        "hasSystem": parsed
            .as_ref()
            .and_then(|value| value.get("system"))
            .is_some(),
        "requestBytes": body.len(),
        "validJson": parsed.is_some(),
    })
}

/// 移除查詢字串與片段，避免 API 紀錄意外保存 URL 內的敏感資料。
fn redacted_url(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// 記錄一次 API 請求結果。
struct ApiOutcome<'a> {
    enabled: bool,
    call_id: u64,
    request: &'a Value,
    target_url: &'a str,
    transport: &'a str,
    outcome: &'a str,
    elapsed_ms: u128,
    status: Option<u16>,
    content_type: Option<&'a str>,
    error: Option<&'a str>,
}

fn record_api_outcome(outcome: ApiOutcome<'_>) {
    if !outcome.enabled {
        return;
    }
    super::api_log::record_api_call(json!({
        "timestampMs": super::api_log::unix_time_ms(),
        "callId": outcome.call_id,
        "outcome": outcome.outcome,
        "transport": outcome.transport,
        "targetUrl": redacted_url(outcome.target_url),
        "status": outcome.status,
        "contentType": outcome.content_type,
        "elapsedMs": outcome.elapsed_ms,
        "request": outcome.request,
        "error": outcome.error,
    }));
}

pub async fn handle_root() -> impl IntoResponse {
    "FreeClaudeDesktop API proxy is running"
}

/// 處理 `handle_app_icon` 對應的請求。
pub async fn handle_app_icon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        include_bytes!("../../../icon.png").as_slice(),
    )
}

/// 處理 `handle_healthz` 對應的請求。
pub async fn handle_healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod healthz_tests {
    use super::*;

    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[allow(dead_code)]
    struct CompanionRequest {
        request_id: String,
        #[serde(flatten)]
        request: DashboardRpcRequest,
    }

    #[tokio::test]
    /// 驗證 `healthz_returns_ok_status` 的行為符合預期。
    async fn healthz_returns_ok_status() {
        assert_eq!(handle_healthz().await.0, json!({ "status": "ok" }));
    }

    #[test]
    /// 驗證 `gateway_url_requires_https_except_for_loopback` 的行為符合預期。
    fn gateway_url_requires_https_except_for_loopback() {
        assert_eq!(
            validate_gateway_url("http://127.0.0.1:4000/").unwrap(),
            "http://127.0.0.1:4000"
        );
        assert!(validate_gateway_url("https://gateway.example/v1").is_ok());
        assert!(validate_gateway_url("http://gateway.example").is_err());
        assert!(validate_gateway_url("file:///tmp/gateway").is_err());
    }

    #[test]
    /// 驗證 `custom_claude_path_preserves_host_platform_syntax` 的行為符合預期。
    fn custom_claude_path_preserves_host_platform_syntax() {
        assert_eq!(
            normalize_custom_claude_path(Some(
                r"  C:\Program Files\Claude\Claude.exe  ".to_string()
            )),
            Some(r"C:\Program Files\Claude\Claude.exe".to_string())
        );
        assert_eq!(normalize_custom_claude_path(Some("   ".to_string())), None);
    }

    #[test]
    /// 驗證 `rpc_request_uses_allowlist` 的行為符合預期。
    fn rpc_request_uses_allowlist() {
        assert!(serde_json::from_str::<DashboardRpcRequest>(r#"{"method":"GetStatus"}"#).is_ok());
        assert!(
            serde_json::from_str::<DashboardRpcRequest>(r#"{"method":"LaunchClaude"}"#).is_ok()
        );
        assert!(serde_json::from_str::<DashboardRpcRequest>(
            r#"{"method":"ApplySettings","baseUrl":"https://gateway.example/v1","authScheme":"bearer"}"#
        )
        .is_ok());
        assert!(
            serde_json::from_str::<DashboardRpcRequest>(r#"{"method":"DeleteEverything"}"#)
                .is_err()
        );
    }

    #[test]
    /// 驗證 `companion_request_requires_request_id_only` 的行為符合預期。
    fn companion_request_requires_request_id_only() {
        assert!(
            serde_json::from_str::<CompanionRequest>(r#"{"requestId":"1","method":"GetStatus"}"#)
                .is_ok()
        );
        assert!(serde_json::from_str::<CompanionRequest>(r#"{"method":"GetStatus"}"#).is_err());
    }
}

/// 處理 `handle_proxy` 對應的請求。
pub async fn handle_proxy(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    // Debug: log request headers without leaking local/upstream credentials.
    for (name, value) in &headers {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(lower.as_str(), "authorization" | "x-api-key" | "cookie")
            || lower.contains("key")
            || lower.contains("token")
            || lower.contains("secret")
            || lower.contains("auth")
        {
            tracing::debug!("[req header] {}: <redacted>", name);
        } else {
            tracing::debug!("[req header] {}: {:?}", name, value);
        }
    }
    if let Some(origin) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        tracing::info!("[req header] Origin: {}", origin);
    }

    // 1. Load settings.
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

    let body_str = String::from_utf8_lossy(&body);
    let call_id = super::api_log::next_call_id();
    let started_at = Instant::now();
    let request_summary = api_request_summary(&body_str);

    // 3. Try local optimizations (quota mock, prefix detection, etc.)
    if let Some(response) = optimization::try_optimizations(&body_str, &settings).await {
        record_api_outcome(ApiOutcome {
            enabled: settings.optimizations.enable_api_call_logging,
            call_id,
            request: &request_summary,
            target_url: "local://optimization",
            transport: "local",
            outcome: "optimized",
            elapsed_ms: started_at.elapsed().as_millis(),
            status: Some(StatusCode::OK.as_u16()),
            content_type: None,
            error: None,
        });
        return super::optimization_response::into_response(response);
    }

    if let Some(diagnostic) = request_diagnostic(&body_str) {
        tracing::info!("[未攔截請求] {diagnostic}");
    }

    // 4. Determine transport type
    let is_anthropic_native = settings.gateway.transport_type == "anthropic_messages"
        || (settings.gateway.transport_type.is_empty()
            && settings.gateway.real_base_url.contains("api.anthropic.com"));

    let is_openai_format = !is_anthropic_native;

    let req_model = match serde_json::from_str::<Value>(&body_str) {
        Ok(v) => v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        Err(_) => "unknown".to_string(),
    };

    // 3. Connection Probe interception：攔截背景連線健康檢查（詳見 try_probe_response）。
    if let Some(response) = try_probe_response(&body_str, &req_model) {
        record_api_outcome(ApiOutcome {
            enabled: settings.optimizations.enable_api_call_logging,
            call_id,
            request: &request_summary,
            target_url: "local://connection-probe",
            transport: "local",
            outcome: "probe",
            elapsed_ms: started_at.elapsed().as_millis(),
            status: Some(StatusCode::OK.as_u16()),
            content_type: None,
            error: None,
        });
        return response;
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
        let is_stream = serde_json::from_str::<Value>(&body_str)
            .ok()
            .and_then(|v| v.get("stream").and_then(Value::as_bool))
            .unwrap_or(false);
        (prepare_proxy_body(&body_str, &settings), is_stream)
    };

    let target_url = if is_openai_format {
        match normalize_chat_completions_url(&settings.gateway.real_base_url) {
            Ok(url) => url,
            Err(error) => {
                tracing::error!("<- 錯誤: 無效的 Gateway URL: {:?}", error);
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
            }
        }
    } else {
        match normalize_messages_url(&settings.gateway.real_base_url) {
            Ok(url) => url,
            Err(error) => {
                tracing::error!("<- 錯誤: 無效的 Gateway URL: {:?}", error);
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
            }
        }
    };

    let api_key = match unprotect_runtime_api_key(settings.gateway.real_api_key.clone()).await {
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

    // 所有轉發路徑共用同一個 request builder，確保非串流請求也遵守
    // bearer / x-api-key 設定，並讓上游錯誤維持原始狀態碼與 JSON 格式。
    // Body 仍以原始字串傳送，不會遺失 reasoning、影像或 Gateway 自訂欄位。
    let upstream_req = match build_upstream_request(
        crate::server::http_client(),
        &target_url,
        proxy_body.clone(),
        &headers,
        &api_key,
        &settings.gateway.real_auth_scheme,
        is_anthropic_native,
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
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            record_api_outcome(ApiOutcome {
                enabled: settings.optimizations.enable_api_call_logging,
                call_id,
                request: &request_summary,
                target_url: &target_url,
                transport: if is_anthropic_native {
                    "anthropic_messages"
                } else {
                    "openai_chat_completions"
                },
                outcome: "upstream_response",
                elapsed_ms: started_at.elapsed().as_millis(),
                status: Some(status_u16),
                content_type: content_type.as_deref(),
                error: None,
            });

            if is_openai_format && is_stream {
                tracing::info!("<- 上游回應狀態碼(流式): {}", status_u16);
                if !status.is_success() {
                    let text = read_bounded_error(response).await;
                    tracing::error!("<- 上游流式錯誤狀態碼: {}", status_u16);
                    if let Some(retry) = try_stale_model_retry(
                        &settings,
                        &api_key,
                        &proxy_body,
                        &req_model,
                        &target_url,
                        &headers,
                        false,
                        &text,
                        true,
                    )
                    .await
                    {
                        let reasoning_mode = reasoning_mode_from(&settings);
                        let rx = start_sse_stream_conversion(retry, req_model, reasoning_mode);
                        return sse_stream_response(rx);
                    }
                    return (status, Json(json!({ "error": text }))).into_response();
                }

                let reasoning_mode = reasoning_mode_from(&settings);
                let rx = start_sse_stream_conversion(response, req_model, reasoning_mode);
                sse_stream_response(rx)
            } else {
                tracing::info!("<- 上游回應狀態碼: {}", status_u16);
                if is_openai_format && status.is_success() {
                    let response_text = match response.text().await {
                        Ok(t) => t,
                        Err(e) => {
                            return (
                                StatusCode::BAD_GATEWAY,
                                Json(json!({ "error": format!("讀取上游回應失敗：{e}") })),
                            )
                                .into_response();
                        }
                    };
                    if response_text.trim().is_empty() {
                        if is_short_connection_probe(&body_str) {
                            tracing::info!(
                                "-> [空白探測回應] 將上游空白成功本文轉為 Claude 探測成功回應"
                            );
                            return non_stream_probe_response(&req_model);
                        }
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(json!({
                                "error": "Upstream returned an empty successful response"
                            })),
                        )
                            .into_response();
                    }
                    match openai_to_anthropic_response(&response_text, &req_model) {
                        Ok(anthropic_res) => (StatusCode::OK, Json(anthropic_res)).into_response(),
                        Err(e) => invalid_openai_response(
                            status,
                            &response_text,
                            &e,
                            &body_str,
                            &req_model,
                        ),
                    }
                } else if is_openai_format {
                    let text = read_bounded_error(response).await;
                    tracing::error!("<- 上游錯誤狀態碼: {}", status_u16);
                    if let Some(retry) = try_stale_model_retry(
                        &settings,
                        &api_key,
                        &proxy_body,
                        &req_model,
                        &target_url,
                        &headers,
                        false,
                        &text,
                        true,
                    )
                    .await
                    {
                        let retry_text = retry.text().await.unwrap_or_default();
                        if let Ok(anthropic_res) =
                            openai_to_anthropic_response(&retry_text, &req_model)
                        {
                            return (StatusCode::OK, Json(anthropic_res)).into_response();
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
                    if let Some(retry) = try_stale_model_retry(
                        &settings,
                        &api_key,
                        &proxy_body,
                        &req_model,
                        &target_url,
                        &headers,
                        true,
                        &text,
                        false,
                    )
                    .await
                    {
                        let mut res_builder =
                            axum::response::Response::builder().status(retry.status());
                        for (name, value) in retry.headers() {
                            res_builder = res_builder.header(name.clone(), value.clone());
                        }
                        let body = axum::body::Body::from_stream(retry.bytes_stream());
                        return res_builder.body(body).unwrap();
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
            let error_message = error.to_string();
            record_api_outcome(ApiOutcome {
                enabled: settings.optimizations.enable_api_call_logging,
                call_id,
                request: &request_summary,
                target_url: &target_url,
                transport: if is_anthropic_native {
                    "anthropic_messages"
                } else {
                    "openai_chat_completions"
                },
                outcome: "upstream_error",
                elapsed_ms: started_at.elapsed().as_millis(),
                status: None,
                content_type: None,
                error: Some(&error_message),
            });
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
