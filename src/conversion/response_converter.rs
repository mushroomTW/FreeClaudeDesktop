use crate::config::Settings;
use crate::models::openai::{
    InferenceModel, NormalizedModel, NormalizedModels, ProviderModel, ProviderModelsResponse,
};
use serde_json::{json, Value};
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
        if let Some(mapped) = settings.real_model_routes.get(model) {
            tracing::info!("[model 映射] {} → {}", model, mapped);
            data["model"] = Value::String(mapped.clone());
        } else if let Some(fallback) = &settings.real_model {
            tracing::warn!(
                "[model 映射] {} 不在 routes 中，使用預設 model: {}",
                model,
                fallback
            );
            data["model"] = Value::String(fallback.clone());
        } else {
            tracing::debug!(
                "[model 映射] {} 不在 routes 中，也沒有預設 model，原樣轉發",
                model
            );
        }
    } else if let Some(model) = &settings.real_model {
        data["model"] = Value::String(model.clone());
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

    let mut fallback = settings
        .real_model_routes
        .iter()
        .filter(|(alias, model)| alias.as_str() != requested_model && model.as_str() != stale_model)
        .map(|(_, model)| model.clone())
        .collect::<Vec<_>>();
    fallback.sort();
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
    if is_free_model(model) {
        100
    } else {
        200
    }
}

fn provider_model_id(model: &ProviderModel) -> String {
    if model.id.is_empty() {
        model.model_name.clone().unwrap_or_default()
    } else {
        model.id.clone()
    }
}

fn override_reasoning_levels(level: &str) -> Vec<String> {
    match level {
        "low" => vec!["none".to_string(), "low".to_string()],
        "medium" => vec!["none".to_string(), "medium".to_string()],
        "high" => vec!["none".to_string(), "high".to_string()],
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
            format!("claude-opus-4-8[{index}]")
        } else {
            format!("claude-sonnet-4-6[{index}]")
        }
    } else {
        format!("claude-3-5-haiku[{index}]")
    }
}

pub fn normalize_models_response(provider_response: Value) -> Result<NormalizedModels, String> {
    normalize_models_response_with_overrides(provider_response, &HashMap::new())
}

pub fn normalize_models_response_with_overrides(
    provider_response: Value,
    reasoning_overrides: &HashMap<String, String>,
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

    let mut reasoning_effort_routes = std::collections::HashMap::new();
    let data: Vec<_> = models
        .into_iter()
        .enumerate()
        .map(|(index, model)| {
            let provider_model_id = provider_model_id(&model);
            let alias = model_alias(&model, index, reasoning_overrides);
            let effort_levels = effective_reasoning_effort_levels(&model, reasoning_overrides);
            if supports_reasoning_effort(&model, reasoning_overrides) {
                reasoning_effort_routes.insert(alias.clone(), effort_levels);
            }
            // 將 OpenAI/NVIDIA 格式的 unix timestamp 轉成 Anthropic 規範的 RFC 3339 字串。
            // 沒有時維持 epoch fallback，避免 Claude Desktop 拒絕。
            let created_at = model
                .created
                .and_then(unix_secs_to_rfc3339)
                .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string());

            // `max_input_tokens` 是 NVIDIA/OpenAI 直接給的視窗大小；若只給
            // `context_length`（總容量），我們暫且當作輸入視窗，輸出另讀
            // `max_completion_tokens`。兩個都沒有時保持 `None`，讓 Claude Desktop
            // 走預設（200k）做為最低限度的視覺提示。
            let max_input = model.max_input_tokens.or(model.context_length);
            let max_output = model.max_output_tokens.or(model.max_completion_tokens);

            NormalizedModel {
                kind: "model".to_string(),
                id: alias,
                display_name: model
                    .name
                    .clone()
                    .unwrap_or_else(|| provider_model_id.clone()),
                created_at,
                provider_model_id,
                max_input_tokens: max_input,
                max_tokens: max_output,
                capabilities: model_capabilities(&model, reasoning_overrides),
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
            label_override: model.display_name.clone(),
            provider_model_id: model.provider_model_id.clone(),
            display_name: model.display_name.clone(),
            max_input_tokens: model.max_input_tokens,
            max_tokens: model.max_tokens,
            capabilities: model.capabilities.clone(),
            transport_type: crate::models::openai::default_transport_type(),
        })
        .collect()
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
mod tests {
    use super::*;

    #[test]
    fn cors_origin_allows_only_local_http_port() {
        // No origin → always allowed
        assert!(is_allowed_origin(None, 3000));
        // Local HTTP origins → allowed
        assert!(is_allowed_origin(Some("http://localhost:3000"), 3000));
        assert!(is_allowed_origin(Some("http://127.0.0.1:3000"), 3000));
        assert!(is_allowed_origin(Some("http://[::1]:3000"), 3000));
        // Claude Desktop origins → allowed
        assert!(is_allowed_origin(Some("https://claude.ai"), 3000));
        assert!(is_allowed_origin(Some("https://claude.com"), 3000));
        assert!(is_allowed_origin(Some("https://preview.claude.com"), 3000));
        assert!(is_allowed_origin(Some("app://localhost"), 3000));
        assert!(is_allowed_origin(Some("anthropic://desktop"), 3000));
        assert!(is_allowed_origin(Some("file://"), 3000));
        // Browser origins that are not Claude Desktop → blocked
        assert!(!is_allowed_origin(Some("https://evil.example"), 3000));
        assert!(!is_allowed_origin(Some("http://localhost:4000"), 3000));
    }

    #[test]
    fn empty_tool_calls_do_not_force_tool_use_stop_reason() {
        let openai_res = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "hi",
                    "tool_calls": []
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1
            }
        });

        let converted =
            openai_to_anthropic_response(&openai_res.to_string(), "claude-test").unwrap();

        assert_eq!(converted["stop_reason"], "end_turn");
        assert_eq!(converted["content"][0]["type"], "text");
        assert_eq!(converted["content"][0]["text"], "hi");
    }

    #[test]
    fn models_default_to_no_thinking_capability() {
        let normalized = normalize_models_response(json!({
            "data": [{
                "id": "nemotron-3-super-120b"
            }]
        }))
        .unwrap();

        assert_eq!(
            normalized.data[0].provider_model_id,
            "nemotron-3-super-120b"
        );
        assert_eq!(
            normalized.data[0].capabilities["thinking"]["supported"],
            false
        );
    }

    #[test]
    fn models_use_litellm_model_info_thinking_capability() {
        let normalized = normalize_models_response(json!({
            "data": [{
                "model_name": "claude-native",
                "model_info": {
                    "supports_thinking": true
                }
            }]
        }))
        .unwrap();

        assert_eq!(normalized.data[0].provider_model_id, "claude-native");
        assert_eq!(
            normalized.data[0].capabilities["thinking"]["supported"],
            true
        );
    }

    #[test]
    fn models_store_litellm_reasoning_effort_levels() {
        let normalized = normalize_models_response(json!({
            "data": [{
                "model_name": "nim-high",
                "model_info": {
                    "supports_reasoning_effort": true,
                    "reasoning_effort_levels": ["none", "low", "high"]
                }
            }]
        }))
        .unwrap();

        assert_eq!(
            normalized.reasoning_effort_routes["claude-sonnet-4-6[0]"],
            vec!["none", "low", "high"]
        );
        assert_eq!(normalized.data[0].id, "claude-sonnet-4-6[0]");
        assert_eq!(
            normalized.data[0].capabilities["thinking"]["supported"],
            true
        );
    }

    #[test]
    fn models_with_max_reasoning_use_opus_alias() {
        let normalized = normalize_models_response(json!({
            "data": [{
                "model_name": "deepseek-v4-pro",
                "model_info": {
                    "supports_reasoning_effort": true,
                    "reasoning_effort_levels": ["none", "high", "max"]
                }
            }]
        }))
        .unwrap();

        assert_eq!(normalized.data[0].id, "claude-opus-4-8[0]");
        assert_eq!(
            normalized.reasoning_effort_routes["claude-opus-4-8[0]"],
            vec!["none", "high", "max"]
        );
    }

    #[test]
    fn model_reasoning_override_enables_reasoning_alias() {
        let mut overrides = HashMap::new();
        overrides.insert("glm-5.2".to_string(), "high".to_string());

        let normalized = normalize_models_response_with_overrides(
            json!({
                "data": [{
                    "model_name": "glm-5.2",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                }]
            }),
            &overrides,
        )
        .unwrap();

        assert_eq!(normalized.data[0].id, "claude-sonnet-4-6[0]");
        assert_eq!(
            normalized.reasoning_effort_routes["claude-sonnet-4-6[0]"],
            vec!["none", "high"]
        );
    }

    #[test]
    fn models_without_reasoning_use_anthropic_shaped_alias() {
        let normalized = normalize_models_response(json!({
            "data": [{
                "model_name": "glm-5.2",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            }]
        }))
        .unwrap();

        assert_eq!(normalized.data[0].id, "claude-3-5-haiku[0]");
        assert_eq!(normalized.routes["claude-3-5-haiku[0]"], "glm-5.2");
    }

    #[test]
    fn duplicate_litellm_deployments_are_deduped_by_model_name() {
        let normalized = normalize_models_response(json!({
            "data": [
                {
                    "model_name": "glm-5.2",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "model_name": "glm-5.2",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                }
            ]
        }))
        .unwrap();

        assert_eq!(normalized.data.len(), 1);
        assert_eq!(normalized.data[0].id, "claude-3-5-haiku[0]");
    }

    #[test]
    fn rewrites_stale_mapped_model_to_fallback_route() {
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "claude-opus-4-8[0]".to_string(),
            "deepseek-v4-flash".to_string(),
        );
        routes.insert("claude-opus-4-8[3]".to_string(), "glm-5.1".to_string());
        let settings = Settings {
            real_model_routes: routes,
            ..Settings::default()
        };

        let rewritten = rewrite_stale_model_request(
            r#"{"model":"glm-5.1","messages":[]}"#,
            &settings,
            "claude-opus-4-8[3]",
        )
        .unwrap();

        assert_eq!(rewritten.fallback_model, "deepseek-v4-flash");
        assert_eq!(rewritten.updated_body["model"], "deepseek-v4-flash");
    }
}
