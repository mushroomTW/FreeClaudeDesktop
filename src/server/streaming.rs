use axum::body::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
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

    let get_usage_str = |u: &Option<Value>| -> String {
        let mut usage_json = json!({
            "input_tokens": 0,
            "output_tokens": 0
        });
        if let Some(ref val) = u {
            let input = val
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let output = val
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            usage_json["input_tokens"] = json!(input);
            usage_json["output_tokens"] = json!(output);

            let cached = val
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if cached > 0 {
                usage_json["cache_read_input_tokens"] = json!(cached);
            }
        }
        serde_json::to_string(&usage_json)
            .unwrap_or_else(|_| "{\"input_tokens\":0,\"output_tokens\":0}".to_string())
    };

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
                if data_str == "[DONE]" {
                    if sent_start && !sent_stop {
                        if thinking_block_open {
                            finish_thinking_block(content_block_index, &tx).await;
                            thinking_block_open = false;
                            content_block_index += 1;
                        }
                        if text_block_open {
                            let _ = tx.send(Ok(Bytes::from(format!(
                                "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                                content_block_index
                            )))).await;
                            text_block_open = false;
                            content_block_index += 1;
                        }

                        finish_active_tools(&active_tools, content_block_index, &tx).await;

                        let has_tools = !active_tools.is_empty();
                        let stop_rs = if has_tools { "tool_use" } else { "end_turn" };
                        let delta_payload = format!(
                            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\",\"stop_sequence\":null}},\"usage\":{}}}\n\n",
                            stop_rs,
                            get_usage_str(&final_usage)
                        );
                        let _ = tx.send(Ok(Bytes::from(delta_payload))).await;
                        let _ = tx
                            .send(Ok(Bytes::from(
                                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                            )))
                            .await;
                        sent_stop = true;
                    }
                    break;
                }

                let chunk_val: Value = match serde_json::from_str(data_str) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if let Some(usage_val) = chunk_val.get("usage") {
                    final_usage = Some(usage_val.clone());
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

                if !delta_reasoning.is_empty() {
                    match reasoning_mode {
                        ReasoningReplayMode::Inline => {
                            // Inline mode: wrap reasoning as regular text with <antThinking> tags
                            if !sent_start {
                                let start_msg = json!({
                                    "type": "message_start",
                                    "message": {
                                        "id": msg_id.clone(),
                                        "type": "message",
                                        "role": "assistant",
                                        "content": [],
                                        "model": req_model.clone(),
                                        "stop_reason": null,
                                        "usage": { "input_tokens": 0, "output_tokens": 0 }
                                    }
                                });
                                let _ = tx
                                    .send(Ok(Bytes::from(format!(
                                        "event: message_start\ndata: {}\n\n",
                                        start_msg
                                    ))))
                                    .await;
                                sent_start = true;
                            }

                            if !text_block_open {
                                let block_start = json!({
                                    "type": "content_block_start",
                                    "index": content_block_index,
                                    "content_block": { "type": "text", "text": "" }
                                });
                                let _ = tx
                                    .send(Ok(Bytes::from(format!(
                                        "event: content_block_start\ndata: {}\n\n",
                                        block_start
                                    ))))
                                    .await;
                                text_block_open = true;
                            }

                            let inline_reasoning =
                                format!("<antThinking>{}</antThinking>", delta_reasoning);
                            let block_delta = json!({
                                "type": "content_block_delta",
                                "index": content_block_index,
                                "delta": {
                                    "type": "text_delta",
                                    "text": inline_reasoning
                                }
                            });
                            let _ = tx
                                .send(Ok(Bytes::from(format!(
                                    "event: content_block_delta\ndata: {}\n\n",
                                    block_delta
                                ))))
                                .await;
                        }
                        ReasoningReplayMode::Separate => {
                            // Separate mode: emit thinking blocks before text blocks
                            if !sent_start {
                                let start_msg = json!({
                                    "type": "message_start",
                                    "message": {
                                        "id": msg_id.clone(),
                                        "type": "message",
                                        "role": "assistant",
                                        "content": [],
                                        "model": req_model.clone(),
                                        "stop_reason": null,
                                        "usage": { "input_tokens": 0, "output_tokens": 0 }
                                    }
                                });
                                let _ = tx
                                    .send(Ok(Bytes::from(format!(
                                        "event: message_start\ndata: {}\n\n",
                                        start_msg
                                    ))))
                                    .await;
                                sent_start = true;
                            }

                            if text_block_open {
                                let _ = tx
                                    .send(Ok(Bytes::from(format!(
                                        "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                                        content_block_index
                                    ))))
                                    .await;
                                text_block_open = false;
                                content_block_index += 1;
                            }

                            if !thinking_block_open {
                                let block_start = json!({
                                    "type": "content_block_start",
                                    "index": content_block_index,
                                    "content_block": { "type": "thinking", "thinking": "", "signature": "" }
                                });
                                let _ = tx
                                    .send(Ok(Bytes::from(format!(
                                        "event: content_block_start\ndata: {}\n\n",
                                        block_start
                                    ))))
                                    .await;
                                thinking_block_open = true;
                            }

                            let block_delta = json!({
                                "type": "content_block_delta",
                                "index": content_block_index,
                                "delta": {
                                    "type": "thinking_delta",
                                    "thinking": delta_reasoning
                                }
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

                // Handle content (TextDelta)
                if !delta_content.is_empty() {
                    if !sent_start {
                        let start_msg = json!({
                            "type": "message_start",
                            "message": {
                                "id": msg_id.clone(),
                                "type": "message",
                                "role": "assistant",
                                "content": [],
                                "model": req_model.clone(),
                                "stop_reason": null,
                                "usage": { "input_tokens": 0, "output_tokens": 0 }
                            }
                        });
                        let _ = tx
                            .send(Ok(Bytes::from(format!(
                                "event: message_start\ndata: {}\n\n",
                                start_msg
                            ))))
                            .await;
                        sent_start = true;
                    }

                    if thinking_block_open {
                        finish_thinking_block(content_block_index, &tx).await;
                        thinking_block_open = false;
                        content_block_index += 1;
                    }

                    if !text_block_open {
                        let block_start = json!({
                            "type": "content_block_start",
                            "index": content_block_index,
                            "content_block": { "type": "text", "text": "" }
                        });
                        let _ = tx
                            .send(Ok(Bytes::from(format!(
                                "event: content_block_start\ndata: {}\n\n",
                                block_start
                            ))))
                            .await;
                        text_block_open = true;
                    }

                    let block_delta = json!({
                        "type": "content_block_delta",
                        "index": content_block_index,
                        "delta": {
                            "type": "text_delta",
                            "text": delta_content
                        }
                    });
                    let _ = tx
                        .send(Ok(Bytes::from(format!(
                            "event: content_block_delta\ndata: {}\n\n",
                            block_delta
                        ))))
                        .await;
                }

                // Handle tool_calls
                if let Some(tool_calls) = delta_obj
                    .and_then(|d| d.get("tool_calls"))
                    .and_then(Value::as_array)
                {
                    if thinking_block_open {
                        finish_thinking_block(content_block_index, &tx).await;
                        thinking_block_open = false;
                        content_block_index += 1;
                    }

                    if text_block_open {
                        let _ = tx.send(Ok(Bytes::from(format!(
                            "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                            content_block_index
                        )))).await;
                        text_block_open = false;
                        content_block_index += 1;
                    }

                    for tc in tool_calls {
                        let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0);
                        let tc_id = tc.get("id").and_then(Value::as_str).map(|s| s.to_string());
                        let function_obj = tc.get("function");
                        let tc_name = function_obj
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .map(|s| s.to_string());
                        let tc_args = function_obj
                            .and_then(|f| f.get("arguments"))
                            .and_then(Value::as_str)
                            .unwrap_or("");

                        let state = active_tools.entry(idx).or_insert_with(|| ToolCallState {
                            id: tc_id.clone().unwrap_or_default(),
                            name: tc_name.clone().unwrap_or_default(),
                            started: false,
                        });

                        if let Some(ref id) = tc_id {
                            state.id = id.clone();
                        }
                        if let Some(ref name) = tc_name {
                            state.name = name.clone();
                        }

                        let block_idx = content_block_index + idx;

                        if !state.started && !state.id.is_empty() && !state.name.is_empty() {
                            let block_start = json!({
                                "type": "content_block_start",
                                "index": block_idx,
                                "content_block": {
                                    "type": "tool_use",
                                    "id": state.id.clone(),
                                    "name": state.name.clone(),
                                    "input": {}
                                }
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
                                "type": "content_block_delta",
                                "index": block_idx,
                                "delta": {
                                    "type": "input_json_delta",
                                    "partial_json": tc_args
                                }
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

                if finish_reason.is_some() {
                    let is_tool_finish = finish_reason == Some("tool_calls")
                        || finish_reason == Some("function_call")
                        || !active_tools.is_empty();
                    let stop_rs = if is_tool_finish {
                        "tool_use"
                    } else {
                        "end_turn"
                    };

                    if sent_start && !sent_stop {
                        if thinking_block_open {
                            finish_thinking_block(content_block_index, &tx).await;
                            thinking_block_open = false;
                            content_block_index += 1;
                        }
                        if text_block_open {
                            let _ = tx.send(Ok(Bytes::from(format!(
                                "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                                content_block_index
                            )))).await;
                            text_block_open = false;
                            content_block_index += 1;
                        }

                        finish_active_tools(&active_tools, content_block_index, &tx).await;

                        let delta_payload = format!(
                            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\",\"stop_sequence\":null}},\"usage\":{}}}\n\n",
                            stop_rs,
                            get_usage_str(&final_usage)
                        );
                        let _ = tx.send(Ok(Bytes::from(delta_payload))).await;
                        let _ = tx
                            .send(Ok(Bytes::from(
                                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                            )))
                            .await;
                        sent_stop = true;
                    }
                    break;
                }
            }
        }
    }

    if sent_start && !sent_stop {
        if thinking_block_open {
            finish_thinking_block(content_block_index, &tx).await;
            content_block_index += 1;
        }
        if text_block_open {
            let _ = tx.send(Ok(Bytes::from(format!(
                "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{}}}\n\n",
                content_block_index
            )))).await;
        }

        finish_active_tools(&active_tools, content_block_index, &tx).await;

        let has_tools = !active_tools.is_empty();
        let stop_rs = if has_tools { "tool_use" } else { "end_turn" };
        let delta_payload = format!(
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{}\",\"stop_sequence\":null}},\"usage\":{}}}\n\n",
            stop_rs,
            get_usage_str(&final_usage)
        );
        let _ = tx.send(Ok(Bytes::from(delta_payload))).await;
        let _ = tx
            .send(Ok(Bytes::from(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            )))
            .await;
    }

    Ok(())
}

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
