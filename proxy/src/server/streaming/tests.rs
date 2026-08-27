use super::*;
use axum::{Router, routing::get};

#[tokio::test]
/// 驗證 `stream_converts_reasoning_content_to_thinking_events` 的行為符合預期。
async fn stream_converts_reasoning_content_to_thinking_events() {
    let app = Router::new().route(
        "/",
        get(|| async {
            axum::response::Response::builder()
                .header("Content-Type", "text/event-stream")
                .body(axum::body::Body::from(concat!(
                    "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"brief thought\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"4\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
                    "data: [DONE]\n\n"
                )))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
    let mut rx = start_sse_stream_conversion(
        response,
        "claude-test".to_string(),
        Some(ReasoningReplayMode::Separate),
    );
    let mut out = String::new();
    while let Some(Ok(bytes)) = rx.recv().await {
        out.push_str(&String::from_utf8_lossy(&bytes));
    }

    assert!(out.contains("\"type\":\"thinking\""));
    assert!(out.contains("\"type\":\"thinking_delta\""));
    assert!(out.contains("\"thinking\":\"brief thought\""));
    assert!(out.contains("\"type\":\"signature_delta\""));
    assert!(out.contains("\"type\":\"text_delta\""));
    assert!(out.contains("\"text\":\"4\""));
}

#[tokio::test]
/// 驗證 `stream_handles_reasoning_after_text_content` 的行為符合預期。
async fn stream_handles_reasoning_after_text_content() {
    let app = Router::new().route(
        "/",
        get(|| async {
            axum::response::Response::builder()
                .header("Content-Type", "text/event-stream")
                .body(axum::body::Body::from(concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thought later\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                )))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
    let mut rx = start_sse_stream_conversion(
        response,
        "claude-test".to_string(),
        Some(ReasoningReplayMode::Separate),
    );
    let mut out = String::new();
    while let Some(Ok(bytes)) = rx.recv().await {
        out.push_str(&String::from_utf8_lossy(&bytes));
    }

    // Index 0: text ("Hello")
    assert!(out.contains("{\"content_block\":{\"text\":\"\",\"type\":\"text\"},\"index\":0,\"type\":\"content_block_start\"}"));
    assert!(out.contains("{\"delta\":{\"text\":\"Hello\",\"type\":\"text_delta\"},\"index\":0,\"type\":\"content_block_delta\"}"));
    assert!(out.contains("{\"type\":\"content_block_stop\",\"index\":0}"));

    // Index 1: thinking ("thought later")
    assert!(out.contains("{\"content_block\":{\"signature\":\"\",\"thinking\":\"\",\"type\":\"thinking\"},\"index\":1,\"type\":\"content_block_start\"}"));
    assert!(out.contains("{\"delta\":{\"thinking\":\"thought later\",\"type\":\"thinking_delta\"},\"index\":1,\"type\":\"content_block_delta\"}"));
    assert!(out.contains("{\"type\":\"content_block_stop\",\"index\":1}"));

    // Index 2: text (" world")
    assert!(out.contains("{\"content_block\":{\"text\":\"\",\"type\":\"text\"},\"index\":2,\"type\":\"content_block_start\"}"));
    assert!(out.contains("{\"delta\":{\"text\":\" world\",\"type\":\"text_delta\"},\"index\":2,\"type\":\"content_block_delta\"}"));
    assert!(out.contains("{\"type\":\"content_block_stop\",\"index\":2}"));
}

#[test]
fn build_usage_json_covers_cached_tokens() {
    let usage = serde_json::json!({
        "prompt_tokens": 10,
        "completion_tokens": 5,
        "prompt_tokens_details": {"cached_tokens": 3}
    });
    let s = build_usage_json(&Some(usage));
    assert!(s.contains("cache_read_input_tokens"));
    let s2 = build_usage_json(&None);
    assert!(s2.contains("input_tokens"));
    let usage_no_cache = serde_json::json!({"prompt_tokens": 1, "completion_tokens": 1});
    let s3 = build_usage_json(&Some(usage_no_cache));
    assert!(s3.contains("input_tokens"));
}

#[tokio::test]
async fn process_data_line_covers_all_branches() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    let mut sent_start = false;
    let mut sent_stop = false;
    let mut thinking_open = false;
    let mut text_open = false;
    let mut idx = 0u64;
    let mut active: std::collections::HashMap<u64, ToolCallState> =
        std::collections::HashMap::new();
    let mut finish = None;
    let mut usage = None;
    // invalid JSON -> should continue
    let r = process_data_line(
        "not json",
        &mut sent_start,
        &mut sent_stop,
        &mut thinking_open,
        &mut text_open,
        &mut idx,
        &mut active,
        &mut finish,
        &mut usage,
        "msg_1",
        "claude-test",
        ReasoningReplayMode::Separate,
        &tx,
    )
    .await;
    assert!(!r);
    // usage only
    let r2 = process_data_line(
        r#"{"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
        &mut sent_start,
        &mut sent_stop,
        &mut thinking_open,
        &mut text_open,
        &mut idx,
        &mut active,
        &mut finish,
        &mut usage,
        "msg_1",
        "claude-test",
        ReasoningReplayMode::Separate,
        &tx,
    )
    .await;
    assert!(!r2);
    assert!(usage.is_some());
    // content
    let r3 = process_data_line(
        r#"{"choices":[{"delta":{"content":"hi"}}]}"#,
        &mut sent_start,
        &mut sent_stop,
        &mut thinking_open,
        &mut text_open,
        &mut idx,
        &mut active,
        &mut finish,
        &mut usage,
        "msg_1",
        "claude-test",
        ReasoningReplayMode::Separate,
        &tx,
    )
    .await;
    assert!(!r3);
    // reasoning
    let r4 = process_data_line(
        r#"{"choices":[{"delta":{"reasoning_content":"think"}}]}"#,
        &mut sent_start,
        &mut sent_stop,
        &mut thinking_open,
        &mut text_open,
        &mut idx,
        &mut active,
        &mut finish,
        &mut usage,
        "msg_1",
        "claude-test",
        ReasoningReplayMode::Separate,
        &tx,
    )
    .await;
    assert!(!r4);
    // tool_calls
    let r5 = process_data_line(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"toolu_1","function":{"name":"bash","arguments":"{}"}}]}}]}"#, &mut sent_start, &mut sent_stop, &mut thinking_open, &mut text_open, &mut idx, &mut active, &mut finish, &mut usage, "msg_1", "claude-test", ReasoningReplayMode::Separate, &tx).await;
    assert!(!r5);
    // finish reason
    let r6 = process_data_line(
        r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        &mut sent_start,
        &mut sent_stop,
        &mut thinking_open,
        &mut text_open,
        &mut idx,
        &mut active,
        &mut finish,
        &mut usage,
        "msg_1",
        "claude-test",
        ReasoningReplayMode::Separate,
        &tx,
    )
    .await;
    assert!(!r6);
    assert_eq!(finish, Some("stop".to_string()));
    // DONE
    sent_start = true;
    let r7 = process_data_line(
        "[DONE]",
        &mut sent_start,
        &mut sent_stop,
        &mut thinking_open,
        &mut text_open,
        &mut idx,
        &mut active,
        &mut finish,
        &mut usage,
        "msg_1",
        "claude-test",
        ReasoningReplayMode::Separate,
        &tx,
    )
    .await;
    assert!(r7);
    drop(tx);
    // drain
    while let Some(_) = rx.recv().await {}
}

#[tokio::test]
async fn stream_covers_tool_calls_and_inline_reasoning() {
    let app = Router::new().route(
        "/",
        get(|| async {
            axum::response::Response::builder()
                .header("Content-Type", "text/event-stream")
                .body(axum::body::Body::from(concat!(
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"toolu_1\",\"function\":{\"name\":\"bash\",\"arguments\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"more\"}}]}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"inline think\"}}]}\n\n",
                    "data: [DONE]\n\n"
                )))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
    let mut rx = start_sse_stream_conversion(
        response,
        "claude-test".to_string(),
        Some(ReasoningReplayMode::Inline),
    );
    let mut out = String::new();
    while let Some(Ok(b)) = rx.recv().await {
        out.push_str(&String::from_utf8_lossy(&b));
    }
    assert!(out.contains("tool_use") || out.contains("input_json_delta"));
    assert!(out.contains("antThinking") || out.contains("thinking"));
}

#[tokio::test]
/// 驗證 `stream_does_not_break_early_on_finish_reason_and_includes_usage` 的行為符合預期。
async fn stream_does_not_break_early_on_finish_reason_and_includes_usage() {
    let app = Router::new().route(
        "/",
        get(|| async {
            axum::response::Response::builder()
                .header("Content-Type", "text/event-stream")
                .body(axum::body::Body::from(concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":42,\"completion_tokens\":100}}\n\n",
                    "data: [DONE]\n\n"
                )))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
    let mut rx = start_sse_stream_conversion(
        response,
        "claude-test".to_string(),
        Some(ReasoningReplayMode::Separate),
    );
    let mut out = String::new();
    while let Some(Ok(bytes)) = rx.recv().await {
        out.push_str(&String::from_utf8_lossy(&bytes));
    }

    assert!(out.contains("\"text\":\"Hello\""));
    assert!(out.contains("\"input_tokens\":42"));
    assert!(out.contains("\"output_tokens\":100"));
    assert!(out.contains("\"stop_reason\":\"end_turn\""));
    assert!(out.contains("event: message_stop"));
}
