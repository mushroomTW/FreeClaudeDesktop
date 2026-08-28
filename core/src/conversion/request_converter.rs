use crate::config::Settings;
use crate::models::claude::{
    ClaudeContentBlock, ClaudeMessageContent, ClaudeMessagesRequest, ClaudeRole, ClaudeSystem,
    ClaudeToolResultContent,
};
use serde_json::{Value, json};

/// 執行 `thinking_budget_to_effort` 對應的處理流程。
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

/// 執行 `effort_rank` 對應的處理流程。
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

/// 正規化 `clamp_reasoning_effort` 所處理的資料。
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

fn detect_family(req_lower: &str) -> Option<&'static str> {
    if req_lower.contains("sonnet") {
        Some("sonnet")
    } else if req_lower.contains("opus") {
        Some("opus")
    } else if req_lower.contains("haiku") {
        Some("haiku")
    } else {
        None
    }
}

fn family_override_model<'a>(settings: &'a Settings, family: &str) -> Option<&'a String> {
    let m = match family {
        "sonnet" => &settings.models.real_model_sonnet,
        "opus" => &settings.models.real_model_opus,
        "haiku" => &settings.models.real_model_haiku,
        _ => &None,
    };
    m.as_ref().filter(|s| !s.trim().is_empty())
}

fn try_family_override(settings: &Settings, family: Option<&str>) -> Option<String> {
    let fam = family?;
    let m = family_override_model(settings, fam)?;
    tracing::debug!("[model 映射] 家族 {} 覆寫 → {}", fam, m);
    Some(m.clone())
}

fn try_exact_routes(
    requested_model: &str,
    clean_model: &str,
    settings: &Settings,
) -> Option<String> {
    if let Some(m) = settings.models.real_model_routes.get(requested_model) {
        return Some(m.clone());
    }
    if let Some(m) = settings.models.real_model_routes.get(clean_model) {
        return Some(m.clone());
    }
    None
}

fn try_discovered_direct(
    requested_model: &str,
    clean_model: &str,
    settings: &Settings,
) -> Option<String> {
    let found = settings
        .models
        .discovered_models
        .iter()
        .any(|m| m == requested_model || m == clean_model);
    if found {
        Some(clean_model.to_string())
    } else {
        None
    }
}

fn extract_bracket_inner(clean_model: &str) -> Option<String> {
    let start = clean_model.find('[')?;
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
    let end = end_opt?;
    Some(clean_model[start + 1..end].to_string())
}

fn try_bracket_inner_route(clean_model: &str, settings: &Settings) -> Option<String> {
    let inner = extract_bracket_inner(clean_model)?;
    if settings
        .models
        .discovered_models
        .iter()
        .any(|m| m == &inner)
    {
        return Some(inner);
    }
    settings.models.real_model_routes.get(&inner).cloned()
}

fn try_fuzzy_family(family: Option<&str>, settings: &Settings) -> Option<String> {
    let fam = family?;
    let mut best_key: Option<&String> = None;
    let mut best_val: Option<&String> = None;
    for (k, v) in &settings.models.real_model_routes {
        if k.to_ascii_lowercase().contains(fam) {
            let is_better = best_key.is_none_or(|bk| k < bk);
            if is_better {
                best_key = Some(k);
                best_val = Some(v);
            }
        }
    }
    best_val.cloned()
}

fn fallback_global_model(settings: &Settings) -> Option<String> {
    settings
        .models
        .real_model
        .clone()
        .filter(|m| !m.trim().is_empty())
}

fn is_indexed_claude_alias(requested_model: &str) -> bool {
    ["-", "_"].iter().any(|sep| {
        requested_model
            .rsplit_once(sep)
            .is_some_and(|(prefix, index)| {
                prefix.starts_with("claude-") && index.parse::<usize>().is_ok()
            })
    })
}

fn safety_net_route(requested_model: &str, settings: &Settings) -> Option<String> {
    let needs_net = (requested_model.contains('[') && requested_model.contains(']'))
        || is_indexed_claude_alias(requested_model);
    if !needs_net {
        return None;
    }
    if let Some(m) = settings.models.real_model_routes.values().min() {
        tracing::warn!(
            "[model 映射安全兜底] 無法解析 alias {}，強制映射為 routes 內的第一個模型 {}",
            requested_model,
            m
        );
        return Some(m.clone());
    }
    if let Some(m) = settings.models.discovered_models.first() {
        tracing::warn!(
            "[model 映射安全兜底] 無法解析 alias {}，強制映射為已偵測的第一個模型 {}",
            requested_model,
            m
        );
        return Some(m.clone());
    }
    None
}

/// 解析請求的 model 名稱，將其映射到適當的真實模型 ID。
pub fn resolve_model_route(requested_model: &str, settings: &Settings) -> Option<String> {
    if requested_model.is_empty() {
        return fallback_global_model(settings);
    }
    let req_lower = requested_model.to_ascii_lowercase();
    let family = detect_family(&req_lower);
    if let Some(m) = try_family_override(settings, family) {
        return Some(m);
    }
    let clean_model = requested_model
        .strip_suffix("[1m]")
        .or_else(|| requested_model.strip_suffix("[1M]"))
        .unwrap_or(requested_model);
    if let Some(m) = try_exact_routes(requested_model, clean_model, settings) {
        return Some(m);
    }
    if let Some(m) = try_discovered_direct(requested_model, clean_model, settings) {
        return Some(m);
    }
    if let Some(m) = try_bracket_inner_route(clean_model, settings) {
        return Some(m);
    }
    if let Some(m) = try_fuzzy_family(family, settings) {
        return Some(m);
    }
    if let Some(m) = fallback_global_model(settings) {
        return Some(m);
    }
    safety_net_route(requested_model, settings)
}

fn apply_model_mapping(data: &mut Value, req_model: &str, settings: &Settings) {
    if let Some(mapped) = resolve_model_route(req_model, settings) {
        tracing::info!("[model 映射] {} → {}", req_model, mapped);
        data["model"] = Value::String(mapped);
    } else {
        tracing::debug!(
            "[model 映射] {} 不在 routes 中，也沒有預設 model，原樣轉發",
            req_model
        );
    }
}

fn apply_thinking_mapping(data: &mut Value, req: &ClaudeMessagesRequest, settings: &Settings) {
    let Some(ref thinking) = req.thinking else {
        return;
    };
    if !thinking.enabled.unwrap_or(true) {
        return;
    }
    let budget = thinking.budget_tokens.unwrap_or(1024);
    let effort = thinking_budget_to_effort(budget);
    let target_model = data["model"].as_str().unwrap_or("");
    if let Some(supported) = settings.models.real_model_reasoning_efforts.get(&req.model) {
        if let Some(e) = clamp_reasoning_effort(effort, supported) {
            data["reasoning_effort"] = Value::String(e.to_string());
        }
    } else if target_model.contains("o1") || target_model.contains("o3") {
        data["reasoning_effort"] = Value::String(effort.to_string());
    }
}

fn extract_system_text(system: &ClaudeSystem) -> Option<String> {
    let text = match system {
        ClaudeSystem::Text(s) => s.trim().to_string(),
        ClaudeSystem::Blocks(blocks) => {
            let parts: Vec<String> = blocks
                .iter()
                .map(|b| b.text.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            parts.join("\n\n")
        }
    };
    if text.is_empty() { None } else { Some(text) }
}

fn build_system_message(system: &Option<ClaudeSystem>) -> Option<Value> {
    let sys = system.as_ref()?;
    let text = extract_system_text(sys)?;
    Some(json!({"role": "system", "content": text}))
}

fn collect_assistant_parts(content: &ClaudeMessageContent) -> (String, String, Vec<Value>) {
    let mut thinking = String::new();
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    match content {
        ClaudeMessageContent::Text(t) => text.push_str(t),
        ClaudeMessageContent::Blocks(blocks) => {
            for block in blocks {
                match block {
                    ClaudeContentBlock::Text { text: t } => text.push_str(t),
                    ClaudeContentBlock::Thinking { thinking: th } => thinking.push_str(th),
                    ClaudeContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(json!({
                            "id": id, "type": "function",
                            "function": {"name": name, "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())}
                        }));
                    }
                    _ => {}
                }
            }
        }
    }
    (thinking, text, tool_calls)
}

fn build_assistant_message(thinking: String, text: String, tool_calls: Vec<Value>) -> Value {
    let final_content = if !thinking.is_empty() {
        format!("<think>{}</think>{}", thinking, text)
    } else {
        text
    };
    let mut msg = json!({"role": "assistant"});
    if !final_content.is_empty() {
        msg["content"] = Value::String(final_content);
    } else {
        msg["content"] = Value::Null;
    }
    if !tool_calls.is_empty() {
        msg["tool_calls"] = Value::Array(tool_calls);
    }
    msg
}

fn tool_result_content_to_text(content: &Option<ClaudeToolResultContent>) -> String {
    let Some(res_c) = content else {
        return String::new();
    };
    match res_c {
        ClaudeToolResultContent::Text(t) => t.clone(),
        ClaudeToolResultContent::Object(obj) => obj.to_string(),
        ClaudeToolResultContent::Blocks(arr) => {
            let mut parts = Vec::new();
            for res_block in arr {
                if let Some(t) = res_block.get("text").and_then(Value::as_str) {
                    parts.push(t.to_string());
                } else if res_block.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = res_block.get("text").and_then(Value::as_str) {
                        parts.push(t.to_string());
                    }
                } else if res_block.get("type").and_then(Value::as_str) != Some("image") {
                    parts.push(res_block.to_string());
                }
            }
            parts.join("\n")
        }
    }
}

fn build_tool_messages(blocks: &[ClaudeContentBlock]) -> Vec<Value> {
    let mut out = Vec::new();
    for block in blocks {
        if let ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
        } = block
        {
            let text = tool_result_content_to_text(content);
            let combined = text.trim().to_string();
            out.push(json!({"role": "tool", "tool_call_id": tool_use_id, "content": combined}));
        }
    }
    out
}

fn extract_after_tools_user_text(blocks: &[ClaudeContentBlock]) -> Option<Value> {
    let texts: Vec<String> = blocks
        .iter()
        .filter_map(|b| match b {
            ClaudeContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    if texts.is_empty() {
        return None;
    }
    let combined = texts.join("\n").trim().to_string();
    if combined.is_empty() {
        None
    } else {
        Some(json!({"role": "user", "content": combined}))
    }
}

fn convert_user_blocks(blocks: &[ClaudeContentBlock]) -> (Vec<Value>, bool) {
    let mut content = Vec::new();
    let mut has_image = false;
    for block in blocks {
        match block {
            ClaudeContentBlock::Text { text } => {
                content.push(json!({"type": "text", "text": text.clone()}));
            }
            ClaudeContentBlock::Image { source } if source.source_type == "base64" => {
                content.push(json!({
                    "type": "image_url",
                    "image_url": {"url": format!("data:{};base64,{}", source.media_type, source.data)}
                }));
                has_image = true;
            }
            _ => {}
        }
    }
    (content, has_image)
}

fn role_string(role: &ClaudeRole) -> &'static str {
    if *role == ClaudeRole::System {
        "system"
    } else {
        "user"
    }
}

fn combined_text_from_content(openai_content: &[Value]) -> String {
    let mut combined = String::new();
    for item in openai_content {
        if let Some(t) = item.get("text").and_then(Value::as_str) {
            combined.push_str(t);
        }
    }
    combined.trim().to_string()
}

fn convert_single_user_message(msg: &crate::models::claude::ClaudeMessage) -> Option<Value> {
    match &msg.content {
        ClaudeMessageContent::Text(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(json!({"role": "user", "content": trimmed.to_string()}))
            }
        }
        ClaudeMessageContent::Blocks(blocks) => {
            let (openai_content, has_image) = convert_user_blocks(blocks);
            let role = role_string(&msg.role);
            if has_image {
                Some(json!({"role": role, "content": Value::Array(openai_content)}))
            } else {
                let trimmed = combined_text_from_content(&openai_content);
                if trimmed.is_empty() {
                    None
                } else {
                    Some(json!({"role": role, "content": trimmed}))
                }
            }
        }
    }
}

fn handle_assistant_message(msg: &crate::models::claude::ClaudeMessage, out: &mut Vec<Value>) {
    let (thinking, text, tool_calls) = collect_assistant_parts(&msg.content);
    out.push(build_assistant_message(thinking, text, tool_calls));
}

fn try_handle_tool_followup(
    messages: &[crate::models::claude::ClaudeMessage],
    i: &mut usize,
    out: &mut Vec<Value>,
) {
    if *i + 1 >= messages.len() {
        return;
    }
    let next = &messages[*i + 1];
    if next.role != ClaudeRole::User {
        return;
    }
    let ClaudeMessageContent::Blocks(ref next_blocks) = next.content else {
        return;
    };
    let has_tool = next_blocks
        .iter()
        .any(|b| matches!(b, ClaudeContentBlock::ToolResult { .. }));
    if !has_tool {
        return;
    }
    out.extend(build_tool_messages(next_blocks));
    if let Some(user_msg) = extract_after_tools_user_text(next_blocks) {
        out.push(user_msg);
    }
    *i += 1;
}

fn convert_claude_messages(messages: &[crate::models::claude::ClaudeMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        if msg.role == ClaudeRole::Assistant {
            handle_assistant_message(msg, &mut out);
            try_handle_tool_followup(messages, &mut i, &mut out);
        } else if let Some(m) = convert_single_user_message(msg) {
            out.push(m);
        }
        i += 1;
    }
    out
}

fn build_single_openai_tool(tool: &crate::models::claude::ClaudeTool) -> Result<Value, String> {
    if tool.name.trim().is_empty() {
        return Ok(json!({}));
    }
    let Some(input_schema) = tool.input_schema.clone() else {
        return Err(format!(
            "Anthropic-native tool '{}' cannot be converted to OpenAI-compatible function calling. Use an Anthropic-compatible gateway for Claude Desktop built-in tools.",
            tool.name
        ));
    };
    Ok(json!({
        "type": "function",
        "function": {
            "name": tool.name.clone(),
            "description": tool.description.clone().unwrap_or_default(),
            "parameters": input_schema
        }
    }))
}

fn apply_tools_mapping(data: &mut Value, req: &ClaudeMessagesRequest) -> Result<(), String> {
    let Some(ref tools_val) = req.tools else {
        return Ok(());
    };
    let mut openai_tools = Vec::new();
    for tool in tools_val {
        let v = build_single_openai_tool(tool)?;
        if v.get("type").is_some() {
            openai_tools.push(v);
        }
    }
    if !openai_tools.is_empty() {
        data["tools"] = Value::Array(openai_tools);
    }
    Ok(())
}

fn map_tool_choice_type(choice_type: &str, tool_choice_val: &Value) -> Value {
    match choice_type {
        "auto" => json!("auto"),
        "any" => json!("required"),
        "tool" => {
            if let Some(name) = tool_choice_val.get("name").and_then(Value::as_str) {
                json!({"type": "function", "function": {"name": name}})
            } else {
                json!("auto")
            }
        }
        _ => json!("auto"),
    }
}

fn apply_tool_choice_mapping(data: &mut Value, tool_choice: &Option<Value>) {
    let Some(val) = tool_choice else {
        return;
    };
    let Some(choice_type) = val.get("type").and_then(Value::as_str) else {
        return;
    };
    let new_choice = map_tool_choice_type(choice_type, val);
    data["tool_choice"] = new_choice;
}

/// 執行 `anthropic_to_openai_request` 對應的處理流程。
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

    apply_model_mapping(&mut data, &req.model, settings);
    apply_thinking_mapping(&mut data, &req, settings);

    let mut openai_messages = Vec::new();
    if let Some(msg) = build_system_message(&req.system) {
        openai_messages.push(msg);
    }

    openai_messages.extend(convert_claude_messages(&req.messages));
    data["messages"] = Value::Array(openai_messages);

    if let Some(ref stop_seqs) = req.stop_sequences {
        data["stop"] = serde_json::to_value(stop_seqs).map_err(|e| e.to_string())?;
    }

    apply_tools_mapping(&mut data, &req)?;
    apply_tool_choice_mapping(&mut data, &req.tool_choice);

    let is_stream = req.stream.unwrap_or(false);
    if is_stream && let Some(obj) = data.as_object_mut() {
        obj.insert("stream".to_string(), json!(true));
        obj.insert(
            "stream_options".to_string(),
            json!({
                "include_usage": true
            }),
        );
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
mod tests;
