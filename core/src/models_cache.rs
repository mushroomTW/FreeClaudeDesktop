use crate::models::openai::NormalizedModels;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
struct ModelsCache {
    base_url: String,
    auth_scheme: String,
    reasoning_overrides: std::collections::HashMap<String, String>,
    m1_overrides: std::collections::HashMap<String, bool>,
    visibility_overrides: std::collections::HashMap<String, bool>,
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
    visibility_overrides: &std::collections::HashMap<String, bool>,
    models: &NormalizedModels,
) {
    if let Ok(mut cache) = models_cache().lock() {
        *cache = Some(ModelsCache {
            base_url: base_url.trim().to_string(),
            auth_scheme: auth_scheme.to_string(),
            reasoning_overrides: reasoning_overrides.clone(),
            m1_overrides: m1_overrides.clone(),
            visibility_overrides: visibility_overrides.clone(),
            models: models.clone(),
        });
    }
}

pub fn cached_models(
    base_url: &str,
    auth_scheme: &str,
    reasoning_overrides: &std::collections::HashMap<String, String>,
    m1_overrides: &std::collections::HashMap<String, bool>,
    visibility_overrides: &std::collections::HashMap<String, bool>,
) -> Option<NormalizedModels> {
    let cache = models_cache().lock().ok()?;
    let cache = cache.as_ref()?;
    (cache.base_url == base_url.trim()
        && cache.auth_scheme == auth_scheme
        && &cache.reasoning_overrides == reasoning_overrides
        && &cache.m1_overrides == m1_overrides
        && &cache.visibility_overrides == visibility_overrides)
        .then(|| cache.models.clone())
}

pub fn clear_models_cache() {
    if let Ok(mut cache) = models_cache().lock() {
        *cache = None;
    }
}

pub async fn fetch_models_list_typed(
    settings: &crate::Settings,
    api_key: &str,
) -> Result<Value, String> {
    let model_info_url =
        crate::conversion::response_converter::normalize_model_info_url(&settings.real_base_url)?;
    if let Ok(value) = fetch_json(
        crate::http_client(),
        &model_info_url,
        api_key,
        &settings.real_auth_scheme,
    )
    .await
    {
        return Ok(value);
    }

    use crate::gateway_client::GatewayClientFactory;
    let client = crate::gateway_client::AsyncOpenAiGatewayFactory
        .gateway_client(settings)
        .map_err(|error| error.to_string())?;
    let models = client
        .models()
        .list()
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_value(models).map_err(|error| error.to_string())
}

pub async fn fetch_models_list_async(
    base_url: &str,
    api_key: &str,
    auth_scheme: &str,
) -> Result<Value, String> {
    let model_info_url = crate::conversion::response_converter::normalize_model_info_url(base_url)?;
    let client = crate::http_client();
    if let Ok(value) = fetch_json(client, &model_info_url, api_key, auth_scheme).await {
        return Ok(value);
    }

    let url = crate::conversion::response_converter::normalize_models_url(base_url)?;
    fetch_json(client, &url, api_key, auth_scheme).await
}

async fn fetch_json(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    auth_scheme: &str,
) -> Result<Value, String> {
    let req = crate::apply_gateway_auth(client.get(url), auth_scheme, api_key, url)
        .map_err(|error| error.to_string())?;
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
