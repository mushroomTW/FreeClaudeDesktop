use axum::{
    Json,
    body::{Body, Bytes},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime};

pub(crate) const MAX_UPSTREAM_ERROR_PREVIEW_CHARS: usize = 4096;

/// 產生不含使用者內容的安全請求摘要。
pub(crate) fn request_diagnostic(body: &str) -> Option<String> {
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

/// 攔截 Claude Desktop 的背景連線健康檢查並立即回傳成功。
pub(crate) fn try_probe_response(body: &str, model: &str) -> Option<Response> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    let max_tokens = value
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(9999);
    let is_stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let messages_empty = value
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    let has_user_content =
        value
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages.iter().any(|message| {
                    message.get("role").and_then(Value::as_str) == Some("user")
                        && message
                            .get("content")
                            .is_some_and(|content| !content.is_null())
                })
            });

    if has_user_content || !messages_empty || max_tokens > 5 || body.len() >= 400 {
        return None;
    }

    tracing::info!("-> [探測攔截] 繞過 Claude 檢查，自動回傳成功回應 (model: {model})");
    let message_id = probe_message_id();

    if is_stream {
        Some(stream_probe_response(&message_id, model))
    } else {
        Some(json_probe_response(&message_id, model))
    }
}

/// 判斷空白成功本文是否來自 Claude Desktop 的短連線探測。
pub(crate) fn is_short_connection_probe(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
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
        && body.len() <= 256
}

/// 建立非串流 Claude Desktop 連線探測的最小成功回應。
pub(crate) fn non_stream_probe_response(model: &str) -> Response {
    json_probe_response(&probe_message_id(), model)
}

/// 將無法解析的 OpenAI 回應轉成 Claude 可診斷的錯誤。
pub(crate) fn invalid_openai_response(
    upstream_status: reqwest::StatusCode,
    response_body: &str,
    parse_error: &str,
    request_body: &str,
    model: &str,
) -> Response {
    if upstream_status.is_success() && is_short_connection_probe(request_body) {
        tracing::warn!(
            "上游探測回應無法解析，改回傳本機探測成功結果（model: {model}）：{parse_error}"
        );
        return non_stream_probe_response(model);
    }

    let trimmed_body = response_body.trim();
    let response_preview = bounded_preview(trimmed_body);
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
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": error_message,
            "upstreamStatus": upstream_status.as_u16(),
            "responseBody": response_preview
        })),
    )
        .into_response()
}

fn probe_message_id() -> String {
    format!(
        "msg_probe_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis()
    )
}

fn json_probe_response(message_id: &str, model: &str) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "text", "text": "." }],
            "model": model,
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })),
    )
        .into_response()
}

fn stream_probe_response(message_id: &str, model: &str) -> Response {
    let events = vec![
        format!(
            "event: message_start\ndata: {}\n\n",
            json!({
                "type": "message_start",
                "message": {
                    "id": message_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": model,
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
    let stream = futures::stream::iter(
        events
            .into_iter()
            .map(|event| Ok::<_, std::convert::Infallible>(Bytes::from(event))),
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(stream))
        .expect("固定的探測回應標頭必須有效")
}

fn bounded_preview(body: &str) -> String {
    if body.is_empty() {
        return "<empty>".to_string();
    }
    let mut preview: String = body
        .chars()
        .take(MAX_UPSTREAM_ERROR_PREVIEW_CHARS)
        .collect();
    if preview.len() < body.len() {
        preview.push_str("...");
    }
    preview
}
