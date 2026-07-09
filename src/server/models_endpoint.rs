use crate::config::{get_launcher_settings, save_launcher_settings};
use crate::conversion::response_converter::{
    build_inference_models, normalize_models_response_with_overrides,
};
use crate::crypto::unprotect_secret;
use crate::models::openai::NormalizedModels;
use axum::{http::HeaderMap, response::IntoResponse, Json};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
struct ModelsCache {
    base_url: String,
    auth_scheme: String,
    reasoning_overrides: std::collections::HashMap<String, String>,
    m1_overrides: std::collections::HashMap<String, bool>,
    models: NormalizedModels,
}

static MODELS_CACHE: OnceLock<Mutex<Option<ModelsCache>>> = OnceLock::new();

fn models_cache() -> &'static Mutex<Option<ModelsCache>> {
    MODELS_CACHE.get_or_init(|| Mutex::new(None))
}

pub fn store_models_cache(
    base_url: &str,
    auth_scheme: &str,
    reasoning_overrides: &std::collections::HashMap<String, String>,
    m1_overrides: &std::collections::HashMap<String, bool>,
    models: &NormalizedModels,
) {
    if let Ok(mut cache) = models_cache().lock() {
        *cache = Some(ModelsCache {
            base_url: base_url.trim().to_string(),
            auth_scheme: auth_scheme.to_string(),
            reasoning_overrides: reasoning_overrides.clone(),
            m1_overrides: m1_overrides.clone(),
            models: models.clone(),
        });
    }
}

fn cached_models(
    base_url: &str,
    auth_scheme: &str,
    reasoning_overrides: &std::collections::HashMap<String, String>,
    m1_overrides: &std::collections::HashMap<String, bool>,
) -> Option<NormalizedModels> {
    let cache = models_cache().lock().ok()?;
    let cache = cache.as_ref()?;
    (cache.base_url == base_url.trim()
        && cache.auth_scheme == auth_scheme
        && &cache.reasoning_overrides == reasoning_overrides
        && &cache.m1_overrides == m1_overrides)
        .then(|| cache.models.clone())
}

pub fn clear_models_cache() {
    if let Ok(mut cache) = models_cache().lock() {
        *cache = None;
    }
}

#[cfg(test)]
fn clear_models_cache_for_tests() {
    clear_models_cache();
}

pub async fn handle_models(headers: HeaderMap) -> impl IntoResponse {
    // 1. Load settings for the configured proxy token.
    let Some(mut settings) = get_launcher_settings() else {
        tracing::error!("<- 錯誤: Launcher 尚未配置");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Launcher has not been configured yet." })),
        )
            .into_response();
    };

    // 2. Validate authorization
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok());
    let x_api_key_header = headers.get("x-api-key").and_then(|h| h.to_str().ok());
    let is_authorized = crate::server::is_authorized_proxy_request(
        auth_header,
        x_api_key_header,
        &settings.proxy_auth_token,
    );
    if !is_authorized {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized" })),
        )
            .into_response();
    }

    if let Some(models) = cached_models(
        &settings.real_base_url,
        &settings.real_auth_scheme,
        &settings.model_reasoning_overrides,
        &settings.model_1m_overrides,
    ) {
        return (axum::http::StatusCode::OK, Json(models)).into_response();
    }

    // 3. Decrypt API key
    let api_key = match unprotect_secret(&settings.real_api_key) {
        Ok(key) => key,
        Err(error) => {
            tracing::error!("<- 錯誤: 解密 API key 失敗: {:?}", error);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    tracing::info!("-> 正在獲取模型列表，Gateway: {}", settings.real_base_url);

    // 4. Fetch and normalize models
    match fetch_models_list_async(
        &settings.real_base_url,
        &api_key,
        &settings.real_auth_scheme,
    )
    .await
    {
        Ok(raw_models) => match normalize_models_response_with_overrides(
            raw_models,
            &settings.model_reasoning_overrides,
            &settings.model_1m_overrides,
        ) {
            Ok(normalized) => {
                tracing::info!("<- 獲取模型列表成功，模型數量: {}", normalized.data.len());
                store_models_cache(
                    &settings.real_base_url,
                    &settings.real_auth_scheme,
                    &settings.model_reasoning_overrides,
                    &settings.model_1m_overrides,
                    &normalized,
                );
                settings.real_model_routes = normalized.routes.clone();
                settings.real_model_reasoning_efforts = normalized.reasoning_effort_routes.clone();
                settings.discovered_models = normalized
                    .data
                    .iter()
                    .map(|model| model.provider_model_id.clone())
                    .collect();
                let _ = save_launcher_settings(&settings);

                let inference_models = build_inference_models(&normalized.data);
                let port = settings
                    .active_port
                    .unwrap_or(crate::constants::DEFAULT_PORT);
                let content = serde_json::to_string_pretty(&crate::launcher::claude_config(
                    port,
                    &inference_models,
                    &settings.proxy_auth_token,
                ))
                .unwrap();
                let _ = crate::launcher::write_config_to_all_paths(
                    &format!("{}.json", crate::constants::CONFIG_ID),
                    &content,
                );

                (axum::http::StatusCode::OK, Json(normalized)).into_response()
            }
            Err(error) => {
                tracing::error!("<- 解析模型列表失敗: {:?}", error);
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
                    .into_response()
            }
        },
        Err(error) => {
            tracing::error!("<- 獲取模型列表失敗: {:?}", error);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

pub async fn fetch_models_list_async(
    base_url: &str,
    api_key: &str,
    auth_scheme: &str,
) -> Result<Value, String> {
    let model_info_url = crate::conversion::response_converter::normalize_model_info_url(base_url)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            crate::constants::HTTP_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| e.to_string())?;
    if let Ok(value) = fetch_json(&client, &model_info_url, api_key, auth_scheme).await {
        return Ok(value);
    }

    let url = crate::conversion::response_converter::normalize_models_url(base_url)?;
    fetch_json(&client, &url, api_key, auth_scheme).await
}

async fn fetch_json(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    auth_scheme: &str,
) -> Result<Value, String> {
    let mut req = client.get(url);
    if auth_scheme == "x-api-key" {
        req = req.header("x-api-key", api_key);
    } else {
        req = req.bearer_auth(api_key);
    }
    let res = req
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("API responded with status {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("JSON parse error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::openai::{NormalizedModel, NormalizedModels};
    use std::collections::HashMap;

    fn normalized_models() -> NormalizedModels {
        NormalizedModels {
            data: vec![NormalizedModel {
                kind: "model".to_string(),
                id: "claude-3-5-haiku[0]".to_string(),
                name: "glm-5.2".to_string(),
                display_name: "glm-5.2".to_string(),
                created_at: "1970-01-01T00:00:00.000Z".to_string(),
                provider_model_id: "glm-5.2".to_string(),
                max_input_tokens: None,
                max_tokens: None,
                capabilities: json!({ "thinking": { "supported": false } }),
                supports1m: None,
            }],
            has_more: false,
            first_id: Some("claude-3-5-haiku[0]".to_string()),
            last_id: Some("claude-3-5-haiku[0]".to_string()),
            routes: HashMap::from([("claude-3-5-haiku[0]".to_string(), "glm-5.2".to_string())]),
            reasoning_effort_routes: HashMap::new(),
        }
    }

    #[test]
    fn cached_models_are_reused_only_for_matching_gateway_settings() {
        clear_models_cache_for_tests();
        let empty_reasoning = HashMap::new();
        let empty_m1: HashMap<String, bool> = HashMap::new();
        store_models_cache(
            "http://localhost:4000",
            "bearer",
            &empty_reasoning,
            &empty_m1,
            &normalized_models(),
        );

        assert!(cached_models(
            "http://localhost:4000",
            "bearer",
            &empty_reasoning,
            &empty_m1
        )
        .is_some());
        assert!(cached_models(
            "http://localhost:4001",
            "bearer",
            &empty_reasoning,
            &empty_m1
        )
        .is_none());
        assert!(cached_models(
            "http://localhost:4000",
            "x-api-key",
            &empty_reasoning,
            &empty_m1
        )
        .is_none());
    }
}
