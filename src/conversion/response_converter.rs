use crate::config::Settings;
use crate::models::openai::{
    InferenceModel, NormalizedModel, NormalizedModels, ProviderModel, ProviderModelsResponse,
};
use serde_json::{json, Value};
use url::Url;

fn is_local_hostname(hostname: &str) -> bool {
    matches!(hostname, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Proxy 只綁定 127.0.0.1，不存在跨域風險。
/// Claude Desktop (Electron) 會帶各種 Origin（如 `https://claude.ai`、
/// `anthropic://desktop`、`file://` 等），一律放行確保回應不被 CORS 攔截。
pub fn is_allowed_origin(origin: Option<&str>, _port: u16) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    // 本地 proxy 對任何 Origin 都放行
    if origin.is_empty() {
        return true;
    }
    // ponytail: 原本只允許 http://localhost:{port}，但 Claude Desktop 帶的 Origin
    // 不是這個格式，造成回應被 CORS 靜默丟棄 → gateway timeout。全放行。
    true
}

fn normalize_gateway_url(base_url: &str, endpoint: &str) -> Result<String, String> {
  let mut target_url =
    Url::parse(base_url.trim()).map_err(|_| "Invalid Gateway Base URL".to_string())?;
  if target_url.scheme() != "https" {
      let is_local = target_url.host_str().map(is_local_hostname).unwrap_or(false);
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
            tracing::warn!("[model 映射] {} 不在 routes 中，使用預設 model: {}", model, fallback);
            data["model"] = Value::String(fallback.clone());
        } else {
            tracing::debug!("[model 映射] {} 不在 routes 中，也沒有預設 model，原樣轉發", model);
        }
    } else if let Some(model) = &settings.real_model {
        data["model"] = Value::String(model.clone());
    }

    serde_json::to_string(&data).unwrap_or_else(|_| body.to_string())
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

pub fn normalize_models_response(provider_response: Value) -> Result<NormalizedModels, String> {
    let parsed: ProviderModelsResponse =
        serde_json::from_value(provider_response).map_err(|e| e.to_string())?;
    let mut models: Vec<_> = parsed
        .data
        .into_iter()
        .filter(|model| !model.id.is_empty())
        .collect();
    models.sort_by(|a, b| {
        model_priority(a).cmp(&model_priority(b)).then_with(|| {
            a.name
                .as_deref()
                .unwrap_or(&a.id)
                .cmp(b.name.as_deref().unwrap_or(&b.id))
        })
    });

    let data: Vec<_> = models
        .into_iter()
        .enumerate()
        .map(|(index, model)| {
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
            let max_input = model
                .max_input_tokens
                .or(model.context_length);
            let max_output = model
                .max_output_tokens
                .or(model.max_completion_tokens);

            NormalizedModel {
                kind: "model".to_string(),
                id: format!("claude-opus-4-8[{}]", index),
                display_name: model.name.clone().unwrap_or_else(|| model.id.clone()),
                created_at,
                provider_model_id: model.id.clone(),
                max_input_tokens: max_input,
                max_tokens: max_output,
                capabilities: serde_json::json!({
                    "thinking": {
                        "supported": true,
                        "types": {
                            "enabled": {
                                "supported": true
                            }
                        }
                    }
                }),
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
    if y_final < 1970 || y_final > 2099 {
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
        // Non-local origins → also allowed (proxy only binds 127.0.0.1)
        assert!(is_allowed_origin(Some("https://localhost:3000"), 3000));
        assert!(is_allowed_origin(Some("http://localhost:4000"), 3000));
        assert!(is_allowed_origin(Some("https://claude.ai"), 3000));
        assert!(is_allowed_origin(Some("https://evil.example"), 3000));
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


}
