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
    http::{HeaderMap, StatusCode},
    response::Html,
    response::IntoResponse,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime};
use url::Url;

const MAX_UPSTREAM_ERROR_BYTES: usize = 64 * 1024;

use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use std::sync::Arc;
use std::collections::HashMap;

struct ActiveCompanion {
    tx: mpsc::UnboundedSender<ProxyToCompanionMessage>,
}

struct ProxyToCompanionMessage {
    request_id: String,
    payload: String,
    response_tx: oneshot::Sender<Result<Value, String>>,
}

static ACTIVE_COMPANION: tokio::sync::Mutex<Option<ActiveCompanion>> = tokio::sync::Mutex::const_new(None);


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
    headers: &HeaderMap,
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

    let authorization = headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok());
    let x_api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    if !super::is_authorized_proxy_request(authorization, x_api_key, &settings.proxy_auth_token) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized" })),
        ));
    }

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
        let scheme = match auth_scheme {
            "auto" => {
                if url::Url::parse(target_url)
                    .map_err(|error| crate::AppError::InvalidConfig(error.to_string()))?
                    .host_str()
                    == Some("api.anthropic.com")
                {
                    "x-api-key"
                } else {
                    "bearer"
                }
            }
            "x-api-key" | "bearer" | "sso" => auth_scheme,
            _ => {
                return Err(crate::AppError::InvalidConfig(
                    "不支援的 Auth Scheme".to_string(),
                ));
            }
        };
        if scheme == "x-api-key" {
            Some("x-api-key")
        } else {
            Some("authorization")
        }
    } else {
        None
    };

    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if let Some(skip) = skip_header {
            if lower == skip {
                continue;
            }
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

pub async fn handle_admin_page() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html lang="zh-TW">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
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
      width: 64px;
      height: 64px;
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
      padding: 2rem 0;
      z-index: 100;
    }
    
    .sidebar-header {
      padding: 0 1.5rem 2rem 1.5rem;
      display: flex;
      align-items: center;
      gap: 0.75rem;
      border-bottom: 1px solid var(--border-color);
      margin-bottom: 1.5rem;
    }
    
    .sidebar-header .fox-logo {
      width: 32px;
      height: 32px;
    }
    
    .sidebar-title-group {
      display: flex;
      flex-direction: column;
    }
    
    .sidebar-title {
      font-size: 1.05rem;
      font-weight: 700;
      color: var(--text-active);
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
      padding: 2rem 2.5rem 1.5rem 2.5rem;
      display: flex;
      justify-content: space-between;
      align-items: center;
      border-bottom: 1px solid var(--border-color);
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
    
    .dot-online {
      width: 8px;
      height: 8px;
      background: var(--success);
      border-radius: 50%;
      display: inline-block;
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
        padding: 0 0 1.5rem 0;
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
          <svg class="fox-logo" viewBox="0 0 100 100">
            <path d="M 50 10 L 20 40 L 30 75 L 50 90 L 70 75 L 80 40 Z" fill="var(--primary-color)"/>
            <path d="M 20 40 L 10 15 L 35 30 Z" fill="var(--primary-hover)"/>
            <path d="M 80 40 L 90 15 L 65 30 Z" fill="var(--primary-hover)"/>
            <path d="M 30 75 L 50 90 L 40 60 Z" fill="#ffffff"/>
            <path d="M 70 75 L 50 90 L 60 60 Z" fill="#ffffff"/>
            <circle cx="50" cy="90" r="4" fill="#121212"/>
            <polygon points="35,50 42,50 38,55" fill="#121212"/>
            <polygon points="65,50 58,50 62,55" fill="#121212"/>
          </svg>
        </div>
        <h1 class="auth-title">FreeClaudeDesktop</h1>
        <p class="auth-subtitle">管理您的本機代理伺服器、模型路由與優化開關</p>
        <div class="form-group" style="text-align: left;">
          <label for="token">Proxy Token</label>
          <input id="token" type="password" placeholder="請輸入 fcl_..." autocomplete="off">
        </div>
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
            <svg class="fox-logo" viewBox="0 0 100 100">
              <path d="M 50 10 L 20 40 L 30 75 L 50 90 L 70 75 L 80 40 Z" fill="var(--primary-color)"/>
              <path d="M 20 40 L 10 15 L 35 30 Z" fill="var(--primary-hover)"/>
              <path d="M 80 40 L 90 15 L 65 30 Z" fill="var(--primary-hover)"/>
              <path d="M 30 75 L 50 90 L 40 60 Z" fill="#ffffff"/>
              <path d="M 70 75 L 50 90 L 60 60 Z" fill="#ffffff"/>
              <circle cx="50" cy="90" r="4" fill="#121212"/>
              <polygon points="35,50 42,50 38,55" fill="#121212"/>
              <polygon points="65,50 58,50 62,55" fill="#121212"/>
            </svg>
            <div class="sidebar-title-group">
              <span class="sidebar-title">FreeClaudeDesktop</span>
              <span class="sidebar-subtitle">設定</span>
            </div>
          </div>
          <nav class="sidebar-nav">
            <a href="#connection" class="nav-item active" data-tab="connection">
              <span class="nav-indicator"></span>
              <span>連線設定</span>
            </a>
            <a href="#models" class="nav-item" data-tab="models">
              <span class="nav-indicator"></span>
              <span>模型與思考</span>
            </a>
            <a href="#extensions" class="nav-item" data-tab="extensions">
              <span class="nav-indicator"></span>
              <span>擴充與技能</span>
            </a>
            <a href="#optimization" class="nav-item" data-tab="optimization">
              <span class="nav-indicator"></span>
              <span>效能優化</span>
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
              <h2 class="content-title">FreeClaudeDesktop</h2>
              <p class="proxy-status">本機 Proxy : <span id="activePort">--</span></p>
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
                <div class="card-title">基本連線設定</div>
                <div class="grid">
                  <div class="form-group">
                    <label for="apiProvider">API 供應商</label>
                    <div class="select-wrapper">
                      <select id="apiProvider">
                        <option value="custom">custom</option>
                      </select>
                    </div>
                  </div>
                  <div class="form-group">
                    <label for="baseUrl">API URL</label>
                    <input id="baseUrl" type="url" required placeholder="http://127.0.0.1:4000">
                  </div>
                  <div class="form-group">
                    <label for="apiKey">API Key <span id="keyStatus" style="font-size: 0.75rem; margin-left: 0.5rem;"></span></label>
                    <input id="apiKey" type="password" placeholder="••••••••••••••••" autocomplete="new-password">
                  </div>
                  <div class="form-group">
                    <label for="authScheme">驗證方式 (Auth Scheme)</label>
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
                      <span>使用自訂 Claude.exe 路徑</span>
                      <span class="switch-desc">本機 GUI 管理功能，Web 端僅供展示</span>
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
                    <span class="dot-online"></span>
                    <span style="font-weight: 600;">已偵測 Claude Desktop</span>
                  </div>
                  <div id="detectedClaudePath" style="font-size: 0.8rem; color: var(--text-muted); margin-top: 0.5rem; font-family: monospace; word-break: break-all;">
                    偵測中...
                  </div>
                </div>
              </div>
            </section>

            <!-- 2. 模型與思考分頁 -->
            <section id="tab-models" class="tab-section hidden">
              <div class="card">
                <div class="card-title">模型別名與路由</div>
                <p class="section-desc">配置 Claude Desktop 對應的核心別名模型，可手動輸入或從偵測到的上游模型中選擇。</p>
                
                <div class="grid">
                  <div class="form-group">
                    <label for="realModelSonnet">Sonnet Model 別名</label>
                    <input id="realModelSonnet" type="text" placeholder="例如: claude-3-5-sonnet-latest" list="modelSuggestions">
                  </div>
                  <div class="form-group">
                    <label for="realModelOpus">Opus Model 別名</label>
                    <input id="realModelOpus" type="text" placeholder="例如: claude-3-opus-latest" list="modelSuggestions">
                  </div>
                  <div class="form-group">
                    <label for="realModelHaiku">Haiku Model 別名</label>
                    <input id="realModelHaiku" type="text" placeholder="例如: claude-3-5-haiku-latest" list="modelSuggestions">
                  </div>
                  <div class="form-group">
                    <label for="realModel">預設保底 Model</label>
                    <input id="realModel" type="text" placeholder="當找不到路由時使用" list="modelSuggestions">
                  </div>
                </div>
                <datalist id="modelSuggestions"></datalist>

                <h3 class="subsection-title">已偵測上游模型清單 (Discovered Models)</h3>
                <p class="section-desc">勾選「顯示」使其呈現在 Claude Desktop 列表中；「1M」啟用 100 萬 Context 上下文支援。</p>
                
                <div class="table-container">
                  <table>
                    <thead>
                      <tr>
                        <th>模型名稱</th>
                        <th style="width: 5rem; text-align: center;">顯示</th>
                        <th style="width: 5rem; text-align: center;">1M</th>
                        <th style="width: 10rem;">Reasoning Effort (思考上限)</th>
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
                <div class="card-title">擴充與本地技能</div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableQuotaCheckMock').click()">
                    <span>配額檢查攔截 (Quota Mock)</span>
                    <span class="switch-desc">攔截 max_tokens=1 且含有 "quota" 的測試請求</span>
                  </div>
                  <label class="switch" aria-label="配額檢查攔截">
                    <input type="checkbox" id="enableQuotaCheckMock">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enablePrefixDetection').click()">
                    <span>命令前綴快速檢測 (Prefix Detection)</span>
                    <span class="switch-desc">本地解析 shell 命令，避免不必要地呼叫 LLM</span>
                  </div>
                  <label class="switch" aria-label="命令前綴快速檢測">
                    <input type="checkbox" id="enablePrefixDetection">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableTitleGenerationSkip').click()">
                    <span>跳過對話標題生成</span>
                    <span class="switch-desc">直接回傳固定標題 "Conversation"，加速對話開啟</span>
                  </div>
                  <label class="switch" aria-label="跳過對話標題生成">
                    <input type="checkbox" id="enableTitleGenerationSkip">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableSuggestionModeSkip').click()">
                    <span>跳過建議提問模式</span>
                    <span class="switch-desc">直接回傳空建議，減少無用 API 請求</span>
                  </div>
                  <label class="switch" aria-label="跳過建議提問模式">
                    <input type="checkbox" id="enableSuggestionModeSkip">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableFilepathExtractionMock').click()">
                    <span>本機檔案路徑提取</span>
                    <span class="switch-desc">由命令輸出中進行本地路徑分析</span>
                  </div>
                  <label class="switch" aria-label="本機檔案路徑提取">
                    <input type="checkbox" id="enableFilepathExtractionMock">
                    <span class="slider"></span>
                  </label>
                </div>

                <div class="switch-container">
                  <div class="switch-label" onclick="document.getElementById('enableWebServerTools').click()">
                    <span>Web 網頁存取工具</span>
                    <span class="switch-desc">允許本地執行 web_search 與 web_fetch 抓取工具</span>
                  </div>
                  <label class="switch" aria-label="Web 網頁存取工具">
                    <input type="checkbox" id="enableWebServerTools">
                    <span class="slider"></span>
                  </label>
                </div>
                
                <div id="webToolsSettings" class="hidden" style="margin-left: 1.5rem; padding-left: 1rem; border-left: 2px solid var(--border-color); margin-top: 0.5rem; margin-bottom: 0.75rem;">
                  <div class="form-group">
                    <label for="webFetchAllowedSchemes">Web Fetch 允許 URL Schemes (以逗號分隔)</label>
                    <input id="webFetchAllowedSchemes" type="text" placeholder="http,https">
                  </div>
                  <div class="switch-container" style="background: none; border: none; padding: 0.5rem 0;">
                    <div class="switch-label" onclick="document.getElementById('webFetchAllowPrivateNetworks').click()">
                      <span>允許 web_fetch 存取私有網路 (Private Networks)</span>
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
                <div class="card-title">效能優化設定</div>
                
                <div class="grid">
                  <div class="form-group">
                    <label for="transportType">傳輸協定 (Transport Protocol)</label>
                    <div class="select-wrapper">
                      <select id="transportType">
                        <option value="openai_chat">OpenAI Chat 格式轉換</option>
                        <option value="anthropic_messages">原生 Anthropic passthrough</option>
                      </select>
                    </div>
                  </div>
                  
                  <div class="form-group">
                    <label for="reasoningReplayMode">Thinking 模式 (Reasoning Replay)</label>
                    <div class="select-wrapper">
                      <select id="reasoningReplayMode">
                        <option value="disabled">不啟用 (丟棄思考內容)</option>
                        <option value="think_tags">Think Tags (包裝在 &lt;thinking&gt; 標籤)</option>
                        <option value="reasoning_content">Reasoning Content 欄位 (Provider 原生支援)</option>
                      </select>
                    </div>
                  </div>
                </div>

                <select id="themeMode" class="hidden">
                  <option value="light">明亮 (Light)</option>
                  <option value="dark">深色 (Dark)</option>
                  <option value="system">系統 (System)</option>
                </select>

                <div style="margin-top: 2rem;">
                  <button type="button" class="btn btn-secondary" id="launchClaudeBtn" style="width: 100%;">
                    啟動 Claude Desktop
                  </button>
                </div>
              </div>
            </section>
          </div>

          <!-- 3. 底部固定動作列 -->
          <footer class="bottom-actions">
            <div class="actions-wrapper">
              <button type="button" class="btn btn-secondary" id="resetMirrorBtn">重置鏡像 Profile</button>
              <button type="button" class="btn btn-secondary" id="syncOfficialBtn">從原版同步</button>
              <button type="button" class="btn btn-secondary" id="saveOnlyBtn">僅儲存</button>
              <button type="submit" class="btn btn-primary" id="saveAndLaunchBtn">儲存並啟動 ↵</button>
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

    const headers = () => ({
      'Authorization': 'Bearer ' + $('token').value.trim()
    });

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
      const token = $('token').value.trim();
      if (!token) {
        showToast('請輸入 Proxy Token', 'error');
        return;
      }
      
      showLoading(true);
      try {
        const [settings, status] = await Promise.all([
          request('/admin/settings'),
          request('/admin/status')
        ]);
        
        loadedSettings = settings;
        
        $('authWrapper').classList.add('hidden');
        $('mainLayout').classList.remove('hidden');
        $('appContainer').classList.remove('unauthorized');
        
        $('activePort').textContent = '127.0.0.1 : ' + (status.proxy.port || '3000');
        
        $('baseUrl').value = settings.baseUrl || '';
        $('authScheme').value = settings.authScheme || 'bearer';
        $('apiKey').placeholder = settings.hasApiKey ? '•••••••••••••••• (已儲存)' : '尚未設定 API Key';
        $('keyStatus').textContent = settings.hasApiKey ? '✅ 已儲存金鑰' : '❌ 未儲存金鑰';
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
        
        renderModelsTable(settings);
        
        $('transportType').value = settings.transportType || 'openai_chat';
        $('reasoningReplayMode').value = settings.reasoningReplayMode || 'think_tags';
        
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
        
        // Detect Claude Path via RPC
        try {
          const detectRes = await request('/admin/rpc', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ method: 'DetectClaude' })
          });
          if (detectRes && detectRes.result && detectRes.result.path) {
            $('detectedClaudePath').textContent = detectRes.result.path;
          } else {
            $('detectedClaudePath').textContent = '未偵測到安裝路徑，將使用預設路徑';
          }
        } catch (err) {
          $('detectedClaudePath').textContent = '無法偵測安裝路徑';
        }
        
        showToast('設定載入成功！');
      } catch (e) {
        showToast('載入失敗: ' + e.message, 'error');
      } finally {
        showLoading(false);
      }
    }

    function renderModelsTable(settings) {
      const tbody = $('modelsTableBody');
      tbody.innerHTML = '';
      
      const models = settings.discoveredModels || [];
      if (models.length === 0) {
        tbody.innerHTML = `<tr><td colspan="4" style="text-align: center; color: var(--text-muted);">尚未偵測到官方上游模型</td></tr>`;
        return;
      }
      
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
                <option value="" ${effort === '' ? 'selected' : ''}>預設 (Default)</option>
                <option value="low" ${effort === 'low' ? 'selected' : ''}>低 (Low)</option>
                <option value="medium" ${effort === 'medium' ? 'selected' : ''}>中 (Medium)</option>
                <option value="high" ${effort === 'high' ? 'selected' : ''}>高 (High)</option>
              </select>
            </div>
          </td>
        `;
        tbody.appendChild(tr);
      });
    }

    $('loadBtn').onclick = load;
    $('token').addEventListener('keypress', (e) => {
      if (e.key === 'Enter') load();
    });

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

        await request('/admin/settings', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload)
        });

        $('apiKey').value = '';
        showToast('設定已成功儲存！');
        
        if (launchAfterSave) {
          try {
            const launchRes = await request('/admin/rpc', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ method: 'LaunchClaude' })
            });
            showToast('Claude Desktop 啟動成功，路徑: ' + launchRes.result.path);
          } catch (launchErr) {
            showToast('儲存成功，但 Claude 啟動失敗: ' + launchErr.message, 'error');
          }
        }
        
        await load();
      } catch (e) {
        showToast('儲存失敗: ' + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    // RPC Actions
    $('launchClaudeBtn').onclick = async () => {
      showLoading(true);
      try {
        const res = await request('/admin/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'LaunchClaude' })
        });
        showToast('Claude Desktop 啟動成功，路徑: ' + res.result.path);
      } catch(e) {
        showToast('啟動失敗: ' + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('resetMirrorBtn').onclick = async () => {
      if (!confirm('⚠ 確定要重置鏡像 Profile 目錄？原版目錄完全不受影響。')) return;
      showLoading(true);
      try {
        await request('/admin/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'ResetMirrorProfile' })
        });
        showToast('已成功重置鏡像目錄！');
        await load();
      } catch(e) {
        showToast('重置失敗: ' + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('syncOfficialBtn').onclick = async () => {
      if (!confirm('⚠ 確定要從官方原版 Claude Desktop 同步配置？')) return;
      showLoading(true);
      try {
        await request('/admin/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'SyncFromOfficial' })
        });
        showToast('已成功從原版同步！');
        await load();
      } catch(e) {
        showToast('同步失敗: ' + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    // Theme initialization
    (function() {
      const savedTheme = localStorage.getItem('theme') || 'system';
      applyTheme(savedTheme);
    })();
  </script>
</body>
</html>"#,
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
        return (StatusCode::OK, Json(json!({
            "result": {
                "proxy": { "status": "ok", "port": settings.active_port },
                "settings": to_public_config(&settings),
            }
        }))).into_response();
    }

    // Check active companion connection
    let tx_opt = {
        let active = ACTIVE_COMPANION.lock().await;
        active.as_ref().map(|c| c.tx.clone())
    };

    let companion_tx = match tx_opt {
        Some(tx) => tx,
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Companion offline" }))).into_response();
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
        obj.insert("token".to_string(), Value::String(settings.proxy_auth_token.clone()));
    }

    let (response_tx, response_rx) = oneshot::channel();
    let msg = ProxyToCompanionMessage {
        request_id,
        payload: payload_val.to_string(),
        response_tx,
    };

    if companion_tx.send(msg).is_err() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Companion offline" }))).into_response();
    }

    match response_rx.await {
        Ok(Ok(res)) => (StatusCode::OK, Json(json!({ "result": res }))).into_response(),
        Ok(Err(err)) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": err }))).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Companion disconnected" }))).into_response(),
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
    let pending_requests: Arc<tokio::sync::Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>> = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
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
                        if let Ok(resp_val) = serde_json::from_str::<Value>(&text) {
                            if let Some(req_id) = resp_val.get("requestId").and_then(|v| v.as_str()) {
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
        token: String,
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
    fn companion_request_requires_token_and_request_id() {
        assert!(
            serde_json::from_str::<CompanionRequest>(
                r#"{"requestId":"1","token":"secret","method":"GetStatus"}"#
            )
            .is_ok()
        );
        assert!(
            serde_json::from_str::<CompanionRequest>(r#"{"token":"secret","method":"GetStatus"}"#)
                .is_err()
        );
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
                                false,
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
                                false,
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
                                false,
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
