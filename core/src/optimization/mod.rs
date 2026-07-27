//! Local optimization handlers for fast-path API responses.
//!
//! Each handler checks if the request matches known Claude Code auxiliary
//! request patterns and returns an immediate response, saving API quota.

pub mod command_utils;
pub mod detection;
pub mod web_tools;

use serde_json::{Value, json};
use std::time::{Duration, SystemTime};

use crate::config::Settings;
use detection::*;

/// 本機最佳化命中後產生的傳輸中立結果。
#[derive(Debug, Clone, PartialEq)]
pub enum OptimizationResponse {
    Json(Value),
    Sse(Vec<String>),
}

/// 建立一次性 JSON 回應內容。
pub fn build_text_response(
    model: &str,
    text: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Value {
    let msg_id = format!(
        "msg_opt_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis()
    );

    let response = json!({
        "id": msg_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [
            { "type": "text", "text": text }
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    });

    response
}

/// 建立預先產生的 SSE 事件。
pub fn build_text_sse(
    model: &str,
    text: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Vec<String> {
    let msg_id = format!(
        "msg_opt_sse_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis()
    );

    vec![
        format!(
            "event: message_start\ndata: {}\n\n",
            json!({
                "type": "message_start",
                "message": {
                    "id": msg_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": model,
                    "stop_reason": null,
                    "usage": { "input_tokens": input_tokens, "output_tokens": 0 }
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
                "delta": { "type": "text_delta", "text": text }
            })
        ),
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n"
            .to_string(),
        format!(
            "event: message_delta\ndata: {}\n\n",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                "usage": { "input_tokens": input_tokens, "output_tokens": output_tokens }
            })
        ),
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
    ]
}

/// 處理 `request_is_stream` 對應的請求。
fn request_is_stream(body_str: &str) -> bool {
    serde_json::from_str::<Value>(body_str)
        .ok()
        .and_then(|v| v.get("stream").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// 建立 `build_optimized_text_response` 所需的結果。
fn build_optimized_text_response(
    body_str: &str,
    model: &str,
    text: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> OptimizationResponse {
    if request_is_stream(body_str) {
        OptimizationResponse::Sse(build_text_sse(model, text, input_tokens, output_tokens))
    } else {
        OptimizationResponse::Json(build_text_response(
            model,
            text,
            input_tokens,
            output_tokens,
        ))
    }
}

/// Try all optimization handlers in order.
/// Returns `Some(Response)` if any handler matched.
///
/// Order (cheapest / most common first):
///   1. Quota check mock
///   2. Prefix detection
///   3. Title skip
///   4. Suggestion skip
///   5. Filepath mock
///   6. Web server tool interception
///   7. Safety classifier handling
pub async fn try_optimizations(
    body_str: &str,
    settings: &Settings,
) -> Option<OptimizationResponse> {
    // 1. Quota check (detect first, trivial cost)
    if settings.optimizations.enable_quota_check_mock && is_quota_check_request(body_str) {
        tracing::info!("Optimization: mocked quota check");
        let model = extract_model(body_str);
        return Some(build_optimized_text_response(
            body_str,
            &model,
            "配額檢查通過。",
            10,
            5,
        ));
    }

    // 2. Prefix detection
    if settings.optimizations.enable_prefix_detection
        && let Some(prefix) = extract_command_prefix(body_str)
    {
        tracing::info!("Optimization: fast prefix detection");
        let model = extract_model(body_str);
        return Some(build_optimized_text_response(
            body_str, &model, &prefix, 100, 5,
        ));
    }

    // 3. Title generation skip
    if settings.optimizations.enable_title_generation_skip && is_title_generation_request(body_str)
    {
        tracing::info!("Optimization: skipped title generation");
        let model = extract_model(body_str);
        return Some(build_optimized_text_response(
            body_str,
            &model,
            "Conversation",
            100,
            5,
        ));
    }

    // 4. Suggestion mode skip
    if settings.optimizations.enable_suggestion_mode_skip && is_suggestion_mode_request(body_str) {
        tracing::info!("Optimization: skipped suggestion mode");
        let model = extract_model(body_str);
        return Some(build_optimized_text_response(body_str, &model, "", 100, 1));
    }

    // 5. Filepath extraction
    if settings.optimizations.enable_filepath_extraction_mock
        && let Some(filepaths) = extract_filepaths(body_str)
    {
        tracing::info!("Optimization: mocked filepath extraction");
        let model = extract_model(body_str);
        return Some(build_optimized_text_response(
            body_str, &model, &filepaths, 100, 10,
        ));
    }

    // 6. Web server tools
    if settings.optimizations.enable_web_server_tools
        && let Some((_id, name, input)) = web_tools::extract_latest_web_tool_call(body_str)
    {
        let policy = web_tools::policy_from_settings(settings);
        if let Some(text) = web_tools::execute_web_tool(&policy, &name, &input).await {
            tracing::info!("Optimization: executed local web tool {}", name);
            let model = extract_model(body_str);
            return Some(build_optimized_text_response(
                body_str, &model, &text, 100, 100,
            ));
        }
    }

    None
}

/// 解析 `extract_model` 所需的資料。
fn extract_model(body_str: &str) -> String {
    serde_json::from_str::<Value>(body_str)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    #[tokio::test]
    /// 驗證 `non_stream_optimization_returns_json_message` 的行為符合預期。
    async fn non_stream_optimization_returns_json_message() {
        let settings = Settings::default();
        let body = json!({
            "model": "claude-test",
            "max_tokens": 1,
            "stream": false,
            "tools": [{ "name": "usage_probe" }],
            "messages": [{ "role": "user", "content": "count" }]
        })
        .to_string();

        let OptimizationResponse::Json(value) = try_optimizations(&body, &settings).await.unwrap()
        else {
            panic!("非串流請求應回傳 JSON");
        };
        assert_eq!(value["content"][0]["text"], "配額檢查通過。");
    }

    #[tokio::test]
    /// 驗證 `web_fetch_blocks_private_network_when_disabled` 的行為符合預期。
    async fn web_fetch_blocks_private_network_when_disabled() {
        let settings = {
            let mut settings = Settings::default();
            settings.optimizations.enable_web_server_tools = true;
            settings.optimizations.web_fetch_allowed_schemes = "http,https".to_string();
            settings.optimizations.web_fetch_allow_private_networks = false;
            settings
        };
        let body = json!({
            "model": "claude-test",
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "web_fetch",
                    "input": { "url": "http://127.0.0.1:9/private" }
                }]
            }]
        })
        .to_string();

        let OptimizationResponse::Json(value) = try_optimizations(&body, &settings).await.unwrap()
        else {
            panic!("非串流請求應回傳 JSON");
        };
        let text = value["content"][0]["text"].as_str().unwrap();

        assert!(text.contains("Private network access is not allowed"));
    }

    #[tokio::test]
    /// 驗證 `web_fetch_returns_page_text_when_allowed` 的行為符合預期。
    async fn web_fetch_returns_page_text_when_allowed() {
        use axum::{Router, routing::get};

        let app = Router::new().route("/page", get(|| async { "hello from web fetch" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let settings = {
            let mut settings = Settings::default();
            settings.optimizations.enable_web_server_tools = true;
            settings.optimizations.web_fetch_allowed_schemes = "http,https".to_string();
            settings.optimizations.web_fetch_allow_private_networks = true;
            settings
        };
        let body = json!({
            "model": "claude-test",
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "web_fetch",
                    "input": { "url": format!("http://{addr}/page") }
                }]
            }]
        })
        .to_string();

        let OptimizationResponse::Json(value) = try_optimizations(&body, &settings).await.unwrap()
        else {
            panic!("非串流請求應回傳 JSON");
        };
        let text = value["content"][0]["text"].as_str().unwrap();

        assert!(text.contains("hello from web fetch"));
    }
}
