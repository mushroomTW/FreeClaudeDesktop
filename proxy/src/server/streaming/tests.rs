use super::*;
use axum::{Router, routing::get};

#[tokio::test]
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

#[tokio::test]
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
