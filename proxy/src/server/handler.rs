use crate::config_service::{load_runtime_settings, unprotect_runtime_api_key};
use crate::conversion::request_converter::anthropic_to_openai_request;
use crate::conversion::response_converter::{
    normalize_chat_completions_url, normalize_messages_url,
    normalize_models_response_with_overrides, openai_to_anthropic_response, prepare_proxy_body,
    rewrite_stale_model_request,
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

use free_claude_core::{AsyncOpenAiGatewayFactory, GatewayClientFactory};
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
}

use free_claude_core::AdminRpcRequest;

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
        settings.real_api_key = crate::protect_secret(&api_key).map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        })?;
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
    crate::save_launcher_settings(settings).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
    })?;
    crate::server::models_endpoint::clear_models_cache();
    Ok(to_public_config(settings))
}

async fn load_authorized_settings(
    _headers: &HeaderMap,
) -> Result<Settings, (StatusCode, Json<Value>)> {
    let settings = match load_runtime_settings().await {
        Ok(Some(settings)) => settings,
        Ok(None) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Launcher has not been configured yet." })),
            ));
        }
        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            ));
        }
    };

    Ok(settings)
}

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

fn reasoning_mode_from(settings: &Settings) -> Option<ReasoningReplayMode> {
    match settings.reasoning_replay_mode.as_str() {
        "inline" => Some(ReasoningReplayMode::Inline),
        "separate" => Some(ReasoningReplayMode::Separate),
        _ => None,
    }
}

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

pub async fn handle_root() -> impl IntoResponse {
    "FreeClaudeDesktop API proxy is running"
}

pub async fn handle_app_icon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        include_bytes!("../../../icon.png").as_slice(),
    )
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

pub async fn handle_admin_page() -> Html<&'static str> {
    Html(
        r##"<!doctype html>
<html lang="zh-TW">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="google" content="notranslate">
  <title>FreeClaude Admin Dashboard</title>
  <style>
    :root {
      --font-sans: 'Outfit', 'Segoe UI', system-ui, -apple-system, sans-serif;
      
      /* Dark Theme (Default) */
      --bg-main: #121212;
      --bg-sidebar: #181818;
      --bg-card: #1e1e1e;
      --bg-input: #151515;
      --bg-input-focus: #1a1a1a;
      --border-color: #2b2b2b;
      --text-main: #e0e0e0;
      --text-muted: #888888;
      --text-active: #ffffff;
      
      --primary-color: #D96B43;
      --primary-hover: #E87A53;
      --primary-active: #C85B33;
      --primary-bg-active: rgba(217, 107, 67, 0.12);
      
      --secondary-bg: #2d2d2d;
      --secondary-hover: #383838;
      --secondary-active: #242424;
      
      --success: #3ab54a;
      --warning: #f59e0b;
      --danger: #d9534f;
      --focus-ring: 0 0 0 2px rgba(217, 107, 67, 0.4);
    }

    [data-theme="light"] {
      --bg-main: #f5f5f5;
      --bg-sidebar: #eaeaea;
      --bg-card: #ffffff;
      --bg-input: #f0f0f0;
      --bg-input-focus: #ffffff;
      --border-color: #dcdcdc;
      --text-main: #222222;
      --text-muted: #666666;
      --text-active: #000000;
      
      --primary-color: #E06A3B;
      --primary-hover: #F07A4B;
      --primary-active: #D05A2B;
      --primary-bg-active: rgba(224, 106, 59, 0.12);
      
      --secondary-bg: #e0e0e0;
      --secondary-hover: #d5d5d5;
      --secondary-active: #c8c8c8;
      
      --success: #2e7d32;
      --warning: #d97706;
      --danger: #c62828;
      --focus-ring: 0 0 0 2px rgba(224, 106, 59, 0.4);
    }

    * {
      box-sizing: border-box;
      margin: 0;
      padding: 0;
    }

    body {
      background: var(--bg-main);
      color: var(--text-main);
      font-family: var(--font-sans);
      min-height: 100vh;
      overflow-x: hidden;
      transition: background-color 0.2s, color 0.2s;
    }
    
    /* 授權驗證樣式 */
    .auth-wrapper {
      display: flex;
      align-items: center;
      justify-content: center;
      min-height: 100vh;
      width: 100vw;
      padding: 1.5rem;
      background: var(--bg-main);
    }

    #authWrapper {
      display: none !important;
    }
    
    .auth-card {
      background: var(--bg-card);
      border: 1px solid var(--border-color);
      border-radius: 8px;
      padding: 2.5rem;
      width: 100%;
      max-width: 420px;
      box-shadow: 0 4px 20px rgba(0, 0, 0, 0.2);
      text-align: center;
    }
    
    .auth-logo-area {
      margin-bottom: 1.5rem;
      display: flex;
      justify-content: center;
    }
    
    .fox-logo {
      width: 96px;
      height: 96px;
      object-fit: contain;
    }
    
    .auth-title {
      font-size: 1.8rem;
      font-weight: 700;
      color: var(--text-active);
      margin-bottom: 0.5rem;
    }
    
    .auth-subtitle {
      font-size: 0.85rem;
      color: var(--text-muted);
      margin-bottom: 2rem;
      line-height: 1.4;
    }
    
    /* 雙欄佈局樣式 */
    .main-layout {
      display: flex;
      min-height: 100vh;
      width: 100%;
    }
    
    /* 左側側邊欄 */
    .sidebar {
      width: 260px;
      background: var(--bg-sidebar);
      border-right: 1px solid var(--border-color);
      display: flex;
      flex-direction: column;
      position: sticky;
      top: 0;
      height: 100vh;
      padding: 0 0 2rem 0;
      z-index: 100;
    }
    
    .sidebar-header {
      height: 7rem;
      padding: 0 1.5rem;
      display: flex;
      align-items: center;
      gap: 0.75rem;
      border-bottom: 1px solid var(--border-color);
      margin-bottom: 1.5rem;
      box-sizing: border-box;
    }
    
    .sidebar-header .fox-logo {
      width: 72px;
      height: 72px;
      flex-shrink: 0;
    }
    
    .sidebar-title-group {
      display: flex;
      flex-direction: column;
    }
    
    .sidebar-title {
      font-size: 1.75rem;
      font-weight: 700;
      color: var(--text-active);
      line-height: 1.15;
    }
    
    .sidebar-subtitle {
      font-size: 0.75rem;
      color: var(--text-muted);
    }
    
    .sidebar-nav {
      flex-grow: 1;
      display: flex;
      flex-direction: column;
      gap: 0.25rem;
      padding: 0 0.75rem;
    }
    
    .nav-item {
      display: flex;
      align-items: center;
      padding: 0.75rem 1rem 0.75rem 2.25rem;
      color: var(--text-muted);
      text-decoration: none;
      font-size: 0.95rem;
      font-weight: 500;
      border-radius: 6px;
      position: relative;
      transition: background-color 0.15s, color 0.15s;
    }
    
    .nav-item:hover {
      background: rgba(255, 255, 255, 0.03);
      color: var(--text-main);
    }
    
    [data-theme="light"] .nav-item:hover {
      background: rgba(0, 0, 0, 0.03);
    }
    
    .nav-item.active {
      color: var(--primary-color);
      background: var(--primary-bg-active);
      font-weight: 600;
    }
    
    .nav-indicator {
      display: none;
      width: 4px;
      height: 18px;
      background: var(--primary-color);
      border-radius: 2px;
      position: absolute;
      left: 12px;
      top: 50%;
      transform: translateY(-50%);
    }
    
    .nav-item.active .nav-indicator {
      display: block;
    }
    
    .sidebar-footer {
      padding: 0 1.5rem;
      margin-top: auto;
    }
    
    /* 右側主要區域 */
    .content-area {
      flex-grow: 1;
      display: flex;
      flex-direction: column;
      background: var(--bg-main);
      position: relative;
      min-width: 0;
    }
    
    .content-header {
      height: 7rem;
      padding: 0 2.5rem;
      display: flex;
      justify-content: space-between;
      align-items: center;
      border-bottom: 1px solid var(--border-color);
      box-sizing: border-box;
    }
    
    .content-title {
      font-size: 1.75rem;
      font-weight: 700;
      color: var(--text-active);
    }
    
    .proxy-status {
      font-size: 0.85rem;
      color: var(--text-muted);
      margin-top: 0.25rem;
    }
    
    .proxy-status span {
      color: var(--text-main);
      font-family: monospace;
      background: var(--bg-sidebar);
      padding: 0.15rem 0.4rem;
      border-radius: 4px;
    }
    
    /* 三段主題切換藥丸 */
    .theme-capsule {
      display: flex;
      background: var(--bg-sidebar);
      border: 1px solid var(--border-color);
      border-radius: 20px;
      padding: 2px;
    }
    
    .theme-btn {
      background: transparent;
      border: none;
      color: var(--text-muted);
      cursor: pointer;
      display: flex;
      align-items: center;
      justify-content: center;
      width: 32px;
      height: 32px;
      border-radius: 16px;
      transition: background-color 0.15s, color 0.15s;
    }
    
    .theme-btn:hover {
      color: var(--text-main);
    }
    
    .theme-btn.active {
      background: var(--primary-color);
      color: #ffffff;
    }
    
    .tab-content-container {
      flex-grow: 1;
      padding: 2rem 2.5rem 6rem 2.5rem;
      overflow-y: auto;
    }
    
    .tab-section {
      animation: fadeIn 0.2s ease-out;
    }
    
    @keyframes fadeIn {
      from { opacity: 0; transform: translateY(4px); }
      to { opacity: 1; transform: translateY(0); }
    }
    
    .card {
      background: var(--bg-card);
      border: 1px solid var(--border-color);
      border-radius: 8px;
      padding: 2rem;
      margin-bottom: 1.5rem;
      box-shadow: 0 2px 10px rgba(0, 0, 0, 0.1);
    }
    
    .card-title {
      font-size: 1.15rem;
      font-weight: 600;
      color: var(--text-active);
      margin-bottom: 1.5rem;
      display: flex;
      align-items: center;
      gap: 0.5rem;
    }
    
    .section-desc {
      font-size: 0.85rem;
      color: var(--text-muted);
      margin-bottom: 1.5rem;
      line-height: 1.5;
    }
    
    .subsection-title {
      font-size: 1rem;
      font-weight: 600;
      color: var(--text-active);
      margin: 2rem 0 0.5rem 0;
    }
    
    /* 表單欄位與輸入項 */
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
      gap: 1.5rem;
    }
    
    .form-group {
      display: flex;
      flex-direction: column;
      gap: 0.5rem;
    }
    
    .form-group.full-width {
      grid-column: 1 / -1;
    }
    
    label {
      font-size: 0.85rem;
      font-weight: 600;
      color: var(--text-main);
    }
    
    input[type="text"],
    input[type="password"],
    input[type="url"],
    select {
      background: var(--bg-input);
      border: 1px solid var(--border-color);
      border-radius: 6px;
      padding: 0.75rem 1rem;
      color: var(--text-main);
      font-family: var(--font-sans);
      font-size: 0.95rem;
      width: 100%;
      outline: none;
      transition: border-color 0.15s, box-shadow 0.15s, background-color 0.15s;
    }
    
    input:focus, select:focus {
      background: var(--bg-input-focus);
      border-color: var(--primary-color);
      box-shadow: var(--focus-ring);
    }
    
    .select-wrapper {
      position: relative;
      width: 100%;
    }
    
    .select-wrapper select {
      appearance: none;
      -webkit-appearance: none;
      padding-right: 2.5rem;
    }
    
    .select-wrapper::after {
      content: "";
      position: absolute;
      right: 1rem;
      top: 50%;
      transform: translateY(-50%);
      width: 0;
      height: 0;
      border-left: 5px solid transparent;
      border-right: 5px solid transparent;
      border-top: 6px solid var(--text-muted);
      pointer-events: none;
    }
    
    /* Toggle switch design */
    .switch-container {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 1rem;
      background: var(--bg-input);
      border-radius: 6px;
      border: 1px solid var(--border-color);
      margin-bottom: 0.75rem;
      cursor: pointer;
      transition: background-color 0.15s;
    }
    
    .switch-container:hover {
      background: var(--bg-sidebar);
    }
    
    .switch-label {
      display: flex;
      flex-direction: column;
      gap: 0.25rem;
      flex-grow: 1;
    }
    
    .switch-label span {
      font-size: 0.9rem;
      font-weight: 600;
      color: var(--text-main);
    }
    
    .switch-desc {
      font-size: 0.75rem;
      color: var(--text-muted) !important;
      font-weight: normal !important;
    }
    
    .switch {
      position: relative;
      display: inline-block;
      width: 44px;
      height: 24px;
      flex-shrink: 0;
    }
    
    .switch input {
      opacity: 0;
      width: 0;
      height: 0;
    }
    
    .slider {
      position: absolute;
      cursor: pointer;
      top: 0; left: 0; right: 0; bottom: 0;
      background-color: var(--secondary-bg);
      transition: .2s;
      border-radius: 24px;
      border: 1px solid var(--border-color);
    }
    
    .slider:before {
      position: absolute;
      content: "";
      height: 16px;
      width: 16px;
      left: 3px;
      bottom: 3px;
      background-color: var(--text-muted);
      transition: .2s;
      border-radius: 50%;
    }
    
    input:checked + .slider {
      background-color: var(--primary-bg-active);
      border-color: var(--primary-color);
    }
    
    input:checked + .slider:before {
      transform: translateX(20px);
      background-color: var(--primary-color);
    }
    
    /* Table for Models */
    .table-container {
      overflow-x: auto;
      margin-top: 1rem;
      border-radius: 6px;
      border: 1px solid var(--border-color);
    }
    
    table {
      width: 100%;
      border-collapse: collapse;
      text-align: left;
      font-size: 0.85rem;
    }
    
    th, td {
      padding: 0.75rem 1rem;
      border-bottom: 1px solid var(--border-color);
    }
    
    th {
      background: var(--bg-sidebar);
      color: var(--text-muted);
      font-weight: 600;
    }
    
    tr:last-child td {
      border-bottom: none;
    }
    
    /* Buttons */
    .btn {
      background: var(--secondary-bg);
      color: var(--text-main);
      border: 1px solid var(--border-color);
      border-radius: 6px;
      padding: 0.75rem 1.5rem;
      font-family: var(--font-sans);
      font-size: 0.9rem;
      font-weight: 600;
      cursor: pointer;
      transition: background-color 0.15s, transform 0.1s, border-color 0.15s;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 0.5rem;
    }
    
    .btn:hover {
      background: var(--secondary-hover);
      color: var(--text-active);
    }
    
    .btn:active {
      background: var(--secondary-active);
      transform: scale(0.98);
    }
    
    .btn:focus {
      outline: none;
      box-shadow: var(--focus-ring);
    }
    
    .btn-primary {
      background: var(--primary-color);
      border-color: var(--primary-color);
      color: #ffffff;
    }
    
    .btn-primary:hover {
      background: var(--primary-hover);
      border-color: var(--primary-hover);
    }
    
    .btn-primary:active {
      background: var(--primary-active);
      border-color: var(--primary-active);
    }
    
    /* 底部固定動作列 */
    .bottom-actions {
      position: fixed;
      bottom: 0;
      right: 0;
      left: 260px;
      background: var(--bg-main);
      border-top: 1px solid var(--border-color);
      padding: 1rem 2.5rem;
      display: flex;
      justify-content: flex-end;
      z-index: 99;
      transition: left 0.2s;
    }
    
    .actions-wrapper {
      display: flex;
      gap: 0.75rem;
      width: 100%;
      max-width: 52rem;
      justify-content: flex-end;
    }
    
    /* 狀態內卡片 */
    .status-card-inner {
      background: var(--bg-input);
      border: 1px solid var(--border-color);
      border-radius: 6px;
      padding: 1rem;
    }
    
    .status-dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      display: inline-block;
    }
    
    .status-dot.online {
      background: var(--success);
    }

    .status-dot.offline {
      background: var(--warning);
    }

    .status-dot.failed {
      background: var(--danger);
    }

    /* Loading overlay */
    .overlay {
      position: fixed;
      top: 0; left: 0; right: 0; bottom: 0;
      background: rgba(0, 0, 0, 0.6);
      display: none;
      align-items: center;
      justify-content: center;
      z-index: 9999;
      backdrop-filter: blur(2px);
    }

    .spinner {
      width: 2.5rem;
      height: 2.5rem;
      border: 3px solid rgba(255, 255, 255, 0.1);
      border-top-color: var(--primary-color);
      border-radius: 50%;
      animation: spin 1s linear infinite;
    }
    
    @keyframes spin {
      to { transform: rotate(360deg); }
    }

    /* Toast */
    .toast-container {
      position: fixed;
      bottom: 5.5rem;
      right: 2rem;
      z-index: 10000;
      display: flex;
      flex-direction: column;
      gap: 0.75rem;
    }
    
    .toast {
      background: var(--bg-card);
      color: var(--text-main);
      border: 1px solid var(--border-color);
      border-left: 4px solid var(--primary-color);
      border-radius: 6px;
      padding: 1rem 1.25rem;
      box-shadow: 0 10px 15px -3px rgba(0,0,0,0.3);
      display: flex;
      align-items: center;
      gap: 0.75rem;
      transform: translateX(120%);
      transition: transform 0.3s cubic-bezier(0.16, 1, 0.3, 1);
      min-width: 18rem;
    }
    
    .toast.show {
      transform: translateX(0);
    }
    
    .toast.success {
      border-left-color: var(--success);
    }
    
    .toast.error {
      border-left-color: var(--danger);
    }
    
    .hidden {
      display: none !important;
    }
    
    @media (max-width: 800px) {
      .sidebar {
        width: 70px;
        align-items: center;
      }
      .sidebar-title-group, .sidebar-subtitle, .sidebar-header span {
        display: none;
      }
      .sidebar-header {
        height: 7rem;
        padding: 0;
        justify-content: center;
      }
      .nav-item {
        padding: 0.75rem 0;
        justify-content: center;
        width: 48px;
      }
      .nav-item span:not(.nav-indicator) {
        display: none;
      }
      .bottom-actions {
        left: 70px;
      }
    }
  </style>
</head>
<body>
  <div id="appContainer" class="unauthorized">
    <!-- 1. 驗證卡片畫面 (未授權時) -->
    <div id="authWrapper" class="auth-wrapper">
      <div class="auth-card">
        <div class="auth-logo-area">
          <img class="fox-logo" src="/assets/icon.png" alt="FreeClaudeDesktop 圖標">
        </div>
        <h1 class="auth-title">FreeClaudeDesktop</h1>
        <p class="auth-subtitle">管理您的本機代理伺服器、模型路由與優化開關</p>
        <button class="btn btn-primary" id="loadBtn" style="margin-top: 1.5rem; width: 100%;">
          載入設定 ↵
        </button>
      </div>
    </div>

    <!-- 2. 主雙欄佈局 (授權成功後) -->
    <form id="settingsForm" style="display: contents;">
      <div id="mainLayout" class="main-layout hidden">
        <!-- 左側側邊欄 -->
        <aside class="sidebar">
          <div class="sidebar-header">
            <img class="fox-logo" src="/assets/icon.png" alt="FreeClaudeDesktop 圖標">
            <div class="sidebar-title-group">
              <span class="sidebar-title">FreeClaude<br>Desktop</span>
              <span class="sidebar-subtitle" data-i18n="sidebar_subtitle">設定</span>
            </div>
          </div>
          <nav class="sidebar-nav">
            <a href="#connection" class="nav-item active" data-tab="connection">
              <span class="nav-indicator"></span>
              <span data-i18n="nav_connection">連線設定</span>
            </a>
            <a href="#models" class="nav-item" data-tab="models">
              <span class="nav-indicator"></span>
              <span data-i18n="nav_models">模型與思考</span>
            </a>
            <a href="#extensions" class="nav-item" data-tab="extensions">
              <span class="nav-indicator"></span>
              <span data-i18n="nav_extensions">擴充與技能</span>
            </a>
            <a href="#optimization" class="nav-item" data-tab="optimization">
              <span class="nav-indicator"></span>
              <span data-i18n="nav_optimization">效能優化</span>
            </a>
          </nav>
          <div class="sidebar-footer">
            <div class="select-wrapper">
              <select id="language" aria-label="語言">
                <option value="zh-tw">繁體中文</option>
                <option value="en">English</option>
              </select>
            </div>
          </div>
        </aside>

        <!-- 右側主要區域 -->
        <main class="content-area">
          <header class="content-header">
            <div class="header-left">
              <h2 class="content-title" data-i18n="content_title">FreeClaude 控制台</h2>
              <p class="proxy-status"><span data-i18n="conn_local_proxy">本機 Proxy</span> : <span id="activePort">--</span></p>
            </div>
            <div class="header-right">
              <div class="theme-capsule">
                <button type="button" class="theme-btn" id="theme-system" data-theme="system" title="系統">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <rect x="2" y="3" width="20" height="14" rx="2" ry="2"/>
                    <line x1="8" y1="21" x2="16" y2="21"/>
                    <line x1="12" y1="17" x2="12" y2="21"/>
                  </svg>
                </button>
                <button type="button" class="theme-btn" id="theme-light" data-theme="light" title="亮色">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <circle cx="12" cy="12" r="5"/>
                    <line x1="12" y1="1" x2="12" y2="3"/>
                    <line x1="12" y1="21" x2="12" y2="23"/>
                    <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/>
                    <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/>
                    <line x1="1" y1="12" x2="3" y2="12"/>
                    <line x1="21" y1="12" x2="23" y2="12"/>
                    <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/>
                    <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
                  </svg>
                </button>
                <button type="button" class="theme-btn" id="theme-dark" data-theme="dark" title="暗色">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>
                  </svg>
                </button>
              </div>
            </div>
          </header>

          <div class="tab-content-container">
            <!-- 1. 連線設定分頁 -->
            <section id="tab-connection" class="tab-section">
              <div class="card">
                <div class="card-title" data-i18n="conn_title">基本連線設定</div>
                <div class="grid">
                  <div class="form-group">
                    <label for="apiProvider" data-i18n="conn_provider">API 供應商</label>
                    <div class="select-wrapper">
                      <select id="apiProvider">
                        <option value="custom">custom</option>
                        <option value="nvidia">NVIDIA NIM</option>
                        <option value="openrouter">OpenRouter</option>
                        <option value="gemini">Google Gemini</option>
                        <option value="deepseek">DeepSeek</option>
                        <option value="groq">Groq</option>
                        <option value="grok">xAI Grok</option>
                        <option value="zai">Z.ai</option>
                        <option value="kimi">Kimi (Moonshot AI)</option>
                        <option value="minimax">MiniMax</option>
                        <option value="qwen">Qwen（國際）</option>
                      </select>
                    </div>
                  </div>
                  <div class="form-group">
                    <label for="baseUrl" data-i18n="conn_api_url">API URL</label>
                    <input id="baseUrl" type="url" required placeholder="http://127.0.0.1:4000">
                  </div>
                  <div class="form-group">
                    <label for="apiKey"><span data-i18n="conn_api_key">API Key</span> <span id="keyStatus" style="font-size: 0.75rem; margin-left: 0.5rem;"></span></label>
                    <input id="apiKey" type="password" placeholder="••••••••••••••••" autocomplete="new-password">
                  </div>
                  <div class="form-group">
                    <label for="authScheme" data-i18n="conn_auth_scheme">驗證方式</label>
                    <div class="select-wrapper">
                      <select id="authScheme">
                        <option value="bearer">bearer</option>
                        <option value="x-api-key">x-api-key</option>
                      </select>
                    </div>
                  </div>
                </div>

                <div style="margin-top: 1.5rem;">
                  <div class="switch-container" style="opacity: 0.7;">
                    <div class="switch-label">
                      <span data-i18n="conn_custom_path_title">使用自訂 Claude.exe 路徑</span>
                      <span class="switch-desc" data-i18n="conn_custom_path_desc">本機 GUI 管理功能，Web 端僅供展示</span>
                    </div>
                    <label class="switch">
                      <input type="checkbox" id="useCustomClaudePath" disabled>
                      <span class="slider"></span>
                    </label>
                  </div>
                  <div class="form-group" style="margin-top: 0.75rem;">
                    <input type="text" id="customClaudePath" value="C:\Users\...\Claude.exe" disabled style="opacity: 0.5;">
                  </div>
                </div>

                <!-- 偵測到的 Claude Desktop 狀態卡片 -->
                <div class="status-card-inner" style="margin-top: 1.5rem;">
                  <div style="display: flex; align-items: center; gap: 0.75rem;">
                    <span id="detectedClaudeDot" class="status-dot offline"></span>
                    <span id="detectedClaudeStatus" style="font-weight: 600;" data-i18n="conn_detecting">偵測中...</span>
                  </div>
                  <div id="detectedClaudePath" style="font-size: 0.8rem; color: var(--text-muted); margin-top: 0.5rem; font-family: monospace; word-break: break-all;" data-i18n="conn_detecting">
                    偵測中...
                  </div>
                </div>
              </div>
            </section>

            <!-- 2. 模型與思考分頁 -->
            <section id="tab-models" class="tab-section hidden">
              <div class="card">
                <div class="card-title" data-i18n="models_title">模型別名與路由</div>
                <p class="section-desc" data-i18n="models_desc">配置 Claude Desktop 對應的核心別名模型，可手動輸入或從偵測到的上游模型中選擇。</p>
                
                <div class="grid">
                  <div class="form-group">
                    <label for="realModelSonnet" data-i18n="models_sonnet">Sonnet 模型別名</label>
                    <input id="realModelSonnet" type="text" placeholder="例如: claude-3-5-sonnet-latest" data-i18n-placeholder="placeholder_sonnet" list="modelSuggestions">
                  </div>
                  <div class="form-group">
                    <label for="realModelOpus" data-i18n="models_opus">Opus 模型別名</label>
                    <input id="realModelOpus" type="text" placeholder="例如: claude-3-opus-latest" data-i18n-placeholder="placeholder_opus" list="modelSuggestions">
                  </div>
                  <div class="form-group">
                    <label for="realModelHaiku" data-i18n="models_haiku">Haiku 模型別名</label>
                    <input id="realModelHaiku" type="text" placeholder="例如: claude-3-5-haiku-latest" data-i18n-placeholder="placeholder_haiku" list="modelSuggestions">
                  </div>
                  <div class="form-group">
                    <label for="realModel" data-i18n="models_fallback">預設保底模型</label>
                    <input id="realModel" type="text" placeholder="當找不到路由時使用" data-i18n-placeholder="placeholder_fallback" list="modelSuggestions">
                  </div>
                </div>
                <datalist id="modelSuggestions"></datalist>

                <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 1.5rem; margin-bottom: 0.5rem; flex-wrap: wrap; gap: 0.5rem;">
                  <h3 class="subsection-title" style="margin: 0;" data-i18n="models_discovered_title">已偵測上游模型清單</h3>
                  <button type="button" class="btn" id="fetchModelsBtn" style="padding: 0.4rem 0.8rem; font-size: 0.85rem; background-color: var(--primary-color);">
                    <span data-i18n="models_fetch_btn">抓取模型清單</span>
                  </button>
                </div>
                <p class="section-desc" data-i18n="models_discovered_desc">勾選「顯示」使其呈現在 Claude Desktop 列表中；「1M」啟用 100 萬上下文支援。</p>
                
                <div class="table-container">
                  <table>
                    <thead>
                      <tr>
                        <th data-i18n="models_table_name">模型名稱</th>
                        <th style="width: 5rem; text-align: center;" data-i18n="models_table_show">顯示</th>
                        <th style="width: 5rem; text-align: center;" data-i18n="models_table_1m">1M</th>
                        <th style="width: 10rem;" data-i18n="models_table_effort">Reasoning Effort</th>
                      </tr>
                    </thead>
                    <tbody id="modelsTableBody">
                      <tr>
                        <td colspan="4" style="text-align: center; color: var(--text-muted);">尚未載入任何模型</td>
                      </tr>
                    </tbody>
                  </table>
                </div>
              </div>
            </section>

            <!-- 3. 擴充與技能分頁 -->
            <section id="tab-extensions" class="tab-section hidden">
              <div class="card">
                <div class="card-title" data-i18n="ext_title">擴充與本地技能</div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableQuotaCheckMock').click()">
                    <span data-i18n="ext_quota_title">配額檢查攔截</span>
                    <span class="switch-desc" data-i18n="ext_quota_desc">攔截 max_tokens=1 且含有 "quota" 的測試請求</span>
                  </div>
                  <label class="switch" aria-label="配額檢查攔截">
                    <input type="checkbox" id="enableQuotaCheckMock">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enablePrefixDetection').click()">
                    <span data-i18n="ext_prefix_title">命令前綴快速檢測</span>
                    <span class="switch-desc" data-i18n="ext_prefix_desc">本地解析 shell 命令，避免不必要地呼叫 LLM</span>
                  </div>
                  <label class="switch" aria-label="命令前綴快速檢測">
                    <input type="checkbox" id="enablePrefixDetection">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableTitleGenerationSkip').click()">
                    <span data-i18n="ext_title_skip_title">跳過對話標題生成</span>
                    <span class="switch-desc" data-i18n="ext_title_skip_desc">直接回傳固定標題 "Conversation"，加速對話開啟</span>
                  </div>
                  <label class="switch" aria-label="跳過對話標題生成">
                    <input type="checkbox" id="enableTitleGenerationSkip">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableSuggestionModeSkip').click()">
                    <span data-i18n="ext_suggest_skip_title">跳過建議提問模式</span>
                    <span class="switch-desc" data-i18n="ext_suggest_skip_desc">直接回傳空建議，減少無用 API 請求</span>
                  </div>
                  <label class="switch" aria-label="跳過建議提問模式">
                    <input type="checkbox" id="enableSuggestionModeSkip">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableFilepathExtractionMock').click()">
                    <span data-i18n="ext_filepath_title">本機檔案路徑提取</span>
                    <span class="switch-desc" data-i18n="ext_filepath_desc">由命令輸出中進行本地路徑分析</span>
                  </div>
                  <label class="switch" aria-label="本機檔案路徑提取">
                    <input type="checkbox" id="enableFilepathExtractionMock">
                    <span class="slider"></span>
                  </label>
                </div>

                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableWebServerTools').click()">
                    <span data-i18n="ext_web_tools_title">Web 網頁存取工具</span>
                    <span class="switch-desc" data-i18n="ext_web_tools_desc">允許本地執行 web_search 與 web_fetch 抓取工具</span>
                  </div>
                  <label class="switch" aria-label="Web 網頁存取工具">
                    <input type="checkbox" id="enableWebServerTools">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div id="webToolsSettings" class="hidden" style="margin-left: 1.5rem; padding-left: 1rem; border-left: 2px solid var(--border-color); margin-top: 0.5rem; margin-bottom: 0.75rem;">
                  <div class="form-group">
                    <label for="webFetchAllowedSchemes" data-i18n="ext_web_fetch_schemes">Web Fetch 允許 URL Schemes (以逗號分隔)</label>
                    <input id="webFetchAllowedSchemes" type="text" placeholder="http,https">
                  </div>
                  <div class="switch-container" style="background: none; border: none; padding: 0.5rem 0;">
                    <div class="switch-label" onclick="document.getElementById('webFetchAllowPrivateNetworks').click()">
                      <span data-i18n="ext_web_fetch_private">允許 web_fetch 存取私有網路 (Private Networks)</span>
                    </div>
                    <label class="switch" aria-label="允許 web_fetch 存取私有網路">
                      <input type="checkbox" id="webFetchAllowPrivateNetworks">
                      <span class="slider"></span>
                    </label>
                  </div>
                </div>
              </div>
            </section>

            <!-- 4. 效能優化分頁 -->
            <section id="tab-optimization" class="tab-section hidden">
              <div class="card">
                <div class="card-title" data-i18n="opt_title">效能優化設定</div>
                
                <div class="grid">
                  <div class="form-group">
                    <label for="transportType" data-i18n="opt_transport">傳輸協定</label>
                    <div class="select-wrapper">
                      <select id="transportType">
                        <option value="openai_chat" data-i18n="opt_transport_openai">OpenAI Chat 格式轉換</option>
                        <option value="anthropic_messages" data-i18n="opt_transport_anthropic">原生 Anthropic passthrough</option>
                      </select>
                    </div>
                  </div>
                  
                  <div class="form-group">
                    <label for="reasoningReplayMode" data-i18n="opt_thinking">思考模式</label>
                    <div class="select-wrapper">
                      <select id="reasoningReplayMode">
                        <option value="separate" data-i18n="opt_thinking_separate">Separate（Claude 原生 thinking 區塊）</option>
                        <option value="inline" data-i18n="opt_thinking_inline">Inline（包裝在 &lt;antThinking&gt; 標籤）</option>
                      </select>
                    </div>
                  </div>
                </div>

                <select id="themeMode" class="hidden">
                  <option value="light">明亮 (Light)</option>
                  <option value="dark">深色 (Dark)</option>
                  <option value="system">系統 (System)</option>
                </select>


              </div>
            </section>
          </div>

          <!-- 3. 底部固定動作列 -->
          <footer class="bottom-actions">
            <div class="actions-wrapper">
              <button type="button" class="btn btn-secondary" id="resetMirrorBtn" data-i18n="btn_reset_mirror">重置鏡像 Profile</button>
              <button type="button" class="btn btn-secondary" id="syncOfficialBtn" data-i18n="btn_sync_original">從原版同步</button>
              <button type="button" class="btn btn-secondary" id="saveOnlyBtn" data-i18n="btn_save_only">僅儲存</button>
              <button type="submit" class="btn btn-primary" id="saveAndLaunchBtn" data-i18n="btn_save_launch">儲存並啟動 ↵</button>
            </div>
          </footer>
        </main>
      </div>
    </form>
  </div>

  <div class="overlay" id="loadingOverlay">
    <div class="spinner"></div>
  </div>

  <div class="toast-container" id="toastContainer"></div>

  <script>
    const $ = id => document.getElementById(id);
    let loadedSettings = null;
    let launchAfterSave = false;

    const translations = {
      'zh-tw': {
        'nav_connection': '連線設定',
        'nav_models': '模型與思考',
        'nav_extensions': '擴充與技能',
        'nav_optimization': '效能優化',
        'conn_title': '基本連線設定',
        'conn_provider': 'API 供應商',
        'conn_api_url': 'API URL',
        'conn_api_key': 'API Key',
        'conn_auth_scheme': '驗證方式',
        'conn_custom_path_title': '使用自訂 Claude.exe 路徑',
        'conn_custom_path_desc': '本機 GUI 管理功能，Web 端僅供展示',
        'conn_detected_claude': '已偵測 Claude Desktop',
        'conn_detecting': '偵測中...',
        'models_title': '模型別名與路由',
        'models_desc': '配置 Claude Desktop 對應的核心別名模型，可手動輸入或從偵測到的上游模型中選擇。',
        'models_sonnet': 'Sonnet 模型別名',
        'models_opus': 'Opus 模型別名',
        'models_haiku': 'Haiku 模型別名',
        'models_fallback': '預設保底模型',
        'models_discovered_title': '已偵測上游模型清單',
        'models_fetch_btn': '抓取模型清單',
        'models_discovered_desc': '勾選「顯示」使其呈現在 Claude Desktop 列表中；「1M」啟用 100 萬上下文支援。',
        'models_table_name': '模型名稱',
        'models_table_show': '顯示',
        'models_table_1m': '1M',
        'models_table_effort': '推理強度',
        'ext_title': '擴充與本地技能',
        'ext_quota_title': '配額檢查攔截',
        'ext_quota_desc': '攔截 max_tokens=1 且含有 "quota" 的測試請求',
        'ext_prefix_title': '命令前綴快速檢測',
        'ext_prefix_desc': '本地解析 shell 命令，避免不必要地呼叫 LLM',
        'ext_title_skip_title': '跳過對話標題生成',
        'ext_title_skip_desc': '直接回傳固定標題 "Conversation"，加速對話開啟',
        'ext_suggest_skip_title': '跳過建議提問模式',
        'ext_suggest_skip_desc': '直接回傳空建議，減少無用 API 請求',
        'ext_filepath_title': '本機檔案路徑提取',
        'ext_filepath_desc': '由命令輸出中進行本地路徑分析',
        'ext_web_tools_title': 'Web 網頁存取工具',
        'ext_web_tools_desc': '允許本地執行 web_search 與 web_fetch 抓取工具',
        'ext_web_fetch_schemes': 'Web Fetch 允許 URL Schemes (以逗號分隔)',
        'ext_web_fetch_private': '允許 web_fetch 存取私有網路 (Private Networks)',
        'opt_title': '效能優化設定',
        'opt_transport': '傳輸協定',
        'opt_transport_openai': 'OpenAI Chat 格式轉換',
        'opt_transport_anthropic': '原生 Anthropic passthrough',
        'opt_thinking': '思考模式',
        'opt_thinking_inline': 'Inline（包裝在 <antThinking> 標籤）',
        'opt_thinking_separate': 'Separate（Claude 原生 thinking 區塊）',
        'btn_launch_claude': '啟動 Claude Desktop',
        'btn_reset_mirror': '重置鏡像 Profile',
        'btn_sync_original': '從原版同步',
        'btn_save_only': '僅儲存',
        'btn_save_launch': '儲存並啟動 ↵',
        'toast_save_success': '設定已成功儲存！',
        'toast_save_failed': '儲存失敗: ',
        'toast_load_success': '設定載入成功！',
        'toast_load_failed': '載入失敗: ',
        'toast_launch_success': 'Claude Desktop 啟動成功，路徑: ',
        'toast_launch_failed': '儲存成功，但 Claude 啟動失敗: ',
        'toast_fetch_success': '模型清單抓取成功！',
        'toast_fetch_failed': '抓取失敗: ',
        'confirm_sync': '⚠ 確定要從官方原版 Claude Desktop 同步配置？',
        'confirm_reset': '⚠ 確定要重置鏡像 Profile 目錄？原版目錄完全不受影響。',
        'conn_detecting': '偵測中...',
        'detected_online': '已偵測 Claude Desktop',
        'detected_offline': '未偵測到安裝路徑，將使用預設路徑',
        'detected_failed': '無法偵測安裝路徑',
        'detected_offline_title': '未偵測到 Claude Desktop',
        'detected_failed_title': '無法偵測 Claude Desktop',
        'sidebar_subtitle': '設定',
        'content_title': 'FreeClaude 控制台',
        'close_title': '設定已成功儲存並啟動！',
        'close_desc': 'Claude Desktop 已順利啟動，本網頁已完成使命。',
        'close_fallback': '如果此分頁沒有自動關閉，您可以手動將其關閉。',
        'conn_local_proxy': '本機 Proxy',
        'placeholder_sonnet': '例如: claude-3-5-sonnet-latest',
        'placeholder_opus': '例如: claude-3-opus-latest',
        'placeholder_haiku': '例如: claude-3-5-haiku-latest',
        'placeholder_fallback': '當找不到路由時使用',
        'apiKey_saved': '•••••••••••••••• (已儲存)',
        'apiKey_not_set': '尚未設定 API Key',
        'keyStatus_saved': '✅ 已儲存金鑰',
        'keyStatus_not_set': '❌ 未儲存金鑰'
      },
      'en': {
        'nav_connection': 'Connection',
        'nav_models': 'Models & Thinking',
        'nav_extensions': 'Extensions & Skills',
        'nav_optimization': 'Optimization',
        'conn_title': 'Connection Settings',
        'conn_provider': 'API Provider',
        'conn_api_url': 'API URL',
        'conn_api_key': 'API Key',
        'conn_auth_scheme': 'Auth Scheme',
        'conn_custom_path_title': 'Use custom Claude.exe path',
        'conn_custom_path_desc': 'Local GUI only, Web for display',
        'conn_detected_claude': 'Claude Desktop Detected',
        'conn_detecting': 'Detecting...',
        'models_title': 'Model Aliases & Routing',
        'models_desc': 'Configure core model aliases for Claude Desktop. Select or type custom ones.',
        'models_sonnet': 'Sonnet Model Alias',
        'models_opus': 'Opus Model Alias',
        'models_haiku': 'Haiku Model Alias',
        'models_fallback': 'Fallback Model',
        'models_discovered_title': 'Discovered Models',
        'models_fetch_btn': 'Fetch Models',
        'models_discovered_desc': 'Check "Show" to present in Claude; "1M" to enable 1M token context support.',
        'models_table_name': 'Model Name',
        'models_table_show': 'Show',
        'models_table_1m': '1M',
        'models_table_effort': 'Reasoning Effort',
        'ext_title': 'Extensions & Skills',
        'ext_quota_title': 'Quota Mock',
        'ext_quota_desc': 'Intercept max_tokens=1 and quota checks',
        'ext_prefix_title': 'Prefix Detection',
        'ext_prefix_desc': 'Parse shell prefixes locally to bypass LLM',
        'ext_title_skip_title': 'Skip Title Generation',
        'ext_title_skip_desc': 'Return static title "Conversation" to speed up',
        'ext_suggest_skip_title': 'Skip Suggestion Mode',
        'ext_suggest_skip_desc': 'Return empty suggestions to reduce API usage',
        'ext_filepath_title': 'Filepath Extraction',
        'ext_filepath_desc': 'Extract filepaths locally from command output',
        'ext_web_tools_title': 'Web Access Tools',
        'ext_web_tools_desc': 'Enable local execution of web_search and web_fetch',
        'ext_web_fetch_schemes': 'Allowed URL Schemes (comma separated)',
        'ext_web_fetch_private': 'Allow web_fetch to access private networks',
        'opt_title': 'Optimization Settings',
        'opt_transport': 'Transport Protocol',
        'opt_transport_openai': 'OpenAI Chat Format Conversion',
        'opt_transport_anthropic': 'Native Anthropic Passthrough',
        'opt_thinking': 'Thinking Mode',
        'opt_thinking_inline': 'Inline (wrapped in <antThinking> tags)',
        'opt_thinking_separate': 'Separate (native Claude thinking blocks)',
        'btn_launch_claude': 'Launch Claude',
        'btn_reset_mirror': 'Reset Mirror',
        'btn_sync_original': 'Sync from Official',
        'btn_save_only': 'Save Only',
        'btn_save_launch': 'Save & Launch ↵',
        'toast_save_success': 'Settings saved successfully!',
        'toast_save_failed': 'Save failed: ',
        'toast_load_success': 'Settings loaded successfully!',
        'toast_load_failed': 'Load failed: ',
        'toast_launch_success': 'Claude Desktop launched, path: ',
        'toast_launch_failed': 'Saved, but failed to launch Claude: ',
        'toast_fetch_success': 'Model list fetched successfully!',
        'toast_fetch_failed': 'Fetch failed: ',
        'confirm_sync': '⚠ Are you sure you want to sync settings from original Claude?',
        'confirm_reset': '⚠ Are you sure you want to reset mirror Profile? Original profile will not be affected.',
        'detected_online': 'Claude Desktop Detected',
        'detected_offline': 'Claude Desktop not detected, using default path',
        'detected_failed': 'Failed to detect Claude Desktop path',
        'detected_offline_title': 'Claude Desktop Not Detected',
        'detected_failed_title': 'Claude Desktop Detection Failed',
        'sidebar_subtitle': 'Console',
        'content_title': 'FreeClaude Console',
        'close_title': 'Settings Saved & Launched!',
        'close_desc': 'Claude Desktop has been launched successfully.',
        'close_fallback': 'If this tab did not close automatically, you can close it manually.',
        'conn_local_proxy': 'Local Proxy',
        'placeholder_sonnet': 'e.g. claude-3-5-sonnet-latest',
        'placeholder_opus': 'e.g. claude-3-opus-latest',
        'placeholder_haiku': 'e.g. claude-3-5-haiku-latest',
        'placeholder_fallback': 'Used when no route matches',
        'apiKey_saved': '•••••••••••••••• (Saved)',
        'apiKey_not_set': 'API Key not set',
        'keyStatus_saved': '✅ Saved',
        'keyStatus_not_set': '❌ Not set'
      }
    };

    function applyLanguage(lang) {
      const dict = translations[lang] || translations['zh-tw'];
      document.querySelectorAll('[data-i18n]').forEach(el => {
        const key = el.dataset.i18n;
        if (dict[key]) {
          el.textContent = dict[key];
        }
      });
      document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
        const key = el.dataset.i18nPlaceholder;
        if (dict[key]) {
          el.placeholder = dict[key];
        }
      });
      if (loadedSettings) {
        $('apiKey').placeholder = loadedSettings.hasApiKey ? dict['apiKey_saved'] : dict['apiKey_not_set'];
        $('keyStatus').textContent = loadedSettings.hasApiKey ? dict['keyStatus_saved'] : dict['keyStatus_not_set'];
      }
      document.title = dict['title'] || 'FreeClaude Admin Dashboard';
      document.documentElement.lang = lang === 'en' ? 'en' : 'zh-TW';
    }

    function t(key, param = '') {
      const lang = $('language').value || 'zh-tw';
      const dict = translations[lang] || translations['zh-tw'];
      let text = dict[key] || key;
      if (param) {
        text += param;
      }
      return text;
    }

    const providerPresets = {
      nvidia: { baseUrl: 'https://integrate.api.nvidia.com/v1', authScheme: 'bearer' },
      openrouter: { baseUrl: 'https://openrouter.ai/api/v1', authScheme: 'bearer' },
      gemini: { baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', authScheme: 'bearer' },
      deepseek: { baseUrl: 'https://api.deepseek.com', authScheme: 'bearer' },
      groq: { baseUrl: 'https://api.groq.com/openai/v1', authScheme: 'bearer' },
      grok: { baseUrl: 'https://api.x.ai/v1', authScheme: 'bearer' },
      zai: { baseUrl: 'https://api.z.ai/api/paas/v4', authScheme: 'bearer' },
      kimi: { baseUrl: 'https://api.moonshot.ai/v1', authScheme: 'bearer' },
      minimax: { baseUrl: 'https://api.minimax.io/v1', authScheme: 'bearer' },
      qwen: { baseUrl: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1', authScheme: 'bearer' }
    };

    function selectProviderForBaseUrl(baseUrl) {
      const normalized = (baseUrl || '').replace(/\/$/, '');
      const provider = Object.entries(providerPresets)
        .find(([, preset]) => preset.baseUrl === normalized)?.[0] || 'custom';
      $('apiProvider').value = provider;
    }

    $('apiProvider').addEventListener('change', () => {
      const preset = providerPresets[$('apiProvider').value];
      if (!preset) return;
      $('baseUrl').value = preset.baseUrl;
      $('authScheme').value = preset.authScheme;
    });

    // Helper functions for Toast
    function showToast(message, type = 'success') {
      const container = $('toastContainer');
      const toast = document.createElement('div');
      toast.className = `toast ${type}`;
      
      let icon = '';
      if (type === 'success') {
        icon = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>`;
      } else {
        icon = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>`;
      }
      
      toast.innerHTML = `${icon}<span>${message}</span>`;
      container.appendChild(toast);
      
      setTimeout(() => toast.classList.add('show'), 10);
      
      setTimeout(() => {
        toast.classList.remove('show');
        setTimeout(() => toast.remove(), 300);
      }, 3500);
    }

    function showLoading(show) {
      $('loadingOverlay').style.display = show ? 'flex' : 'none';
    }

    const headers = () => ({});

    async function request(path, options = {}) {
      const r = await fetch(path, {
        ...options,
        headers: {
          ...headers(),
          ...(options.headers || {})
        }
      });
      const b = await r.json();
      if (!r.ok) throw new Error(b.error || r.statusText);
      return b;
    }

    // Toggle Web Fetch options when Web Tools Intercept checked
    $('enableWebServerTools').addEventListener('change', (e) => {
      if (e.target.checked) {
        $('webToolsSettings').classList.remove('hidden');
      } else {
        $('webToolsSettings').classList.add('hidden');
      }
    });

    // Theme switching logic
    function applyTheme(theme) {
      const root = document.documentElement;
      if (theme === 'system') {
        const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        root.setAttribute('data-theme', isDark ? 'dark' : 'light');
      } else {
        root.setAttribute('data-theme', theme);
      }
      
      document.querySelectorAll('.theme-btn').forEach(btn => {
        if (btn.dataset.theme === theme) {
          btn.classList.add('active');
        } else {
          btn.classList.remove('active');
        }
      });
      
      localStorage.setItem('theme', theme);
      if ($('themeMode')) {
        $('themeMode').value = theme;
      }
    }

    document.querySelectorAll('.theme-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        applyTheme(btn.dataset.theme);
      });
    });

    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      const currentTheme = localStorage.getItem('theme') || 'system';
      if (currentTheme === 'system') {
        applyTheme('system');
      }
    });

    // Sidebar navigation logic
    document.querySelectorAll('.nav-item').forEach(item => {
      item.addEventListener('click', (e) => {
        e.preventDefault();
        const tabId = item.dataset.tab;
        
        document.querySelectorAll('.nav-item').forEach(i => i.classList.remove('active'));
        item.classList.add('active');
        
        document.querySelectorAll('.tab-section').forEach(sec => {
          if (sec.id === `tab-${tabId}`) {
            sec.classList.remove('hidden');
          } else {
            sec.classList.add('hidden');
          }
        });
      });
    });

    // Load Settings
    async function load() {
      showLoading(true);
      $('detectedClaudeDot').className = 'status-dot offline';
      $('detectedClaudeStatus').setAttribute('data-i18n', 'conn_detecting');
      $('detectedClaudeStatus').textContent = t('conn_detecting');
      $('detectedClaudePath').setAttribute('data-i18n', 'conn_detecting');
      $('detectedClaudePath').textContent = t('conn_detecting');
      try {
        const [settings, status] = await Promise.all([
          request('/settings'),
          request('/status')
        ]);
        
        loadedSettings = settings;
        
        $('authWrapper').classList.add('hidden');
        $('mainLayout').classList.remove('hidden');
        $('appContainer').classList.remove('unauthorized');
        
        $('activePort').textContent = '127.0.0.1 : ' + (status.proxy.port || '3000');
        
        $('baseUrl').value = settings.baseUrl || '';
        $('authScheme').value = settings.authScheme || 'bearer';
        selectProviderForBaseUrl(settings.baseUrl);
        $('apiKey').placeholder = settings.hasApiKey ? t('apiKey_saved') : t('apiKey_not_set');
        $('keyStatus').textContent = settings.hasApiKey ? t('keyStatus_saved') : t('keyStatus_not_set');
        $('keyStatus').style.color = settings.hasApiKey ? '#10b981' : '#f59e0b';
        
        $('realModelSonnet').value = settings.realModelSonnet || '';
        $('realModelOpus').value = settings.realModelOpus || '';
        $('realModelHaiku').value = settings.realModelHaiku || '';
        $('realModel').value = settings.realModel || '';
        
        const dl = $('modelSuggestions');
        dl.innerHTML = '';
        if (settings.discoveredModels) {
          settings.discoveredModels.forEach(m => {
            const opt = document.createElement('option');
            opt.value = m;
            dl.appendChild(opt);
          });
        }
        
        $('transportType').value = settings.transportType || 'openai_chat';
        const reasoningReplayMode = ['inline', 'separate'].includes(settings.reasoningReplayMode)
          ? settings.reasoningReplayMode
          : 'separate';
        $('reasoningReplayMode').value = reasoningReplayMode;
        
        $('enableQuotaCheckMock').checked = settings.enableQuotaCheckMock !== false;
        $('enablePrefixDetection').checked = settings.enablePrefixDetection !== false;
        $('enableTitleGenerationSkip').checked = settings.enableTitleGenerationSkip !== false;
        $('enableSuggestionModeSkip').checked = settings.enableSuggestionModeSkip !== false;
        $('enableFilepathExtractionMock').checked = settings.enableFilepathExtractionMock !== false;
        
        $('enableWebServerTools').checked = settings.enableWebServerTools === true;
        if (settings.enableWebServerTools) {
          $('webToolsSettings').classList.remove('hidden');
        } else {
          $('webToolsSettings').classList.add('hidden');
        }
        
        $('webFetchAllowedSchemes').value = settings.webFetchAllowedSchemes || 'http,https';
        $('webFetchAllowPrivateNetworks').checked = settings.webFetchAllowPrivateNetworks === true;
        
        const theme = localStorage.getItem('theme') || settings.themeMode || 'system';
        localStorage.setItem('theme', theme);
        applyTheme(theme);
        
        $('language').value = settings.language || 'zh-tw';
        applyLanguage($('language').value);
        renderModelsTable(settings);
        
        // Detect Claude Path via RPC
        try {
          const detectRes = await request('/rpc', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ method: 'DetectClaude' })
          });
          if (detectRes && detectRes.result && detectRes.result.path) {
            $('detectedClaudeDot').className = 'status-dot online';
            $('detectedClaudeStatus').setAttribute('data-i18n', 'detected_online');
            $('detectedClaudeStatus').textContent = t('detected_online');
            $('detectedClaudePath').removeAttribute('data-i18n');
            $('detectedClaudePath').textContent = detectRes.result.path;
          } else {
            $('detectedClaudeDot').className = 'status-dot offline';
            $('detectedClaudeStatus').setAttribute('data-i18n', 'detected_offline_title');
            $('detectedClaudeStatus').textContent = t('detected_offline_title');
            $('detectedClaudePath').setAttribute('data-i18n', 'detected_offline');
            $('detectedClaudePath').textContent = t('detected_offline');
          }
        } catch (err) {
          $('detectedClaudeDot').className = 'status-dot failed';
          $('detectedClaudeStatus').setAttribute('data-i18n', 'detected_failed_title');
          $('detectedClaudeStatus').textContent = t('detected_failed_title');
          $('detectedClaudePath').setAttribute('data-i18n', 'detected_failed');
          $('detectedClaudePath').textContent = t('detected_failed');
        }
        
        showToast(t('toast_load_success'));
      } catch (e) {
        showToast(t('toast_load_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    }

    function renderModelsTable(settings) {
      const tbody = $('modelsTableBody');
      tbody.innerHTML = '';
      
      const models = settings.discoveredModels || [];
      const lang = $('language').value || 'zh-tw';
      
      if (models.length === 0) {
        tbody.innerHTML = `<tr><td colspan="4" style="text-align: center; color: var(--text-muted);">${t('no_models_yet')}</td></tr>`;
        return;
      }
      
      const optDefault = lang === 'en' ? 'Default' : '預設';
      const optNone = lang === 'en' ? 'None' : '無';
      const optHigh = lang === 'en' ? 'High' : '高';
      const optMax = lang === 'en' ? 'Max' : '最高';

      models.forEach(model => {
        const tr = document.createElement('tr');
        
        const isVisible = settings.modelVisibilityOverrides && settings.modelVisibilityOverrides[model] !== false;
        const is1m = settings.model1mOverrides && settings.model1mOverrides[model] === true;
        const effort = (settings.modelReasoningOverrides && settings.modelReasoningOverrides[model]) || '';
        
        tr.innerHTML = `
          <td style="font-family: monospace; word-break: break-all;">${model}</td>
          <td style="text-align: center;">
            <input type="checkbox" class="model-visibility" data-model="${model}" ${isVisible ? 'checked' : ''} aria-label="${model} 顯示狀態">
          </td>
          <td style="text-align: center;">
            <input type="checkbox" class="model-1m" data-model="${model}" ${is1m ? 'checked' : ''} aria-label="${model} 1M Context 支援">
          </td>
          <td>
            <div class="select-wrapper">
              <select class="model-effort" data-model="${model}" aria-label="${model} 思考上限設定">
                <option value="" ${effort === '' ? 'selected' : ''}>${optDefault}</option>
                <option value="none" ${effort === 'none' ? 'selected' : ''}>${optNone}</option>
                <option value="high" ${effort === 'high' ? 'selected' : ''}>${optHigh}</option>
                <option value="max" ${effort === 'max' ? 'selected' : ''}>${optMax}</option>
              </select>
            </div>
          </td>
        `;
        tbody.appendChild(tr);
      });
    }

    $('loadBtn').onclick = load;
    // Save Logic
    $('saveAndLaunchBtn').onclick = () => {
      launchAfterSave = true;
    };
    $('saveOnlyBtn').onclick = () => {
      launchAfterSave = false;
      $('settingsForm').requestSubmit();
    };

    $('settingsForm').onsubmit = async (e) => {
      e.preventDefault();
      
      showLoading(true);
      try {
        const modelVisibilityOverrides = {};
        document.querySelectorAll('.model-visibility').forEach(el => {
          modelVisibilityOverrides[el.dataset.model] = el.checked;
        });
        
        const model1mOverrides = {};
        document.querySelectorAll('.model-1m').forEach(el => {
          model1mOverrides[el.dataset.model] = el.checked;
        });
        
        const modelReasoningOverrides = {};
        document.querySelectorAll('.model-effort').forEach(el => {
          modelReasoningOverrides[el.dataset.model] = el.value;
        });

        const payload = {
          baseUrl: $('baseUrl').value.trim(),
          authScheme: $('authScheme').value,
          apiKey: $('apiKey').value.trim() || null,
          
          realModelSonnet: $('realModelSonnet').value.trim() || null,
          realModelOpus: $('realModelOpus').value.trim() || null,
          realModelHaiku: $('realModelHaiku').value.trim() || null,
          realModel: $('realModel').value.trim() || null,
          
          modelVisibilityOverrides,
          model1mOverrides,
          modelReasoningOverrides,
          
          transportType: $('transportType').value,
          reasoningReplayMode: $('reasoningReplayMode').value,
          
          enableQuotaCheckMock: $('enableQuotaCheckMock').checked,
          enablePrefixDetection: $('enablePrefixDetection').checked,
          enableTitleGenerationSkip: $('enableTitleGenerationSkip').checked,
          enableSuggestionModeSkip: $('enableSuggestionModeSkip').checked,
          enableFilepathExtractionMock: $('enableFilepathExtractionMock').checked,
          
          enableWebServerTools: $('enableWebServerTools').checked,
          webFetchAllowedSchemes: $('webFetchAllowedSchemes').value.trim(),
          webFetchAllowPrivateNetworks: $('webFetchAllowPrivateNetworks').checked,
          
          themeMode: localStorage.getItem('theme') || 'system',
          language: $('language').value
        };

        await request('/settings', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload)
        });

        $('apiKey').value = '';
        showToast(t('toast_save_success'));
        
        if (launchAfterSave) {
          try {
            const launchRes = await request('/rpc', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ method: 'LaunchClaude' })
            });
            showToast(t('toast_launch_success') + launchRes.result.path);
            setTimeout(() => {
              window.close();
              document.body.innerHTML = `
                <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100vh; font-family: sans-serif; background: #1e1e1e; color: #fff; padding: 2rem; text-align: center;">
                  <div style="font-size: 4rem; margin-bottom: 1.5rem;">🚀</div>
                  <h1 style="font-size: 1.75rem; font-weight: 700; margin-bottom: 0.5rem;">${t('close_title')}</h1>
                  <p style="color: #aaa; margin-bottom: 2rem;">${t('close_desc')}</p>
                  <p style="font-size: 0.9rem; color: #888;">${t('close_fallback')}</p>
                </div>
              `;
            }, 1000);
            return;
          } catch (launchErr) {
            showToast(t('toast_launch_failed') + launchErr.message, 'error');
          }
        }
        
        await load();
      } catch (e) {
        showToast(t('toast_save_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    // RPC Actions

    $('resetMirrorBtn').onclick = async () => {
      if (!confirm(t('confirm_reset'))) return;
      showLoading(true);
      try {
        await request('/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'ResetMirrorProfile' })
        });
        showToast(t('toast_save_success'));
        await load();
      } catch(e) {
        showToast(t('toast_save_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('syncOfficialBtn').onclick = async () => {
      if (!confirm(t('confirm_sync'))) return;
      showLoading(true);
      try {
        await request('/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'SyncFromOfficial' })
        });
        showToast(t('toast_save_success'));
        await load();
      } catch(e) {
        showToast(t('toast_save_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('fetchModelsBtn').onclick = async () => {
      showLoading(true);
      try {
        await request('/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'FetchModels' })
        });
        showToast(t('toast_fetch_success'));
        await load();
      } catch (e) {
        showToast(t('toast_fetch_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('language').onchange = () => {
      const val = $('language').value;
      applyLanguage(val);
      if (loadedSettings) {
        renderModelsTable(loadedSettings);
      }
    };

    // Theme initialization
    (function() {
      const savedTheme = localStorage.getItem('theme') || 'system';
      applyTheme(savedTheme);
      load();
    })();
  </script>
</body>
</html>"##,
    )
}

pub async fn handle_admin_settings(headers: HeaderMap) -> impl IntoResponse {
    match load_authorized_settings(&headers).await {
        Ok(settings) => (StatusCode::OK, Json(to_public_config(&settings))).into_response(),
        Err(response) => response.into_response(),
    }
}

pub async fn update_admin_settings(
    headers: HeaderMap,
    Json(input): Json<AdminSettingsUpdate>,
) -> impl IntoResponse {
    let mut settings = match load_authorized_settings(&headers).await {
        Ok(settings) => settings,
        Err(response) => return response.into_response(),
    };
    match apply_settings_update(&mut settings, input) {
        Ok(settings) => (StatusCode::OK, Json(settings)).into_response(),
        Err(response) => response.into_response(),
    }
}

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

pub async fn handle_companion_websocket(websocket: WebSocketUpgrade) -> impl IntoResponse {
    websocket.on_upgrade(handle_companion_session)
}

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
    async fn healthz_returns_ok_status() {
        assert_eq!(handle_healthz().await.0, json!({ "status": "ok" }));
    }

    #[test]
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
    fn companion_request_requires_request_id_only() {
        assert!(
            serde_json::from_str::<CompanionRequest>(r#"{"requestId":"1","method":"GetStatus"}"#)
                .is_ok()
        );
        assert!(serde_json::from_str::<CompanionRequest>(r#"{"method":"GetStatus"}"#).is_err());
    }
}

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

    // OpenAI 相容的非串流請求以 async-openai 的 BYOT API 送出，保留
    // LiteLLM 等 gateway 的 reasoning、影像與自訂欄位，不經型別轉換遺失。
    // 原生 Anthropic transport 與串流仍使用其專用路徑，因為前者不是 OpenAI
    // SSE、後者需要保留原始事件供既有 Anthropic SSE adapter 逐段轉換。
    if is_openai_format && !is_stream && !is_anthropic_native {
        let request: Value = match serde_json::from_str(&proxy_body) {
            Ok(value) => value,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": error.to_string() })),
                )
                    .into_response();
            }
        };
        let client = match AsyncOpenAiGatewayFactory.gateway_client(&settings) {
            Ok(client) => client,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": error.to_string() })),
                )
                    .into_response();
            }
        };
        let response: Value = match client.chat().create_byot(request).await {
            Ok(response) => response,
            Err(error) => {
                tracing::error!("<- async-openai 上游請求失敗: {error}");
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": error.to_string() })),
                )
                    .into_response();
            }
        };
        let response_text = match serde_json::to_string(&response) {
            Ok(text) => text,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
                    .into_response();
            }
        };
        return match openai_to_anthropic_response(&response_text, &req_model) {
            Ok(response) => (StatusCode::OK, Json(response)).into_response(),
            Err(error) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response(),
        };
    }

    // 5. Build Upstream request
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
