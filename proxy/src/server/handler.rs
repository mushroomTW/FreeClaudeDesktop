use crate::config_service::{load_runtime_settings, unprotect_runtime_api_key};
use crate::conversion::request_converter::anthropic_to_openai_request;
use crate::conversion::response_converter::{
    normalize_chat_completions_url, normalize_messages_url,
    normalize_models_response_with_overrides_and_prefer1m, openai_to_anthropic_response,
    prepare_proxy_body, rewrite_stale_model_request,
};
use crate::optimization;
use crate::server::streaming::{ReasoningReplayMode, start_sse_stream_conversion};
use crate::{Settings, to_public_config};
use axum::{
    Json,
    body::Bytes,
    extract::{
        WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    response::Html,
    response::IntoResponse,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime};
use url::Url;

const MAX_UPSTREAM_ERROR_BYTES: usize = 64 * 1024;
const MAX_UPSTREAM_ERROR_PREVIEW_CHARS: usize = 4096;

use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

struct ActiveCompanion {
    tx: mpsc::UnboundedSender<ProxyToCompanionMessage>,
}

struct ProxyToCompanionMessage {
    request_id: String,
    payload: String,
    response_tx: oneshot::Sender<Result<Value, String>>,
}

static ACTIVE_COMPANION: tokio::sync::Mutex<Option<ActiveCompanion>> =
    tokio::sync::Mutex::const_new(None);

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminSettingsUpdate {
    pub base_url: String,
    pub auth_scheme: String,
    pub api_key: Option<String>,

    #[serde(default)]
    pub real_model: Option<Option<String>>,
    #[serde(default)]
    pub real_model_sonnet: Option<Option<String>>,
    #[serde(default)]
    pub real_model_opus: Option<Option<String>>,
    #[serde(default)]
    pub real_model_haiku: Option<Option<String>>,
    #[serde(default)]
    pub real_model_routes: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub real_model_reasoning_efforts: Option<std::collections::HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub discovered_models: Option<Vec<String>>,
    #[serde(default)]
    pub model_reasoning_overrides: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub model_1m_overrides: Option<std::collections::HashMap<String, bool>>,
    #[serde(default)]
    pub model_1m_prefer_overrides: Option<std::collections::HashMap<String, bool>>,
    #[serde(default)]
    pub model_visibility_overrides: Option<std::collections::HashMap<String, bool>>,

    #[serde(default)]
    pub transport_type: Option<String>,
    #[serde(default)]
    pub reasoning_replay_mode: Option<String>,
    #[serde(default)]
    pub enable_quota_check_mock: Option<bool>,
    #[serde(default)]
    pub enable_prefix_detection: Option<bool>,
    #[serde(default)]
    pub enable_title_generation_skip: Option<bool>,
    #[serde(default)]
    pub enable_suggestion_mode_skip: Option<bool>,
    #[serde(default)]
    pub enable_filepath_extraction_mock: Option<bool>,
    #[serde(default)]
    pub enable_web_server_tools: Option<bool>,
    #[serde(default)]
    pub web_fetch_allowed_schemes: Option<String>,
    #[serde(default)]
    pub web_fetch_allow_private_networks: Option<bool>,
    #[serde(default)]
    pub theme_mode: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub custom_claude_path: Option<Option<String>>,
}

use free_claude_core::AdminRpcRequest;

/// 驗證 `validate_gateway_url` 所需的條件。
fn validate_gateway_url(base_url: &str) -> Result<String, &'static str> {
    let base_url = base_url.trim().trim_end_matches('/');
    let parsed = Url::parse(base_url).map_err(|_| "Gateway URL 格式無效")?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
        return Err("Gateway URL 必須使用 HTTP 或 HTTPS");
    }

    if parsed.scheme() == "http"
        && !matches!(
            parsed.host_str(),
            Some("localhost") | Some("127.0.0.1") | Some("::1")
        )
    {
        return Err("非本機 Gateway 必須使用 HTTPS");
    }

    Ok(base_url.to_string())
}

/// 正規化 `normalize_custom_claude_path` 所處理的資料。
fn normalize_custom_claude_path(path: Option<String>) -> Option<String> {
    path.and_then(|path| {
        let path = path.trim().to_string();
        (!path.is_empty()).then_some(path)
    })
}

/// 轉換或更新 `apply_settings_update` 所處理的內容。
fn apply_settings_update(
    settings: &mut Settings,
    input: AdminSettingsUpdate,
) -> Result<Value, (StatusCode, Json<Value>)> {
    let base_url = validate_gateway_url(&input.base_url)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
    let auth_scheme = input.auth_scheme.trim().to_ascii_lowercase();
    if !matches!(auth_scheme.as_str(), "bearer" | "x-api-key") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "authScheme 必須是 bearer 或 x-api-key" })),
        ));
    }

    settings.real_base_url = base_url;
    settings.real_auth_scheme = auth_scheme;
    if let Some(api_key) = input.api_key.map(|key| key.trim().to_string())
        && !api_key.is_empty()
    {
        // 暫存明文只供後續驗證；驗證成功後由 config_service 寫入金鑰庫。
        settings.real_api_key = api_key;
    }

    if let Some(val) = input.real_model {
        settings.real_model = val;
    }
    if let Some(val) = input.real_model_sonnet {
        settings.real_model_sonnet = val;
    }
    if let Some(val) = input.real_model_opus {
        settings.real_model_opus = val;
    }
    if let Some(val) = input.real_model_haiku {
        settings.real_model_haiku = val;
    }
    if let Some(val) = input.real_model_routes {
        settings.real_model_routes = val;
    }
    if let Some(val) = input.real_model_reasoning_efforts {
        settings.real_model_reasoning_efforts = val;
    }
    if let Some(val) = input.discovered_models {
        settings.discovered_models = val;
    }
    if let Some(val) = input.model_reasoning_overrides {
        settings.model_reasoning_overrides = val;
    }
    if let Some(val) = input.model_1m_overrides {
        settings.model_1m_overrides = val;
    }
    if let Some(val) = input.model_1m_prefer_overrides {
        settings.model_1m_prefer_overrides = val;
    }
    if let Some(val) = input.model_visibility_overrides {
        settings.model_visibility_overrides = val;
    }

    if let Some(val) = input.transport_type {
        settings.transport_type = val;
    }
    if let Some(val) = input.reasoning_replay_mode {
        settings.reasoning_replay_mode = val;
    }
    if let Some(val) = input.enable_quota_check_mock {
        settings.enable_quota_check_mock = val;
    }
    if let Some(val) = input.enable_prefix_detection {
        settings.enable_prefix_detection = val;
    }
    if let Some(val) = input.enable_title_generation_skip {
        settings.enable_title_generation_skip = val;
    }
    if let Some(val) = input.enable_suggestion_mode_skip {
        settings.enable_suggestion_mode_skip = val;
    }
    if let Some(val) = input.enable_filepath_extraction_mock {
        settings.enable_filepath_extraction_mock = val;
    }
    if let Some(val) = input.enable_web_server_tools {
        settings.enable_web_server_tools = val;
    }
    if let Some(val) = input.web_fetch_allowed_schemes {
        settings.web_fetch_allowed_schemes = val;
    }
    if let Some(val) = input.web_fetch_allow_private_networks {
        settings.web_fetch_allow_private_networks = val;
    }
    if let Some(val) = input.theme_mode {
        settings.theme_mode = val;
    }
    if let Some(val) = input.language {
        settings.language = val;
    }
    if let Some(path) = input.custom_claude_path {
        // Proxy 可能執行於 Docker 容器，無法依宿主平台語意驗證路徑。
        // 實際啟動時由宿主端 Companion 的 launch_claude 驗證。
        settings.custom_claude_path = normalize_custom_claude_path(path);
    }
    Ok(to_public_config(settings))
}

/// 讀取 `load_authorized_settings` 所需的資料。
async fn load_authorized_settings(
    _headers: &HeaderMap,
) -> Result<Settings, (StatusCode, Json<Value>)> {
    let settings = match load_runtime_settings().await {
        Ok(Some(settings)) => settings,
        Ok(None) => Settings::default(),
        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            ));
        }
    };

    Ok(settings)
}

/// 判斷是否符合 `is_model_gone_or_invalid_error` 的條件。
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

/// 判斷是否符合 `may_retry_stale_model` 的條件。
fn may_retry_stale_model(output_started: bool, retry_available: bool, error: &str) -> bool {
    !output_started && retry_available && is_model_gone_or_invalid_error(error)
}

/// 處理 `request_diagnostic` 對應的請求。
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

/// 建立 `build_upstream_request` 所需的結果。
fn build_upstream_request(
    client: &Client,
    target_url: &str,
    body: String,
    headers: &HeaderMap,
    api_key: &str,
    auth_scheme: &str,
    is_anthropic_native: bool,
) -> crate::AppResult<reqwest::RequestBuilder> {
    let mut request = client.post(target_url).body(body);

    let skip_header = if !api_key.is_empty() {
        Some(free_claude_core::resolve_auth_header_name(
            auth_scheme,
            target_url,
        )?)
    } else {
        None
    };

    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if let Some(skip) = skip_header
            && lower == skip
        {
            continue;
        }

        if is_anthropic_native {
            if !matches!(lower.as_str(), "host" | "content-length" | "connection") {
                request = request.header(name.clone(), value.clone());
            }
        } else {
            if matches!(
                lower.as_str(),
                "content-type" | "accept" | "user-agent" | "accept-encoding" | "connection"
            ) || lower.starts_with("anthropic-")
            {
                request = request.header(name.clone(), value.clone());
            }
        }
    }

    crate::server::apply_gateway_auth(request, auth_scheme, api_key, target_url)
}

/// 建立 `copy_safe_response_headers` 所需的結果。
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

/// 讀取 `read_bounded_error` 所需的資料。
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

/// 執行 `refresh_settings_for_retry` 對應的處理流程。
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
    let normalized = normalize_models_response_with_overrides_and_prefer1m(
        raw,
        &settings.model_reasoning_overrides,
        &settings.model_1m_overrides,
        &settings.model_1m_prefer_overrides,
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

/// 執行 `reasoning_mode_from` 對應的處理流程。
fn reasoning_mode_from(settings: &Settings) -> Option<ReasoningReplayMode> {
    match settings.reasoning_replay_mode.as_str() {
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

/// 上游因模型失效/無效而報錯時，刷新路由並改用一個備援模型重試。
///
/// 三種轉發路徑（串流、OpenAI 非串流、Anthropic 直通）共用同一套「刷新→改寫→
/// 重建→送出」流程，避免各自複製一份而發生行為分歧。`is_anthropic_native`
/// 需與外層一致：Anthropic 直通傳 `true`，OpenAI 相容路徑傳 `false`。
/// `require_success` 為 `true` 時只有成功回應才回傳（OpenAI 路徑需再轉換），
/// 為 `false` 時原樣回傳重試結果（Anthropic 直通）。
#[allow(clippy::too_many_arguments)]
async fn try_stale_model_retry(
    settings: &Settings,
    api_key: &str,
    proxy_body: &str,
    req_model: &str,
    target_url: &str,
    headers: &HeaderMap,
    is_anthropic_native: bool,
    error_text: &str,
    require_success: bool,
) -> Option<reqwest::Response> {
    if !may_retry_stale_model(false, true, error_text) {
        return None;
    }
    let retry_settings = refresh_settings_for_retry(settings, api_key)
        .await
        .unwrap_or_else(|| settings.clone());
    let rewrite = rewrite_stale_model_request(proxy_body, &retry_settings, req_model)?;
    tracing::warn!(
        "[model fallback] model error, retrying {} with {}",
        req_model,
        rewrite.fallback_model
    );
    let request = build_upstream_request(
        crate::server::http_client(),
        target_url,
        rewrite.updated_body.to_string(),
        headers,
        api_key,
        &retry_settings.real_auth_scheme,
        is_anthropic_native,
    )
    .ok()?;
    let response = request.send().await.ok()?;
    if require_success && !response.status().is_success() {
        return None;
    }
    Some(response)
}

/// 攔截 Claude Desktop 的背景連線健康檢查（probe）並立即回傳成功回應。
///
/// 只有三條件同時成立才視為 probe，避免誤吃使用者的正式請求：
///   1. `messages` 解析為空 array（人類對話不可能丟空訊息）
///   2. `max_tokens` 很小（避免誤吃沒有 body 但有 tools 的請求）
///   3. body 極短（背景 ping 不會帶大 system 與 tools schemas）
///
/// 不能單看 `max_tokens <= 5`，因為 Anthropic 官方認可 `max_tokens=1` 的多選題
/// 預填用法；若只看 max_tokens 會把正式請求攔下改回假的「.」，造成切換模型時
/// 「訊息送出但什麼都沒發生」的錯覺。
fn try_probe_response(body_str: &str, req_model: &str) -> Option<axum::response::Response> {
    let value = serde_json::from_str::<Value>(body_str).ok()?;
    let max_tokens = value
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(9999);
    let is_probe_stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let messages_empty = value
        .get("messages")
        .and_then(Value::as_array)
        .map(|arr| arr.is_empty())
        .unwrap_or(false);
    let has_user_content = value
        .get("messages")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter().any(|m| {
                m.get("role").and_then(Value::as_str) == Some("user")
                    && m.get("content").map(|c| !c.is_null()).unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if has_user_content || !messages_empty || max_tokens > 5 || body_str.len() >= 400 {
        return None;
    }

    tracing::info!(
        "-> [探測攔截] 繞過 Claude 檢查，自動回傳成功回應 (model: {})",
        req_model
    );

    let msg_id = format!(
        "msg_probe_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis()
    );

    if is_probe_stream {
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
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n"
                .to_string(),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n"
                .to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ];

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::convert::Infallible>>(10);
        tokio::spawn(async move {
            for event in events {
                let _ = tx.send(Ok(Bytes::from(event))).await;
            }
        });
        Some(sse_stream_response(rx))
    } else {
        let probe_res = json!({
            "id": msg_id,
            "type": "message",
            "role": "assistant",
            "content": [ { "type": "text", "text": "." } ],
            "model": req_model,
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        });
        Some((StatusCode::OK, Json(probe_res)).into_response())
    }
}

/// 在上游回傳空白成功本文時辨識 Claude Desktop 的短連線探測。
fn is_short_connection_probe(body_str: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body_str) else {
        return false;
    };
    let short_user_message = value
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| messages.len() == 1)
        .and_then(|messages| messages.first())
        .is_some_and(|message| {
            message.get("role").and_then(Value::as_str) == Some("user")
                && message
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| content.len() <= 32)
        });

    short_user_message
        && value.get("max_tokens").and_then(Value::as_u64) == Some(1)
        && value.get("stream").and_then(Value::as_bool) != Some(true)
        && value.get("tools").is_none()
        && value.get("system").is_none()
        && body_str.len() <= 256
}

/// 建立非串流 Claude Desktop 連線探測的最小成功回應。
fn non_stream_probe_response(req_model: &str) -> axum::response::Response {
    let msg_id = format!(
        "msg_probe_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis()
    );
    (
        StatusCode::OK,
        Json(json!({
            "id": msg_id,
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "text", "text": "." }],
            "model": req_model,
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })),
    )
        .into_response()
}

/// 回傳代理服務的基本運作訊息。
/// 將上游 2xx 但無法解析的 OpenAI 回應轉成 Claude 可診斷的錯誤。
fn invalid_openai_response(
    upstream_status: reqwest::StatusCode,
    response_body: &str,
    parse_error: &str,
    request_body: &str,
    req_model: &str,
) -> axum::response::Response {
    if upstream_status.is_success() && is_short_connection_probe(request_body) {
        tracing::warn!(
            "上游探測回應無法解析，改回傳本機探測成功結果（model: {req_model}）：{parse_error}"
        );
        return non_stream_probe_response(req_model);
    }

    let trimmed_body = response_body.trim();
    let response_preview = if trimmed_body.is_empty() {
        "<empty>".to_string()
    } else {
        let mut preview: String = trimmed_body
            .chars()
            .take(MAX_UPSTREAM_ERROR_PREVIEW_CHARS)
            .collect();
        if preview.len() < trimmed_body.len() {
            preview.push_str("...");
        }
        preview
    };
    let error_message = if trimmed_body.is_empty() {
        "上游回應本文為空".to_string()
    } else {
        format!("上游回應不是有效的 OpenAI JSON：{parse_error}")
    };

    tracing::error!(
        "上游回傳 HTTP {}，但本文無法轉換為 Claude Messages 回應：{}",
        upstream_status.as_u16(),
        parse_error
    );

    (
        axum::http::StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": error_message,
            "upstreamStatus": upstream_status.as_u16(),
            "responseBody": response_preview
        })),
    )
        .into_response()
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

/// 處理 `handle_launcher_show` 對應的請求。
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

/// 處理 `handle_healthz` 對應的請求。
pub async fn handle_healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// 處理 `handle_admin_page` 對應的請求。
pub async fn handle_admin_page() -> impl IntoResponse {
    (
        [
            (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
            (header::PRAGMA, "no-cache"),
            (header::EXPIRES, "0"),
        ],
        Html(include_str!("admin.html")),
    )
}

/// 處理 `handle_admin_settings` 對應的請求。
pub async fn handle_admin_settings(headers: HeaderMap) -> impl IntoResponse {
    match load_authorized_settings(&headers).await {
        Ok(settings) => (StatusCode::OK, Json(to_public_config(&settings))).into_response(),
        Err(response) => response.into_response(),
    }
}

/// 轉換或更新 `update_admin_settings` 所處理的內容。
pub async fn update_admin_settings(
    headers: HeaderMap,
    Json(input): Json<AdminSettingsUpdate>,
) -> impl IntoResponse {
    let mut settings = match load_authorized_settings(&headers).await {
        Ok(settings) => settings,
        Err(response) => return response.into_response(),
    };
    let new_api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string);
    let updated_config = match apply_settings_update(&mut settings, input) {
        Ok(res) => res,
        Err(response) => return response.into_response(),
    };

    let port = settings
        .active_port
        .unwrap_or(crate::constants::DEFAULT_PORT);
    let raw_api_key = match new_api_key {
        Some(key) => key,
        None => {
            match free_claude_core::unprotect_runtime_api_key(settings.real_api_key.clone()).await {
                Ok(key) => key,
                Err(error) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": error.to_string() })),
                    )
                        .into_response();
                }
            }
        }
    };

    let save_input = free_claude_core::SaveConfigInput {
        port,
        base_url: settings.real_base_url.clone(),
        api_key: raw_api_key,
        auth_scheme: settings.real_auth_scheme.clone(),
        enable_quota_check_mock: settings.enable_quota_check_mock,
        enable_prefix_detection: settings.enable_prefix_detection,
        enable_title_generation_skip: settings.enable_title_generation_skip,
        enable_suggestion_mode_skip: settings.enable_suggestion_mode_skip,
        enable_filepath_extraction_mock: settings.enable_filepath_extraction_mock,
        enable_web_server_tools: settings.enable_web_server_tools,
        web_fetch_allow_private_networks: settings.web_fetch_allow_private_networks,
        reasoning_replay_mode: settings.reasoning_replay_mode.clone(),
        transport_type: settings.transport_type.clone(),
        web_fetch_allowed_schemes: settings.web_fetch_allowed_schemes.clone(),
        theme_mode: settings.theme_mode.clone(),
        language: settings.language.clone(),
        model_reasoning_overrides: settings.model_reasoning_overrides.clone(),
        model_1m_overrides: settings.model_1m_overrides.clone(),
        model_1m_prefer_overrides: settings.model_1m_prefer_overrides.clone(),
        model_visibility_overrides: settings.model_visibility_overrides.clone(),
        custom_claude_path: Some(settings.custom_claude_path.clone()),
        real_model: settings.real_model.clone(),
        real_model_sonnet: settings.real_model_sonnet.clone(),
        real_model_opus: settings.real_model_opus.clone(),
        real_model_haiku: settings.real_model_haiku.clone(),
    };

    if let Err(error) = free_claude_core::save_config_async(save_input).await {
        tracing::error!("同步 3P Gateway 設定給 Claude Desktop 失敗: {error}");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response();
    }
    tracing::info!("<- 已成功為 Claude Desktop 部署與套用 3P Gateway 設定！");

    (StatusCode::OK, Json(updated_config)).into_response()
}

/// 處理 `handle_admin_status` 對應的請求。
pub async fn handle_admin_status(headers: HeaderMap) -> impl IntoResponse {
    match load_authorized_settings(&headers).await {
        Ok(settings) => (
            StatusCode::OK,
            Json(json!({
                "proxy": { "status": "ok", "port": settings.active_port },
                "settings": to_public_config(&settings),
            })),
        )
            .into_response(),
        Err(response) => response.into_response(),
    }
}

/// 處理 `handle_admin_rpc` 對應的請求。
pub async fn handle_admin_rpc(
    headers: HeaderMap,
    Json(request): Json<AdminRpcRequest>,
) -> impl IntoResponse {
    let settings = match load_authorized_settings(&headers).await {
        Ok(settings) => settings,
        Err(response) => return response.into_response(),
    };

    if matches!(request, AdminRpcRequest::GetStatus) {
        return (
            StatusCode::OK,
            Json(json!({
                "result": {
                    "proxy": { "status": "ok", "port": settings.active_port },
                    "settings": to_public_config(&settings),
                }
            })),
        )
            .into_response();
    }

    if matches!(request, AdminRpcRequest::FetchModels) {
        super::models_endpoint::clear_models_cache();
        let resp = super::models_endpoint::handle_models(HeaderMap::new()).await;
        return resp.into_response();
    }

    // Check active companion connection
    let tx_opt = {
        let active = ACTIVE_COMPANION.lock().await;
        active.as_ref().map(|c| c.tx.clone())
    };

    let companion_tx = match tx_opt {
        Some(tx) => tx,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Companion offline" })),
            )
                .into_response();
        }
    };

    let request_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();

    let mut payload_val = serde_json::to_value(&request).unwrap_or(Value::Null);
    if let Some(obj) = payload_val.as_object_mut() {
        obj.insert("requestId".to_string(), Value::String(request_id.clone()));
    }

    let (response_tx, response_rx) = oneshot::channel();
    let msg = ProxyToCompanionMessage {
        request_id,
        payload: payload_val.to_string(),
        response_tx,
    };

    if companion_tx.send(msg).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Companion offline" })),
        )
            .into_response();
    }

    match response_rx.await {
        Ok(Ok(res)) => (StatusCode::OK, Json(json!({ "result": res }))).into_response(),
        Ok(Err(err)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Companion disconnected" })),
        )
            .into_response(),
    }
}

/// 處理 `handle_companion_websocket` 對應的請求。
pub async fn handle_companion_websocket(websocket: WebSocketUpgrade) -> impl IntoResponse {
    websocket.on_upgrade(handle_companion_session)
}

/// 處理 `handle_companion_session` 對應的請求。
async fn handle_companion_session(socket: WebSocket) {
    let (tx, mut rx) = mpsc::unbounded_channel::<ProxyToCompanionMessage>();
    {
        let mut active = ACTIVE_COMPANION.lock().await;
        *active = Some(ActiveCompanion { tx });
    }

    #[allow(clippy::type_complexity)]
    let pending_requests: Arc<
        tokio::sync::Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
    > = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let (mut ws_sink, mut ws_stream) = socket.split();

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(proxy_msg) => {
                        pending_requests.lock().await.insert(proxy_msg.request_id.clone(), proxy_msg.response_tx);
                        if ws_sink.send(Message::Text(proxy_msg.payload.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            ws_msg = ws_stream.next() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(resp_val) = serde_json::from_str::<Value>(&text)
                            && let Some(req_id) = resp_val.get("requestId").and_then(|v| v.as_str()) {
                                let mut pending = pending_requests.lock().await;
                                if let Some(tx) = pending.remove(req_id) {
                                    if let Some(err) = resp_val.get("error").and_then(|v| v.as_str()) {
                                        let _ = tx.send(Err(err.to_string()));
                                    } else if let Some(err_val) = resp_val.get("error") {
                                        let _ = tx.send(Err(err_val.to_string()));
                                    } else if let Some(res) = resp_val.get("result") {
                                        let _ = tx.send(Ok(res.clone()));
                                    } else {
                                        let _ = tx.send(Err("Invalid WS RPC format".to_string()));
                                    }
                                }
                            }
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

    // Cleanup
    {
        let mut active = ACTIVE_COMPANION.lock().await;
        *active = None;
    }
    let mut pending = pending_requests.lock().await;
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err("Companion disconnected".to_string()));
    }
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
        request: AdminRpcRequest,
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
        assert!(serde_json::from_str::<AdminRpcRequest>(r#"{"method":"GetStatus"}"#).is_ok());
        assert!(serde_json::from_str::<AdminRpcRequest>(r#"{"method":"LaunchClaude"}"#).is_ok());
        assert!(serde_json::from_str::<AdminRpcRequest>(
            r#"{"method":"ApplySettings","baseUrl":"https://gateway.example/v1","authScheme":"bearer"}"#
        )
        .is_ok());
        assert!(
            serde_json::from_str::<AdminRpcRequest>(r#"{"method":"DeleteEverything"}"#).is_err()
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

    // 3. Connection Probe interception：攔截背景連線健康檢查（詳見 try_probe_response）。
    if let Some(response) = try_probe_response(&body_str, &req_model) {
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

    // 所有轉發路徑共用同一個 request builder，確保非串流請求也遵守
    // bearer / x-api-key 設定，並讓上游錯誤維持原始狀態碼與 JSON 格式。
    // Body 仍以原始字串傳送，不會遺失 reasoning、影像或 Gateway 自訂欄位。
    let upstream_req = match build_upstream_request(
        crate::server::http_client(),
        &target_url,
        proxy_body.clone(),
        &headers,
        &api_key,
        &settings.real_auth_scheme,
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
