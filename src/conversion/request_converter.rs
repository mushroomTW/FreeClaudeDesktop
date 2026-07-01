use crate::config::Settings;
use crate::models::claude::{
    ClaudeContentBlock, ClaudeMessageContent, ClaudeMessagesRequest, ClaudeRole, ClaudeSystem,
    ClaudeToolResultContent,
};
use serde_json::{json, Value};

pub fn anthropic_to_openai_request(
    body: &str,
    settings: &Settings,
) -> Result<(String, bool), String> {
    let req: ClaudeMessagesRequest = serde_json::from_str(body).map_err(|e| e.to_string())?;

    let max_toks = req.max_tokens.unwrap_or(4096);
    let mut data = json!({
        "model": req.model.clone(),
        "messages": [],
        "max_tokens": max_toks,
    });

    // 處理 model 映射
    if let Some(mapped) = settings.real_model_routes.get(&req.model) {
        tracing::info!("[model 映射] {} → {}", req.model, mapped);
        data["model"] = Value::String(mapped.clone());
    } else if let Some(model) = &settings.real_model {
        tracing::warn!("[model 映射] {} 不在 routes 中，使用預設 model: {}", req.model, model);
        data["model"] = Value::String(model.clone());
    } else {
        tracing::debug!("[model 映射] {} 不在 routes 中，也沒有預設 model，原樣轉發", req.model);
    }

    // 處理 thinking 屬性
    if let Some(ref thinking) = req.thinking {
        if thinking.enabled.unwrap_or(true) {
            let budget = thinking.budget_tokens.unwrap_or(1024);
            let effort = if budget > 2048 {
                "high"
            } else if budget > 1024 {
                "medium"
            } else {
                "low"
            };
            let target_model = data["model"].as_str().unwrap_or("");
            if target_model.contains("o1") || target_model.contains("o3") {
                data["reasoning_effort"] = Value::String(effort.to_string());
            }
        }
    }

    // 處理 system prompt（對齊 Python 專案的 System 處理）
    let mut openai_messages = Vec::new();
    if let Some(ref system) = req.system {
        let system_text = match system {
            ClaudeSystem::Text(s) => s.trim().to_string(),
            ClaudeSystem::Blocks(blocks) => {
                let parts: Vec<String> = blocks
                    .iter()
                    .map(|block| block.text.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                parts.join("\n\n")
            }
        };
        if !system_text.is_empty() {
            openai_messages.push(json!({
                "role": "system",
                "content": system_text
            }));
        }
    }

    // 轉換 messages
    let mut i = 0;
    while i < req.messages.len() {
        let msg = &req.messages[i];

        if msg.role == ClaudeRole::Assistant {
            let mut thinking_content = String::new();
            let mut text_content = String::new();
            let mut tool_calls = Vec::new();

            match &msg.content {
                ClaudeMessageContent::Text(text) => {
                    text_content.push_str(text);
                }
                ClaudeMessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ClaudeContentBlock::Text { text } => {
                                text_content.push_str(text);
                            }
                            ClaudeContentBlock::Thinking { thinking } => {
                                thinking_content.push_str(thinking);
                            }
                            ClaudeContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
                                    }
                                }));
                            }
                            _ => {}
                        }
                    }
                }
            }

            // 組合 content
            let final_content = if !thinking_content.is_empty() {
                format!("<think>{}</think>{}", thinking_content, text_content)
            } else {
                text_content
            };

            let mut openai_msg = json!({
                "role": "assistant"
            });
            if !final_content.is_empty() {
                openai_msg["content"] = Value::String(final_content);
            } else {
                openai_msg["content"] = Value::Null;
            }
            if !tool_calls.is_empty() {
                openai_msg["tool_calls"] = Value::Array(tool_calls);
            }
            openai_messages.push(openai_msg);

            // 緊接著檢查下一個訊息是否是 user 訊息且內含 tool_result
            if i + 1 < req.messages.len() {
                let next_msg = &req.messages[i + 1];
                if next_msg.role == ClaudeRole::User {
                    if let ClaudeMessageContent::Blocks(ref next_blocks) = next_msg.content {
                        let has_tool_result = next_blocks
                            .iter()
                            .any(|b| matches!(b, ClaudeContentBlock::ToolResult { .. }));

                        if has_tool_result {
                            for block in next_blocks {
                                if let ClaudeContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                } = block
                                {
                                    let mut result_content = String::new();
                                    if let Some(ref res_c) = content {
                                        match res_c {
                                            ClaudeToolResultContent::Text(text) => {
                                                result_content.push_str(text);
                                            }
                                            ClaudeToolResultContent::Blocks(arr) => {
                                                for res_block in arr {
                                                    if let Some(text) = res_block
                                                        .get("text")
                                                        .and_then(Value::as_str)
                                                    {
                                                        result_content.push_str(text);
                                                    } else if res_block
                                                        .get("type")
                                                        .and_then(Value::as_str)
                                                        == Some("text")
                                                    {
                                                        if let Some(text) = res_block
                                                            .get("text")
                                                            .and_then(Value::as_str)
                                                        {
                                                            result_content.push_str(text);
                                                        }
                                                    } else {
                                                        result_content
                                                            .push_str(&res_block.to_string());
                                                    }
                                                }
                                            }
                                            ClaudeToolResultContent::Object(obj) => {
                                                result_content.push_str(&obj.to_string());
                                            }
                                        }
                                    }

                                    openai_messages.push(json!({
                                        "role": "tool",
                                        "tool_call_id": tool_use_id,
                                        "content": result_content
                                    }));
                                }
                            }
                            i += 1; // 跳過此 user 訊息，因為它的 tool_result 已經被處理了
                        }
                    }
                }
            }
        } else {
            // role == "user" or "system"
            let mut openai_content = Vec::new();
            let mut has_image = false;

            match &msg.content {
                ClaudeMessageContent::Text(text) => {
                    openai_content.push(json!({
                        "type": "text",
                        "text": text.clone()
                    }));
                }
                ClaudeMessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ClaudeContentBlock::Text { text } => {
                                openai_content.push(json!({
                                    "type": "text",
                                    "text": text.clone()
                                }));
                            }
                            ClaudeContentBlock::Image { source }
                                if source.source_type == "base64" =>
                            {
                                openai_content.push(json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": format!(
                                            "data:{};base64,{}",
                                            source.media_type, source.data
                                        )
                                    }
                                }));
                                has_image = true;
                            }
                            _ => {}
                        }
                    }
                }
            }

            let final_openai_content = if has_image {
                Value::Array(openai_content)
            } else {
                let mut combined_text = String::new();
                for item in &openai_content {
                    if let Some(t) = item.get("text").and_then(Value::as_str) {
                        combined_text.push_str(t);
                    }
                }
                Value::String(combined_text)
            };

            let role_to_send = if msg.role == ClaudeRole::System {
                "system"
            } else {
                "user"
            };
            let clean_user_text = match &final_openai_content {
                Value::String(s) => s.clone(),
                _ => String::new(),
            };
            let clean_user_text = clean_user_text.trim().to_string();

            if has_image {
                openai_messages.push(json!({
                    "role": role_to_send,
                    "content": final_openai_content
                }));
            } else if !clean_user_text.is_empty() {
                openai_messages.push(json!({
                    "role": role_to_send,
                    "content": clean_user_text
                }));
            }
        }
        i += 1;
    }

    data["messages"] = Value::Array(openai_messages);

    // 轉換 stop_sequences
    if let Some(ref stop_seqs) = req.stop_sequences {
        data["stop"] = serde_json::to_value(stop_seqs).unwrap();
    }

    // 轉換 tools
    if let Some(ref tools_val) = req.tools {
        let mut openai_tools = Vec::new();
        for tool in tools_val {
            if !tool.name.trim().is_empty() {
                openai_tools.push(json!({
                    "type": "function",
                    "function": {
                        "name": tool.name.clone(),
                        "description": tool.description.clone().unwrap_or_default(),
                        "parameters": tool.input_schema.clone()
                    }
                }));
            }
        }
        if !openai_tools.is_empty() {
            data["tools"] = Value::Array(openai_tools);
        }
    }

    // 轉換 tool_choice
    if let Some(ref tool_choice_val) = req.tool_choice {
        if let Some(choice_type) = tool_choice_val.get("type").and_then(Value::as_str) {
            let new_choice = match choice_type {
                "auto" => json!("auto"),
                "any" => json!("required"),
                "tool" => {
                    if let Some(name) = tool_choice_val.get("name").and_then(Value::as_str) {
                        json!({
                            "type": "function",
                            "function": { "name": name }
                        })
                    } else {
                        json!("auto")
                    }
                }
                _ => json!("auto"),
            };
            data["tool_choice"] = new_choice;
        }
    }

    let is_stream = req.stream.unwrap_or(false);
    if is_stream {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("stream".to_string(), json!(true));
            obj.insert(
                "stream_options".to_string(),
                json!({
                    "include_usage": true
                }),
            );
        }
    }

    if let Some(temp) = req.temperature {
        data["temperature"] = json!(temp);
    }
    if let Some(top_p) = req.top_p {
        data["top_p"] = json!(top_p);
    }

    Ok((serde_json::to_string(&data).unwrap(), is_stream))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_mapping() {
        let settings = Settings {
            real_base_url: "https://openrouter.ai/api".to_string(),
            ..Default::default()
        };
        // Verify User and Assistant roles map correctly
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                },
                {
                    "role": "assistant",
                    "content": "Hi there!"
                }
            ]
        });

        let (converted, is_stream) =
            anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        assert!(!is_stream);
        let val: Value = serde_json::from_str(&converted).unwrap();
        let msgs = val["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "Hi there!");
    }

    #[test]
    fn test_system_prompt_handling() {
        let settings = Settings::default();
        // Test system prompt as a single string
        let body = json!({
            "model": "claude-3-5-sonnet",
            "system": "You are a helpful assistant.",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                }
            ]
        });

        let (converted, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        let val: Value = serde_json::from_str(&converted).unwrap();
        let msgs = val["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are a helpful assistant.");
    }
}
