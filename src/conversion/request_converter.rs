use crate::config::Settings;
use crate::models::claude::{
    ClaudeContentBlock, ClaudeMessageContent, ClaudeMessagesRequest, ClaudeRole, ClaudeSystem,
    ClaudeToolResultContent,
};
use serde_json::{json, Value};

fn thinking_budget_to_effort(budget: u64) -> &'static str {
    if budget > 8192 {
        "max"
    } else if budget > 2048 {
        "high"
    } else if budget > 1024 {
        "medium"
    } else {
        "low"
    }
}

fn effort_rank(effort: &str) -> Option<u8> {
    match effort {
        "none" => Some(0),
        "low" => Some(1),
        "medium" => Some(2),
        "high" => Some(3),
        "max" => Some(4),
        _ => None,
    }
}

fn clamp_reasoning_effort<'a>(requested: &str, supported: &'a [String]) -> Option<&'a str> {
    let requested_rank = effort_rank(requested)?;
    supported
        .iter()
        .filter_map(|effort| effort_rank(effort).map(|rank| (rank, effort.as_str())))
        .filter(|(rank, _)| *rank > 0)
        .min_by_key(|(rank, _)| {
            let distance = (*rank as i16 - requested_rank as i16).abs();
            (distance, std::cmp::Reverse(*rank))
        })
        .map(|(_, effort)| effort)
}

/// 解析請求的 model 名稱，將其映射到適當的真實模型 ID。
pub fn resolve_model_route(requested_model: &str, settings: &Settings) -> Option<String> {
    if requested_model.is_empty() {
        return settings.real_model.clone().filter(|m| !m.trim().is_empty());
    }

    let req_lower = requested_model.to_ascii_lowercase();
    // 算一次 family 供後續兩處使用：家族 override 與模糊匹配 routes。
    let family = if req_lower.contains("sonnet") {
        Some("sonnet")
    } else if req_lower.contains("opus") {
        Some("opus")
    } else if req_lower.contains("haiku") {
        Some("haiku")
    } else {
        None
    };

    // 0. 家族 override：若有為此家族指定的真實模型則直接採用。
    if let Some(fam) = family {
        let family_model = match fam {
            "sonnet" => &settings.real_model_sonnet,
            "opus" => &settings.real_model_opus,
            "haiku" => &settings.real_model_haiku,
            _ => &None,
        };
        if let Some(m) = family_model {
            if !m.trim().is_empty() {
                return Some(m.clone());
            }
        }
    }

    // 1. 精確匹配 routes (例如 "claude-sonnet-4-6[0]" 或 "claude-sonnet-4-6[0][1m]")
    if let Some(mapped) = settings.real_model_routes.get(requested_model) {
        return Some(mapped.clone());
    }

    let clean_model = requested_model.replace("[1m]", "").replace("[1M]", "");
    if let Some(mapped) = settings.real_model_routes.get(&clean_model) {
        return Some(mapped.clone());
    }

    // 2. 若 requested_model 本身（或去除 [1m] 後）在 discovered_models 中，代表直接使用了上游模型名稱
    if settings
        .discovered_models
        .iter()
        .any(|m| m == requested_model || m == &clean_model)
    {
        return Some(clean_model);
    }

    if let Some(start) = clean_model.find('[') {
        let mut depth = 0;
        let mut end_opt = None;
        for (i, c) in clean_model.char_indices().skip(start) {
            if c == '[' {
                depth += 1;
            } else if c == ']' {
                depth -= 1;
                if depth == 0 {
                    end_opt = Some(i);
                    break;
                }
            }
        }
        if let Some(end) = end_opt {
            let inner = &clean_model[start + 1..end];
            if settings.discovered_models.iter().any(|m| m == inner) {
                return Some(inner.to_string());
            }
            if let Some(mapped) = settings.real_model_routes.get(inner) {
                return Some(mapped.clone());
            }
        }
    }

    // 3. 按模型家族（sonnet / opus / haiku）模糊匹配 routes
    if let Some(fam) = family {
        let mut best_key: Option<&String> = None;
        let mut best_val: Option<&String> = None;
        for (k, v) in &settings.real_model_routes {
            if k.to_ascii_lowercase().contains(fam) {
                if let Some(bk) = best_key {
                    if k < bk {
                        best_key = Some(k);
                        best_val = Some(v);
                    }
                } else {
                    best_key = Some(k);
                    best_val = Some(v);
                }
            }
        }
        if let Some(val) = best_val {
            return Some(val.clone());
        }
    }

    // 4. Fallback: 使用使用者設定的全域真實模型名稱
    if let Some(m) = settings.real_model.clone().filter(|m| !m.trim().is_empty()) {
        return Some(m);
    }

    // 5. 安全兜底：如果請求的模型名稱看起來像是一個本地的 alias (含有中括號)，
    //    但我們竟然無法將其映射到任何真實模型，則絕對不能原樣發送（因為上游必定報 400）。
    //    此時我們強制將其映射為我們所知道的任何一個可用上游模型。
    if requested_model.contains('[') && requested_model.contains(']') {
        // (1) 優先使用 routes 裡的任意一個真實模型
        if let Some(m) = settings.real_model_routes.values().next() {
            tracing::warn!(
                "[model 映射安全兜底] 無法解析 alias {}，強制映射為 routes 內的第一個模型 {}",
                requested_model,
                m
            );
            return Some(m.clone());
        }
        // (2) 其次使用 discovered_models 裡的第一個模型
        if let Some(m) = settings.discovered_models.first() {
            tracing::warn!(
                "[model 映射安全兜底] 無法解析 alias {}，強制映射為已偵測的第一個模型 {}",
                requested_model,
                m
            );
            return Some(m.clone());
        }
        // ponytail: 家族模型（sonnet/opus/haiku）在 L50-69 已檢查並 early-return，
        // 此處不可能命中，故不再重複檢查。
    }

    None
}

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
    if let Some(mapped) = resolve_model_route(&req.model, settings) {
        tracing::info!("[model 映射] {} → {}", req.model, mapped);
        data["model"] = Value::String(mapped);
    } else {
        tracing::debug!(
            "[model 映射] {} 不在 routes 中，也沒有預設 model，原樣轉發",
            req.model
        );
    }

    // 處理 thinking 屬性
    if let Some(ref thinking) = req.thinking {
        if thinking.enabled.unwrap_or(true) {
            let budget = thinking.budget_tokens.unwrap_or(1024);
            let effort = thinking_budget_to_effort(budget);
            let target_model = data["model"].as_str().unwrap_or("");
            if let Some(supported) = settings.real_model_reasoning_efforts.get(&req.model) {
                if let Some(effort) = clamp_reasoning_effort(effort, supported) {
                    data["reasoning_effort"] = Value::String(effort.to_string());
                }
            } else if target_model.contains("o1") || target_model.contains("o3") {
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
                                    let mut text_parts = Vec::new();

                                    if let Some(ref res_c) = content {
                                        match res_c {
                                            ClaudeToolResultContent::Text(text) => {
                                                text_parts.push(text.clone());
                                            }
                                            ClaudeToolResultContent::Blocks(arr) => {
                                                for res_block in arr {
                                                    if let Some(text) = res_block
                                                        .get("text")
                                                        .and_then(Value::as_str)
                                                    {
                                                        text_parts.push(text.to_string());
                                                    } else if res_block
                                                        .get("type")
                                                        .and_then(Value::as_str)
                                                        == Some("text")
                                                    {
                                                        if let Some(text) = res_block
                                                            .get("text")
                                                            .and_then(Value::as_str)
                                                        {
                                                            text_parts.push(text.to_string());
                                                        }
                                                    } else if res_block
                                                        .get("type")
                                                        .and_then(Value::as_str)
                                                        == Some("image")
                                                    {
                                                        // tool result 的圖片不轉發。
                                                    } else {
                                                        text_parts.push(res_block.to_string());
                                                    }
                                                }
                                            }
                                            ClaudeToolResultContent::Object(obj) => {
                                                text_parts.push(obj.to_string());
                                            }
                                        }
                                    }

                                    let combined_text = text_parts.join("\n").trim().to_string();

                                    openai_messages.push(json!({
                                        "role": "tool",
                                        "tool_call_id": tool_use_id,
                                        "content": combined_text
                                    }));
                                }
                            }
                            // After tool results, check for after-tools text in the same message
                            let after_tools_text: Vec<String> = next_blocks
                                .iter()
                                .filter_map(|b| {
                                    // Extract text from non-ToolResult blocks that may be after the tools
                                    match b {
                                        ClaudeContentBlock::Text { text } => Some(text.clone()),
                                        _ => None,
                                    }
                                })
                                .collect();
                            if !after_tools_text.is_empty() {
                                let combined_text = after_tools_text.join("\n").trim().to_string();
                                if !combined_text.is_empty() {
                                    openai_messages.push(json!({
                                        "role": "user",
                                        "content": combined_text
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
        data["stop"] = serde_json::to_value(stop_seqs).map_err(|e| e.to_string())?;
    }

    // 轉換 tools
    let mut openai_tools = Vec::new();
    if let Some(ref tools_val) = req.tools {
        for tool in tools_val {
            if !tool.name.trim().is_empty() {
                let Some(input_schema) = tool.input_schema.clone() else {
                    return Err(format!(
                        "Anthropic-native tool '{}' cannot be converted to OpenAI-compatible function calling. Use an Anthropic-compatible gateway for Claude Desktop built-in tools.",
                        tool.name
                    ));
                };
                openai_tools.push(json!({
                    "type": "function",
                    "function": {
                        "name": tool.name.clone(),
                        "description": tool.description.clone().unwrap_or_default(),
                        "parameters": input_schema
                    }
                }));
            }
        }
    }

    if !openai_tools.is_empty() {
        data["tools"] = Value::Array(openai_tools);
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

    let body_str = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    Ok((body_str, is_stream))
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

    #[test]
    fn other_anthropic_native_tools_still_fail_clearly_for_openai_gateways() {
        let settings = Settings::default();
        let body = json!({
            "model": "claude-test",
            "messages": [{"role": "user", "content": "run a command"}],
            "tools": [{
                "type": "bash_20250124",
                "name": "bash"
            }]
        });

        let err = anthropic_to_openai_request(&body.to_string(), &settings).unwrap_err();

        assert!(err.contains("Anthropic-native tool"));
    }

    #[test]
    fn thinking_budget_clamps_to_model_reasoning_effort_levels() {
        let mut routes = std::collections::HashMap::new();
        routes.insert("claude-sonnet-4-6[0]".to_string(), "nim-medium".to_string());
        let mut efforts = std::collections::HashMap::new();
        efforts.insert(
            "claude-sonnet-4-6[0]".to_string(),
            vec!["none".to_string(), "low".to_string(), "medium".to_string()],
        );
        let settings = Settings {
            real_model_routes: routes,
            real_model_reasoning_efforts: efforts,
            ..Settings::default()
        };
        let body = json!({
            "model": "claude-sonnet-4-6[0]",
            "messages": [{"role": "user", "content": "think"}],
            "thinking": {
                "type": "enabled",
                "budget_tokens": 4096
            }
        });

        let (converted, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        let converted: Value = serde_json::from_str(&converted).unwrap();

        assert_eq!(converted["model"], "nim-medium");
        assert_eq!(converted["reasoning_effort"], "medium");
        assert!(converted.get("thinking").is_none());
    }

    #[test]
    fn resolve_model_route_handles_1m_suffix_and_fallbacks() {
        let mut routes = std::collections::HashMap::new();
        routes.insert("claude-sonnet-4-6[0]".to_string(), "nim-medium".to_string());
        routes.insert("claude-opus-4-8[0]".to_string(), "gpt-4o".to_string());

        let settings = Settings {
            real_model_routes: routes,
            discovered_models: vec!["nim-medium".to_string(), "gpt-4o".to_string()],
            real_model: Some("default-model".to_string()),
            ..Settings::default()
        };

        assert_eq!(
            resolve_model_route("claude-sonnet-4-6[0]", &settings),
            Some("nim-medium".to_string())
        );
        assert_eq!(
            resolve_model_route("claude-sonnet-4-6[0][1m]", &settings),
            Some("nim-medium".to_string())
        );
        assert_eq!(
            resolve_model_route("gpt-4o[1m]", &settings),
            Some("gpt-4o".to_string())
        );
    }

    #[test]
    fn resolve_model_route_robust_bracket_and_fuzzy_matching() {
        let mut routes = std::collections::HashMap::new();
        routes.insert("1".to_string(), "nim-one".to_string());
        routes.insert("1[2]".to_string(), "nim-nested".to_string());

        routes.insert("gpt-4o-sonnet".to_string(), "target-gpt-sonnet".to_string());
        routes.insert(
            "claude-3-5-sonnet-real".to_string(),
            "target-claude-sonnet".to_string(),
        );
        routes.insert("z-sonnet".to_string(), "target-z-sonnet".to_string());

        let settings = Settings {
            real_model_routes: routes,
            discovered_models: vec![],
            ..Settings::default()
        };

        assert_eq!(
            resolve_model_route("model[1[2]]", &settings),
            Some("nim-nested".to_string())
        );

        assert_eq!(
            resolve_model_route("model[1][2]", &settings),
            Some("nim-one".to_string())
        );

        assert_eq!(
            resolve_model_route("some-random-sonnet-request", &settings),
            Some("target-claude-sonnet".to_string())
        );
    }

    #[test]
    fn resolve_model_route_applies_fallback_safety_net_for_local_aliases() {
        let mut routes = std::collections::HashMap::new();
        routes.insert("claude-sonnet-4-6[0]".to_string(), "nim-medium".to_string());

        let settings = Settings {
            real_model_routes: routes,
            discovered_models: vec!["gpt-4o".to_string()],
            real_model: None,
            ..Settings::default()
        };

        assert_eq!(
            resolve_model_route("claude-haiku-4-5[2]", &settings),
            Some("nim-medium".to_string())
        );

        let settings_empty_routes = Settings {
            real_model_routes: std::collections::HashMap::new(),
            discovered_models: vec!["gpt-4o".to_string()],
            real_model: None,
            ..Settings::default()
        };
        assert_eq!(
            resolve_model_route("claude-haiku-4-5[2]", &settings_empty_routes),
            Some("gpt-4o".to_string())
        );
    }

    #[test]
    fn resolve_model_route_prefers_family_override_over_dynamic_route() {
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "claude-haiku-4-5[2]".to_string(),
            "diffusiongemma-26b".to_string(),
        );
        let settings = Settings {
            real_model_routes: routes,
            real_model_haiku: Some("nemotron-3-super-120b".to_string()),
            ..Settings::default()
        };

        assert_eq!(
            resolve_model_route("claude-haiku-4-5[2]", &settings),
            Some("nemotron-3-super-120b".to_string())
        );
    }
}
