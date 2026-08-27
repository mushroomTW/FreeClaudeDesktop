#![allow(clippy::too_many_arguments)]
use axum::body::Bytes;
use futures::StreamExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;

struct ToolCallState {
    id: String,
    name: String,
    started: bool,
}

/// Determines how reasoning content should be replayed in the stream.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ReasoningReplayMode {
    /// Emit `<antThinking>` tags inline with text.
    Inline,
    /// Emit thinking blocks before the text blocks (separate).
    #[default]
    Separate,
}

/// Start SSE stream conversion with optional reasoning replay mode.
pub fn start_sse_stream_conversion(
    response: reqwest::Response,
    req_model: String,
    reasoning_mode: Option<ReasoningReplayMode>,
) -> mpsc::Receiver<Result<Bytes, std::convert::Infallible>> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        if let Err(e) = convert_stream_inner(
            response,
            req_model,
            tx.clone(),
            reasoning_mode.unwrap_or_default(),
        )
        .await
        {
            tracing::error!("SSE stream conversion error: {:?}", e);
            let err_json = json!({
                "type": "error",
                "error": {
                    "type": "api_error",
                    "message": format!("Stream conversion failed: {:?}", e)
                }
            });
            let _ = tx
                .send(Ok(Bytes::from(format!(
                    "event: error\ndata: {}\n\n",
                    err_json
                ))))
                .await;
        }
    });

    rx
}

fn build_usage_json(usage: &Option<Value>) -> String {
    let mut j = json!({"input_tokens": 0, "output_tokens": 0});
    if let Some(val) = usage {
        let input = val
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output = val
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        j["input_tokens"] = json!(input);
        j["output_tokens"] = json!(output);
        let cached = val
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if cached > 0 {
            j["cache_read_input_tokens"] = json!(cached);
        }
    }
    serde_json::to_string(&j)
        .unwrap_or_else(|_| "{\"input_tokens\":0,\"output_tokens\":0}".to_string())
}

async fn ensure_message_start(
    sent_start: &mut bool,
    msg_id: &str,
    req_model: &str,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if *sent_start {
        return;
    }
    let start_msg = json!({
        "type": "message_start",
        "message": {"id": msg_id, "type": "message", "role": "assistant", "content": [], "model": req_model, "stop_reason": null, "usage": {"input_tokens": 0, "output_tokens": 0}}
    });
    let _ = tx
        .send(Ok(Bytes::from(format!(
            "event: message_start\ndata: {}\n\n",
            start_msg
        ))))
        .await;
    *sent_start = true;
}

async fn emit_text_block_start(
    text_block_open: &mut bool,
    content_block_index: u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if *text_block_open {
        return;
    }
    let block_start = json!({
        "type": "content_block_start", "index": content_block_index,
        "content_block": {"type": "text", "text": ""}
    });
    let _ = tx
        .send(Ok(Bytes::from(format!(
            "event: content_block_start\ndata: {}\n\n",
            block_start
        ))))
        .await;
    *text_block_open = true;
}

async fn emit_thinking_block_start(
    thinking_open: &mut bool,
    content_block_index: u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if *thinking_open {
        return;
    }
    let block_start = json!({
        "type": "content_block_start", "index": content_block_index,
        "content_block": {"type": "thinking", "thinking": "", "signature": ""}
    });
    let _ = tx
        .send(Ok(Bytes::from(format!(
            "event: content_block_start\ndata: {}\n\n",
            block_start
        ))))
        .await;
    *thinking_open = true;
}

async fn close_thinking_if_needed(
    thinking_open: &mut bool,
    content_block_index: &mut u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if *thinking_open {
        finish_thinking_block(*content_block_index, tx).await;
        *thinking_open = false;
        *content_block_index += 1;
    }
}

async fn close_text_if_needed(
    text_open: &mut bool,
    content_block_index: &mut u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if *text_open {
        let _ = tx
            .send(Ok(Bytes::from(format!(
                "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                *content_block_index
            ))))
            .await;
        *text_open = false;
        *content_block_index += 1;
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_reasoning_delta(
    delta: &str,
    reasoning_mode: ReasoningReplayMode,
    sent_start: &mut bool,
    msg_id: &str,
    req_model: &str,
    text_open: &mut bool,
    thinking_open: &mut bool,
    content_block_index: &mut u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if delta.is_empty() {
        return;
    }
    match reasoning_mode {
        ReasoningReplayMode::Inline => {
            ensure_message_start(sent_start, msg_id, req_model, tx).await;
            emit_text_block_start(text_open, *content_block_index, tx).await;
            let inline = format!("<antThinking>{}</antThinking>", delta);
            let block_delta = json!({
                "type": "content_block_delta", "index": *content_block_index,
                "delta": {"type": "text_delta", "text": inline}
            });
            let _ = tx
                .send(Ok(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    block_delta
                ))))
                .await;
        }
        ReasoningReplayMode::Separate => {
            ensure_message_start(sent_start, msg_id, req_model, tx).await;
            close_text_if_needed(text_open, content_block_index, tx).await;
            emit_thinking_block_start(thinking_open, *content_block_index, tx).await;
            let block_delta = json!({
                "type": "content_block_delta", "index": *content_block_index,
                "delta": {"type": "thinking_delta", "thinking": delta}
            });
            let _ = tx
                .send(Ok(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    block_delta
                ))))
                .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_text_delta(
    delta: &str,
    sent_start: &mut bool,
    msg_id: &str,
    req_model: &str,
    thinking_open: &mut bool,
    text_open: &mut bool,
    content_block_index: &mut u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if delta.is_empty() {
        return;
    }
    ensure_message_start(sent_start, msg_id, req_model, tx).await;
    close_thinking_if_needed(thinking_open, content_block_index, tx).await;
    emit_text_block_start(text_open, *content_block_index, tx).await;
    let block_delta = json!({
        "type": "content_block_delta", "index": *content_block_index,
        "delta": {"type": "text_delta", "text": delta}
    });
    let _ = tx
        .send(Ok(Bytes::from(format!(
            "event: content_block_delta\ndata: {}\n\n",
            block_delta
        ))))
        .await;
}

#[allow(clippy::too_many_arguments)]
async fn handle_tool_calls_delta(
    tool_calls: &[Value],
    active_tools: &mut HashMap<u64, ToolCallState>,
    thinking_open: &mut bool,
    text_open: &mut bool,
    content_block_index: &mut u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    if tool_calls.is_empty() {
        return;
    }
    close_thinking_if_needed(thinking_open, content_block_index, tx).await;
    close_text_if_needed(text_open, content_block_index, tx).await;
    for tc in tool_calls {
        let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0);
        let tc_id = tc.get("id").and_then(Value::as_str).map(|s| s.to_string());
        let func = tc.get("function");
        let tc_name = func
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let tc_args = func
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let state = active_tools.entry(idx).or_insert_with(|| ToolCallState {
            id: tc_id.clone().unwrap_or_default(),
            name: tc_name.clone().unwrap_or_default(),
            started: false,
        });
        if let Some(id) = tc_id {
            state.id = id;
        }
        if let Some(name) = tc_name {
            state.name = name;
        }
        let block_idx = *content_block_index + idx;
        if !state.started && !state.id.is_empty() && !state.name.is_empty() {
            let block_start = json!({
                "type": "content_block_start", "index": block_idx,
                "content_block": {"type": "tool_use", "id": state.id.clone(), "name": state.name.clone(), "input": {}}
            });
            let _ = tx
                .send(Ok(Bytes::from(format!(
                    "event: content_block_start\ndata: {}\n\n",
                    block_start
                ))))
                .await;
            state.started = true;
        }
        if !tc_args.is_empty() && state.started {
            let block_delta = json!({
                "type": "content_block_delta", "index": block_idx,
                "delta": {"type": "input_json_delta", "partial_json": tc_args}
            });
            let _ = tx
                .send(Ok(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    block_delta
                ))))
                .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_data_line(
    data_str: &str,
    sent_start: &mut bool,
    sent_stop: &mut bool,
    thinking_open: &mut bool,
    text_open: &mut bool,
    content_block_index: &mut u64,
    active_tools: &mut HashMap<u64, ToolCallState>,
    detected_finish_reason: &mut Option<String>,
    final_usage: &mut Option<Value>,
    msg_id: &str,
    req_model: &str,
    reasoning_mode: ReasoningReplayMode,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) -> bool {
    if data_str == "[DONE]" {
        handle_done_event(
            *sent_start,
            sent_stop,
            thinking_open,
            text_open,
            content_block_index,
            active_tools,
            detected_finish_reason,
            final_usage,
            tx,
        )
        .await;
        return true;
    }
    let chunk_val: Value = match serde_json::from_str(data_str) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if let Some(u) = chunk_val.get("usage") {
        *final_usage = Some(u.clone());
    }
    let choices = chunk_val
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first());
    let delta_obj = choices
        .and_then(|c| c.get("delta"))
        .or_else(|| choices.and_then(|c| c.get("message")));
    let delta_content = delta_obj
        .and_then(|d| d.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let delta_reasoning = delta_obj
        .and_then(|d| d.get("reasoning_content").or_else(|| d.get("reasoning")))
        .and_then(Value::as_str)
        .unwrap_or("");
    let finish_reason = choices
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str);

    handle_reasoning_delta(
        delta_reasoning,
        reasoning_mode,
        sent_start,
        msg_id,
        req_model,
        text_open,
        thinking_open,
        content_block_index,
        tx,
    )
    .await;
    handle_text_delta(
        delta_content,
        sent_start,
        msg_id,
        req_model,
        thinking_open,
        text_open,
        content_block_index,
        tx,
    )
    .await;
    if let Some(tool_calls) = delta_obj
        .and_then(|d| d.get("tool_calls"))
        .and_then(Value::as_array)
    {
        handle_tool_calls_delta(
            tool_calls,
            active_tools,
            thinking_open,
            text_open,
            content_block_index,
            tx,
        )
        .await;
    }
    if let Some(fr) = finish_reason {
        *detected_finish_reason = Some(fr.to_string());
    }
    false
}

async fn handle_done_event(
    sent_start: bool,
    sent_stop: &mut bool,
    thinking_open: &mut bool,
    text_open: &mut bool,
    content_block_index: &mut u64,
    active_tools: &HashMap<u64, ToolCallState>,
    detected_finish_reason: &Option<String>,
    final_usage: &Option<Value>,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) -> bool {
    if !sent_start || *sent_stop {
        return false;
    }
    if *thinking_open {
        finish_thinking_block(*content_block_index, tx).await;
        *thinking_open = false;
        *content_block_index += 1;
    }
    if *text_open {
        let _ = tx
            .send(Ok(Bytes::from(format!(
                "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                *content_block_index
            ))))
            .await;
        *text_open = false;
        *content_block_index += 1;
    }
    finish_active_tools(active_tools, *content_block_index, tx).await;
    let has_tools = !active_tools.is_empty()
        || detected_finish_reason.as_deref() == Some("tool_calls")
        || detected_finish_reason.as_deref() == Some("function_call");
    let stop_rs = if has_tools { "tool_use" } else { "end_turn" };
    let delta_payload = format!(
        "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\",\"stop_sequence\":null}},\"usage\":{}}}\n\n",
        stop_rs,
        build_usage_json(final_usage)
    );
    let _ = tx.send(Ok(Bytes::from(delta_payload))).await;
    let _ = tx
        .send(Ok(Bytes::from(
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        )))
        .await;
    *sent_stop = true;
    true
}

/// 轉換或更新 `convert_stream_inner` 所處理的內容。
async fn convert_stream_inner(
    response: reqwest::Response,
    req_model: String,
    tx: mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
    reasoning_mode: ReasoningReplayMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = response.bytes_stream();
    let mut line_buffer = String::new();
    let msg_id = format!(
        "msg_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis()
    );
    let mut sent_start = false;
    let mut sent_stop = false;
    let mut active_tools: HashMap<u64, ToolCallState> = HashMap::new();
    let mut final_usage: Option<Value> = None;
    let mut text_block_open = false;
    let mut thinking_block_open = false;
    let mut content_block_index: u64 = 0;
    let mut detected_finish_reason: Option<String> = None;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);
        line_buffer.push_str(&text);
        while let Some(pos) = line_buffer.find('\n') {
            let line = line_buffer[..pos].to_string();
            line_buffer = line_buffer[pos + 1..].to_string();
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("data:") {
                let data_str = trimmed.strip_prefix("data:").unwrap().trim();
                let should_break = process_data_line(
                    data_str,
                    &mut sent_start,
                    &mut sent_stop,
                    &mut thinking_block_open,
                    &mut text_block_open,
                    &mut content_block_index,
                    &mut active_tools,
                    &mut detected_finish_reason,
                    &mut final_usage,
                    &msg_id,
                    &req_model,
                    reasoning_mode,
                    &tx,
                )
                .await;
                if should_break {
                    break;
                }
            }
        }
    }
    if sent_start && !sent_stop {
        handle_done_event(
            sent_start,
            &mut sent_stop,
            &mut thinking_block_open,
            &mut text_block_open,
            &mut content_block_index,
            &active_tools,
            &detected_finish_reason,
            &final_usage,
            &tx,
        )
        .await;
    }
    Ok(())
}

/// 執行 `finish_thinking_block` 對應的處理流程。
async fn finish_thinking_block(
    block_idx: u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    let _ = tx
        .send(Ok(Bytes::from(format!(
            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":{},\"delta\":{{\"type\":\"signature_delta\",\"signature\":\"\"}}}}\n\n",
            block_idx
        ))))
        .await;
    let _ = tx
        .send(Ok(Bytes::from(format!(
            "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
            block_idx
        ))))
        .await;
}

/// 執行 `finish_active_tools` 對應的處理流程。
async fn finish_active_tools(
    active_tools: &HashMap<u64, ToolCallState>,
    base_block_idx: u64,
    tx: &mpsc::Sender<Result<Bytes, std::convert::Infallible>>,
) {
    for (&idx, state) in active_tools.iter() {
        if state.started {
            let block_idx = base_block_idx + idx;
            let payload = format!(
                "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                block_idx
            );
            let _ = tx.send(Ok(Bytes::from(payload))).await;
        }
    }
}

#[cfg(test)]
mod tests;
