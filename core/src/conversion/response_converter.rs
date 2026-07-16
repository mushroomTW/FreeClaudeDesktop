use crate::config::Settings;
use crate::models::openai::{
    InferenceModel, NormalizedModel, NormalizedModels, ProviderModel, ProviderModelsResponse,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use url::Url;

fn is_local_hostname(hostname: &str) -> bool {
    matches!(hostname, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// 只允許 Claude Desktop 常見 origin 與同埠 localhost，避免任意網頁打 localhost proxy。
pub fn is_allowed_origin(origin: Option<&str>, port: u16) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    if origin.is_empty() {
        return true;
    }

    if matches!(
        origin,
        "file://" | "null" | "anthropic://desktop" | "app://localhost"
    ) {
        return true;
    }

    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    let is_local = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");
    if is_local && url.port_or_known_default() == Some(port) {
        return true;
    }

    url.scheme() == "https"
        && (host == "claude.ai"
            || host.ends_with(".claude.ai")
            || host == "claude.com"
            || host.ends_with(".claude.com"))
}

fn normalize_gateway_url(base_url: &str, endpoint: &str) -> Result<String, String> {
    let mut target_url =
        Url::parse(base_url.trim()).map_err(|_| "Invalid Gateway Base URL".to_string())?;
    if target_url.scheme() != "https" {
        let is_local = target_url
            .host_str()
            .map(is_local_hostname)
            .unwrap_or(false);
        if !is_local || target_url.scheme() != "http" {
            return Err("Gateway Base URL must use HTTPS, or HTTP on localhost".to_string());
        }
    }

    let base_path = target_url.path().trim_end_matches('/');
    let path = if base_path.ends_with("/v1") {
        format!("{base_path}/{endpoint}")
    } else {
        format!("{base_path}/v1/{endpoint}")
    };
    target_url.set_path(&path);
    target_url.set_query(None);
    target_url.set_fragment(None);
    Ok(target_url.to_string())
}

pub fn normalize_messages_url(base_url: &str) -> Result<String, String> {
    normalize_gateway_url(base_url, "messages")
}

pub fn normalize_models_url(base_url: &str) -> Result<String, String> {
    normalize_gateway_url(base_url, "models")
}

pub fn normalize_model_info_url(base_url: &str) -> Result<String, String> {
    let mut target_url =
        Url::parse(base_url.trim()).map_err(|_| "Invalid Gateway Base URL".to_string())?;
    if target_url.scheme() != "https" {
        let is_local = target_url
            .host_str()
            .map(is_local_hostname)
            .unwrap_or(false);
        if !is_local || target_url.scheme() != "http" {
            return Err("Gateway Base URL must use HTTPS, or HTTP on localhost".to_string());
        }
    }

    let base_path = target_url.path().trim_end_matches('/');
    let base_path = base_path.strip_suffix("/v1").unwrap_or(base_path);
    target_url.set_path(&format!("{base_path}/model/info"));
    target_url.set_query(None);
    target_url.set_fragment(None);
    Ok(target_url.to_string())
}

pub fn normalize_chat_completions_url(base_url: &str) -> Result<String, String> {
    normalize_gateway_url(base_url, "chat/completions")
}

pub fn prepare_proxy_body(body: &str, settings: &Settings) -> String {
    let mut data: Value = match serde_json::from_str(body) {
        Ok(data) => data,
        Err(_) => return body.to_string(),
    };

    if let Some(model) = data.get("model").and_then(Value::as_str) {
        if let Some(mapped) =
            crate::conversion::request_converter::resolve_model_route(model, settings)
        {
            tracing::info!("[model 映射] {} → {}", model, mapped);
            data["model"] = Value::String(mapped);
        } else {
            tracing::debug!(
                "[model 映射] {} 不在 routes 中，也沒有預設 model，原樣轉發",
                model
            );
        }
    } else if let Some(mapped) =
        crate::conversion::request_converter::resolve_model_route("", settings)
    {
        data["model"] = Value::String(mapped);
    }

    serde_json::to_string(&data).unwrap_or_else(|_| body.to_string())
}

pub struct StaleModelRewrite {
    pub updated_body: Value,
    pub fallback_model: String,
}

pub fn rewrite_stale_model_request(
    proxy_body: &str,
    settings: &Settings,
    requested_model: &str,
) -> Option<StaleModelRewrite> {
    let mut data: Value = serde_json::from_str(proxy_body).ok()?;
    let stale_model = data.get("model").and_then(Value::as_str)?.to_string();

    let mut fallback = Vec::new();
    for (alias, model) in &settings.real_model_routes {
        if alias != requested_model && model != &stale_model {
            fallback.push(model.clone());
        }
    }
    if let Some(real_m) = &settings.real_model
        && !real_m.trim().is_empty()
        && real_m != &stale_model
    {
        fallback.push(real_m.clone());
    }
    for disc in &settings.discovered_models {
        if disc != &stale_model && !disc.trim().is_empty() {
            fallback.push(disc.clone());
        }
    }
    fallback.sort();
    fallback.dedup();

    let fallback_model = fallback.into_iter().next()?;

    data["model"] = Value::String(fallback_model.clone());
    Some(StaleModelRewrite {
        updated_body: data,
        fallback_model,
    })
}

fn is_free_model(model: &ProviderModel) -> bool {
    model.id.ends_with(":free")
        || model
            .pricing
            .as_ref()
            .map(|pricing| {
                pricing.prompt.unwrap_or(0.0) == 0.0 && pricing.completion.unwrap_or(0.0) == 0.0
            })
            .unwrap_or(false)
}

fn model_priority(model: &ProviderModel) -> usize {
    if is_free_model(model) { 100 } else { 200 }
}

fn provider_model_id(model: &ProviderModel) -> String {
    if model.id.is_empty() {
        model.model_name.clone().unwrap_or_default()
    } else {
        model.id.clone()
    }
}

fn model_info_u64(model: &ProviderModel, key: &str) -> Option<u64> {
    model
        .model_info
        .as_ref()
        .and_then(|info| info.get(key))
        .and_then(Value::as_u64)
}

fn override_reasoning_levels(level: &str) -> Vec<String> {
    match level {
        "low" => vec!["none".to_string(), "low".to_string()],
        "medium" => vec!["none".to_string(), "medium".to_string()],
        "high" | "" => vec!["none".to_string(), "high".to_string()],
        "max" => vec!["none".to_string(), "high".to_string(), "max".to_string()],
        _ => vec!["none".to_string()],
    }
}

fn effective_reasoning_effort_levels(
    model: &ProviderModel,
    overrides: &HashMap<String, String>,
) -> Vec<String> {
    let provider_id = provider_model_id(model);
    if let Some(level) = overrides.get(&provider_id) {
        return override_reasoning_levels(level);
    }
    reasoning_effort_levels(model)
}

fn model_capabilities(model: &ProviderModel, overrides: &HashMap<String, String>) -> Value {
    if let Some(capabilities) = &model.capabilities {
        return capabilities.clone();
    }

    let effort_levels = effective_reasoning_effort_levels(model, overrides);
    let supports_reasoning_effort = model
        .model_info
        .as_ref()
        .and_then(|info| info.get("supports_reasoning_effort"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_override = overrides.contains_key(&provider_model_id(model));
    let supports_thinking = model
        .model_info
        .as_ref()
        .and_then(|info| info.get("supports_thinking"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || ((supports_reasoning_effort || has_override)
            && effort_levels.iter().any(|level| level != "none"));

    json!({
        "thinking": {
            "supported": supports_thinking,
            "types": {
                "enabled": {
                    "supported": supports_thinking
                }
            },
            "reasoning_effort_levels": effort_levels
        }
    })
}

fn reasoning_effort_levels(model: &ProviderModel) -> Vec<String> {
    model
        .model_info
        .as_ref()
        .and_then(|info| info.get("reasoning_effort_levels"))
        .and_then(Value::as_array)
        .map(|levels| {
            levels
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|level| !level.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn supports_reasoning_effort(model: &ProviderModel, overrides: &HashMap<String, String>) -> bool {
    let provider_id = provider_model_id(model);
    if let Some(level) = overrides.get(&provider_id) {
        return level != "none";
    }

    model
        .model_info
        .as_ref()
        .and_then(|info| info.get("supports_reasoning_effort"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && reasoning_effort_levels(model)
            .iter()
            .any(|level| level != "none")
}

fn model_alias(model: &ProviderModel, index: usize, overrides: &HashMap<String, String>) -> String {
    if supports_reasoning_effort(model, overrides) {
        let levels = effective_reasoning_effort_levels(model, overrides);
        if levels.iter().any(|level| level == "max") {
            format!("claude-opus-4-8_{index}")
        } else {
            format!("claude-sonnet-4-6_{index}")
        }
    } else {
        format!("claude-haiku-4-5_{index}")
    }
}

fn display_name_with_1m_suffix(name: String, is_1m: bool) -> String {
    if !is_1m {
        return name;
    }

    let lower = name.trim_end().to_ascii_lowercase();
    if lower.ends_with(" 1m") || lower.ends_with("-1m") || lower.ends_with("[1m]") {
        name
    } else {
        format!("{name} 1M")
    }
}

pub fn normalize_models_response(provider_response: Value) -> Result<NormalizedModels, String> {
    normalize_models_response_with_overrides(provider_response, &HashMap::new(), &HashMap::new())
}

pub fn normalize_models_response_with_overrides(
    provider_response: Value,
    reasoning_overrides: &HashMap<String, String>,
    m1_overrides: &HashMap<String, bool>,
) -> Result<NormalizedModels, String> {
    let parsed: ProviderModelsResponse =
        serde_json::from_value(provider_response).map_err(|e| e.to_string())?;
    let mut models: Vec<_> = parsed
        .data
        .into_iter()
        .filter(|model| !provider_model_id(model).is_empty())
        .collect();
    models.sort_by(|a, b| {
        model_priority(a).cmp(&model_priority(b)).then_with(|| {
            let a_id = provider_model_id(a);
            let b_id = provider_model_id(b);
            a.name
                .as_deref()
                .unwrap_or(&a_id)
                .cmp(b.name.as_deref().unwrap_or(&b_id))
        })
    });
    models.dedup_by(|a, b| provider_model_id(a) == provider_model_id(b));

    fn model_base_name(name: &str) -> &str {
        for suffix in ["-1m", "-1M", " 1m", " 1M"] {
            if let Some(base) = name.strip_suffix(suffix) {
                return base;
            }
        }
        name
    }

    let base_1m_set: std::collections::HashSet<String> = models
        .iter()
        .filter_map(|m| {
            let pid = provider_model_id(m);
            if m1_overrides.get(&pid).copied().unwrap_or(false) {
                Some(model_base_name(&pid).to_string())
            } else {
                None
            }
        })
        .collect();
    let ids_1m: std::collections::HashSet<String> = models
        .iter()
        .map(provider_model_id)
        .filter(|pid| m1_overrides.get(pid.as_str()).copied().unwrap_or(false))
        .collect();
    if !base_1m_set.is_empty() {
        models.retain(|m| {
            let pid = provider_model_id(m);
            let base = model_base_name(&pid);
            !base_1m_set.contains(base) || ids_1m.contains(&pid)
        });
    }

    let mut reasoning_effort_routes = std::collections::HashMap::new();
    let data: Vec<_> = models
        .into_iter()
        .enumerate()
        .map(|(index, model)| {
            let provider_model_id = provider_model_id(&model);
            let supports_reasoning = supports_reasoning_effort(&model, reasoning_overrides);
            let effort_levels = effective_reasoning_effort_levels(&model, reasoning_overrides);
            // 將 OpenAI/NVIDIA 格式的 unix timestamp 轉成 Anthropic 規範的 RFC 3339 字串。
            // 沒有時維持 epoch fallback，避免 Claude Desktop 拒絕。
            let created_at = model
                .created
                .and_then(unix_secs_to_rfc3339)
                .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string());

            // `max_input_tokens` 是 NVIDIA/OpenAI 直接給的視窗大小；若只給
            // `context_length`（總容量），我們暫且當作輸入視窗，輸出另讀
            // `max_completion_tokens`。兩個都沒有時保持 `None`，讓 Claude Desktop
            // 走預設（200k）做為最低限度的視覺提示。若開啟 1M，至少提升為 1,000,000。
            let raw_max_input = model
                .max_input_tokens
                .or_else(|| model_info_u64(&model, "max_input_tokens"))
                .or(model.context_length);
            let is_1m = m1_overrides
                .get(&provider_model_id)
                .copied()
                .unwrap_or(false)
                || raw_max_input
                    .map(|tokens| tokens >= 1_000_000)
                    .unwrap_or(false);
            let alias = model_alias(&model, index, reasoning_overrides);
            let max_input = if is_1m {
                Some(raw_max_input.unwrap_or(200_000).max(1_000_000))
            } else {
                raw_max_input
            };
            let max_output = model
                .max_output_tokens
                .or_else(|| model_info_u64(&model, "max_output_tokens"))
                .or(model.max_completion_tokens);

            let display_name = model
                .name
                .clone()
                .unwrap_or_else(|| provider_model_id.clone());
            let display_name = display_name_with_1m_suffix(display_name, is_1m);

            let name = display_name.clone();
            // 1M 能力只透過 `supports1m` 表達，避免 discovery 的模型 ID
            // 與 Claude Desktop config 中的模型名稱不一致。
            let id = alias;
            if supports_reasoning {
                reasoning_effort_routes.insert(id.clone(), effort_levels);
            }

            NormalizedModel {
                kind: "model".to_string(),
                id,
                name,
                display_name,
                created_at,
                provider_model_id,
                max_input_tokens: max_input,
                max_tokens: max_output,
                capabilities: model_capabilities(&model, reasoning_overrides),
                supports1m: is_1m.then_some(true),
            }
        })
        .collect();
    let routes = data
        .iter()
        .map(|model| (model.id.clone(), model.provider_model_id.clone()))
        .collect();
    Ok(NormalizedModels {
        first_id: data.first().map(|model| model.id.clone()),
        last_id: data.last().map(|model| model.id.clone()),
        data,
        has_more: false,
        routes,
        reasoning_effort_routes,
    })
}

/// 將秒級 unix timestamp 轉為 RFC 3339 / ISO 8601（毫秒精度，採 UTC）。
/// Anthropic `/v1/models` discovery 的 `created_at` 欄位要這個格式。
fn unix_secs_to_rfc3339(ts: u64) -> Option<String> {
    use std::time::{Duration, UNIX_EPOCH};
    let secs = ts as i64;
    if secs < 0 {
        return None;
    }
    let dt = UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))?;
    let datetime: std::time::SystemTime = dt;
    // 用 SystemTime 透過 std 計算曆法欄位（避免拉 chrono 依賴）。
    let dur = datetime.duration_since(UNIX_EPOCH).ok()?;
    format_rfc3339_utc(dur)
}

/// 由 epoch 之後流逝的 Duration 拼出 `YYYY-MM-DDTHH:MM:SS.mmmZ`。
/// 僅處理 1970~2100 範圍足夠 NVIDIA / OpenRouter 給的 timestamp 使用。
fn format_rfc3339_utc(dur: std::time::Duration) -> Option<String> {
    let total_secs = dur.as_secs();
    let millis = dur.subsec_millis();

    // 自 1970-01-01 起算的「天數」與「當天秒數」
    let days = (total_secs / 86_400) as i64;
    let mut secs_of_day = (total_secs % 86_400) as u32;

    // 時間分量
    let hour = secs_of_day / 3600;
    secs_of_day %= 3600;
    let minute = secs_of_day / 60;
    let second = secs_of_day % 60;

    // 用 Howard Hinnant 的 days_from_civil 演算法把 days → (y, m, d)
    let (year, month, day) = days_from_civil(days)?;

    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
    ))
}

/// Howard Hinnant 的 days_from_civil：自 1970-01-01 起算的 days → 格里高利曆年月日。
/// 範圍限制 1970~2099（超出範圍回 None）。
fn days_from_civil(z: i64) -> Option<(i32, u32, u32)> {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y_final = if m <= 2 { y + 1 } else { y };
    if !(1970..=2099).contains(&y_final) {
        return None;
    }
    Some((y_final as i32, m, d))
}

pub fn build_inference_models(models: &[NormalizedModel]) -> Vec<InferenceModel> {
    models
        .iter()
        .map(|model| InferenceModel {
            name: model.id.clone(),
            label_override: model.name.clone(),
            provider_model_id: model.provider_model_id.clone(),
            display_name: model.display_name.clone(),
            max_input_tokens: model.max_input_tokens,
            max_tokens: model.max_tokens,
            capabilities: model.capabilities.clone(),
            supports1m: model.supports1m,
            transport_type: crate::models::openai::default_transport_type(),
        })
        .collect()
}

/// 依上游模型 ID 套用顯示設定，並同步清除不可見模型的路由與思考能力資料。
/// 未出現在 overrides 的模型預設顯示。
pub fn apply_model_visibility(models: &mut NormalizedModels, overrides: &HashMap<String, bool>) {
    models.data.retain(|model| {
        overrides
            .get(&model.provider_model_id)
            .copied()
            .unwrap_or(true)
    });

    let visible_aliases: std::collections::HashSet<String> =
        models.data.iter().map(|model| model.id.clone()).collect();
    models
        .routes
        .retain(|alias, _| visible_aliases.contains(alias));
    models
        .reasoning_effort_routes
        .retain(|alias, _| visible_aliases.contains(alias));
    models.first_id = models.data.first().map(|model| model.id.clone());
    models.last_id = models.data.last().map(|model| model.id.clone());
}

pub fn openai_to_anthropic_response(openai_body: &str, req_model: &str) -> Result<Value, String> {
    let data: Value = serde_json::from_str(openai_body).map_err(|e| e.to_string())?;

    let first_choice = data
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());

    let message = first_choice.and_then(|choice| choice.get("message"));

    let content_text = message
        .and_then(|msg| msg.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut content_blocks = Vec::new();
    let reasoning_text = message
        .and_then(|msg| {
            msg.get("reasoning_content")
                .or_else(|| msg.get("reasoning"))
        })
        .and_then(Value::as_str)
        .unwrap_or("");
    if !reasoning_text.is_empty() {
        content_blocks.push(json!({
            "type": "thinking",
            "thinking": reasoning_text,
            "signature": ""
        }));
    }
    if !content_text.is_empty() {
        content_blocks.push(json!({
            "type": "text",
            "text": content_text
        }));
    }

    let mut stop_reason = "end_turn";

    // 處理 tool_calls
    if let Some(tool_calls) = message
        .and_then(|msg| msg.get("tool_calls"))
        .and_then(Value::as_array)
    {
        if !tool_calls.is_empty() {
            stop_reason = "tool_use";
        }
        for tc in tool_calls {
            let tc_id = tc.get("id").and_then(Value::as_str).unwrap_or("");
            let tc_name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let tc_args_str = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let tc_args: Value = serde_json::from_str(tc_args_str).unwrap_or(json!({}));

            content_blocks.push(json!({
                "type": "tool_use",
                "id": tc_id,
                "name": tc_name,
                "input": tc_args
            }));
        }
    }

    let finish_reason = first_choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if finish_reason == "tool_calls" || finish_reason == "function_call" {
        stop_reason = "tool_use";
    }

    let input_tokens = data
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = data
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });

    if let Some(usage) = data.get("usage") {
        let cached_tokens = usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if cached_tokens > 0 {
            usage_json["cache_read_input_tokens"] = json!(cached_tokens);
        }
    }

    let msg_id = format!(
        "msg_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis()
    );

    Ok(json!({
        "id": msg_id,
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": req_model,
        "stop_reason": stop_reason,
        "usage": usage_json
    }))
}

#[cfg(test)]
mod tests;
