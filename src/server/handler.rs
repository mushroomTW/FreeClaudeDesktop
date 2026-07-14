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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminSettingsUpdate {
    base_url: String,
    auth_scheme: String,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "method")]
pub enum AdminRpcRequest {
    GetStatus,
    DetectClaude,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanionRequest {
    request_id: String,
    token: String,
    #[serde(flatten)]
    request: AdminRpcRequest,
}

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

pub async fn handle_admin_page() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html lang="zh-Hant"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>FreeClaude Admin</title><style>body{max-width:44rem;margin:2rem auto;padding:0 1rem;font:16px system-ui;color:#172033}label{display:block;margin-top:1rem;font-weight:600}input,select,button{box-sizing:border-box;width:100%;padding:.6rem;margin-top:.35rem}button{background:#2155d6;color:white;border:0;border-radius:.3rem;font-weight:700;cursor:pointer}pre{padding:1rem;background:#f3f5f9;white-space:pre-wrap}#message{min-height:1.4em}</style></head>
<body><h1>FreeClaude Admin</h1><p>Token 只保留在此頁面記憶體中，不會寫入設定檔或瀏覽器儲存空間。</p>
<label>Proxy token<input id="token" type="password" autocomplete="off"></label><button id="load">載入設定</button>
<form id="settings"><label>Gateway URL<input id="baseUrl" required type="url"></label><label>驗證方式<select id="authScheme"><option value="bearer">Bearer</option><option value="x-api-key">X-API-Key</option></select></label><label>API key（留空保留原值）<input id="apiKey" type="password" autocomplete="new-password"></label><button>儲存設定</button></form>
<h2>Runtime 狀態</h2><pre id="status">尚未載入</pre><p id="message" role="status"></p>
<script>const $=id=>document.getElementById(id),message=$('message');const headers=()=>({Authorization:'Bearer '+$('token').value});async function request(path,options={}){const r=await fetch(path,{...options,headers:{...headers(),...(options.headers||{})}});const b=await r.json();if(!r.ok)throw new Error(b.error||r.statusText);return b}async function load(){try{const [settings,status]=await Promise.all([request('/admin/settings'),request('/admin/status')]);$('baseUrl').value=settings.baseUrl||'';$('authScheme').value=settings.authScheme||'bearer';$('status').textContent=JSON.stringify(status,null,2);message.textContent='設定已載入'}catch(e){message.textContent='錯誤：'+e.message}}$('load').onclick=load;$('settings').onsubmit=async e=>{e.preventDefault();try{await request('/admin/settings',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({baseUrl:$('baseUrl').value,authScheme:$('authScheme').value,apiKey:$('apiKey').value})});$('apiKey').value='';message.textContent='設定已儲存';await load()}catch(e){message.textContent='錯誤：'+e.message}}</script></body></html>"#,
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
    let base_url = match validate_gateway_url(&input.base_url) {
        Ok(base_url) => base_url,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
        }
    };

    let auth_scheme = input.auth_scheme.trim().to_ascii_lowercase();
    if !matches!(auth_scheme.as_str(), "bearer" | "x-api-key") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "authScheme 必須是 bearer 或 x-api-key" })),
        )
            .into_response();
    }

    settings.real_base_url = base_url;
    settings.real_auth_scheme = auth_scheme;
    if let Some(api_key) = input.api_key.map(|key| key.trim().to_string())
        && !api_key.is_empty()
    {
        settings.real_api_key = match crate::protect_secret(&api_key) {
            Ok(secret) => secret,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
                    .into_response();
            }
        };
    }

    match crate::save_launcher_settings(&settings) {
        Ok(()) => (StatusCode::OK, Json(to_public_config(&settings))).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
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

    let result = match request {
        AdminRpcRequest::GetStatus => json!({
            "proxy": { "status": "ok", "port": settings.active_port },
            "settings": to_public_config(&settings),
        }),
        AdminRpcRequest::DetectClaude => json!({
            "path": crate::detect_claude_path().map(|path| path.display().to_string()),
        }),
    };
    (StatusCode::OK, Json(json!({ "result": result }))).into_response()
}

pub async fn handle_companion_websocket(websocket: WebSocketUpgrade) -> impl IntoResponse {
    websocket.on_upgrade(handle_companion_session)
}

async fn handle_companion_session(mut socket: WebSocket) {
    let Some(Ok(Message::Text(message))) = socket.recv().await else {
        return;
    };
    let request = match serde_json::from_str::<CompanionRequest>(&message) {
        Ok(request) => request,
        Err(error) => {
            let _ = socket
                .send(Message::Text(
                    json!({ "error": "invalid_request", "message": error.to_string() })
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };
    let settings = match load_runtime_settings().await {
        Ok(Some(settings)) if settings.proxy_auth_token == request.token => settings,
        _ => {
            let _ = socket
                .send(Message::Text(
                    json!({ "requestId": request.request_id, "error": "unauthorized" })
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };
    let result = match request.request {
        AdminRpcRequest::GetStatus => json!({
            "proxy": { "status": "ok", "port": settings.active_port },
            "settings": to_public_config(&settings),
        }),
        AdminRpcRequest::DetectClaude => json!({
            "path": crate::detect_claude_path().map(|path| path.display().to_string()),
        }),
    };
    let _ = socket
        .send(Message::Text(
            json!({ "requestId": request.request_id, "result": result })
                .to_string()
                .into(),
        ))
        .await;
}

#[cfg(test)]
mod healthz_tests {
    use super::*;

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
