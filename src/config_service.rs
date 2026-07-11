use crate::conversion::response_converter::{
    build_inference_models, normalize_messages_url, normalize_models_response_with_overrides,
};
use crate::crypto::{protect_secret, unprotect_secret};
use crate::{AppError, AppResult, Settings};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct SaveConfigInput {
    pub port: u16,
    pub base_url: String,
    pub api_key: String,
    pub auth_scheme: String,
    pub enable_quota_check_mock: bool,
    pub enable_prefix_detection: bool,
    pub enable_title_generation_skip: bool,
    pub enable_suggestion_mode_skip: bool,
    pub enable_filepath_extraction_mock: bool,
    pub enable_web_server_tools: bool,
    pub web_fetch_allow_private_networks: bool,
    pub reasoning_replay_mode: String,
    pub transport_type: String,
    pub web_fetch_allowed_schemes: String,
    pub theme_mode: String,
    pub model_reasoning_overrides: HashMap<String, String>,
    pub model_1m_overrides: HashMap<String, bool>,
    pub real_model: Option<String>,
    pub real_model_sonnet: Option<String>,
    pub real_model_opus: Option<String>,
    pub real_model_haiku: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SaveConfigOutput {
    pub discovered_models: Vec<String>,
}

pub(crate) async fn run_config_io<T, F>(operation: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| AppError::Launcher(error.to_string()))?
}

pub(crate) async fn load_runtime_settings() -> AppResult<Option<Settings>> {
    run_config_io(crate::config::load_launcher_settings).await
}

pub(crate) async fn unprotect_runtime_api_key(stored: String) -> AppResult<String> {
    run_config_io(move || unprotect_secret(&stored)).await
}

fn resolve_api_key(api_key: &str, existing: Option<&Settings>) -> AppResult<String> {
    resolve_api_key_with(api_key, existing, unprotect_secret)
}

fn resolve_api_key_with(
    api_key: &str,
    existing: Option<&Settings>,
    decrypt: impl FnOnce(&str) -> AppResult<String>,
) -> AppResult<String> {
    if api_key.trim().is_empty() {
        existing
            .map(|settings| decrypt(&settings.real_api_key))
            .transpose()
            .map(Option::unwrap_or_default)
    } else {
        Ok(api_key.trim().to_string())
    }
}

pub async fn save_config_async(input: SaveConfigInput) -> AppResult<SaveConfigOutput> {
    save_or_refresh(input, false).await
}

pub async fn refresh_models_async(input: SaveConfigInput) -> AppResult<SaveConfigOutput> {
    save_or_refresh(input, true).await
}

async fn save_or_refresh(
    input: SaveConfigInput,
    require_models: bool,
) -> AppResult<SaveConfigOutput> {
    let base_url = input.base_url.trim().to_string();
    if base_url.is_empty() {
        return Err(AppError::InvalidConfig("缺少 Gateway Base URL".to_string()));
    }
    normalize_messages_url(&base_url).map_err(AppError::InvalidConfig)?;
    let _ = crate::server::apply_gateway_auth(
        crate::server::http_client().get(&base_url),
        &input.auth_scheme,
        "",
        &base_url,
    )?;

    let existing = load_runtime_settings().await?;
    let api_key_input = input.api_key.clone();
    let key_existing = existing.clone();
    let real_api_key =
        run_config_io(move || resolve_api_key(&api_key_input, key_existing.as_ref())).await?;

    let normalized = match crate::server::models_endpoint::fetch_models_list_async(
        &base_url,
        &real_api_key,
        &input.auth_scheme,
    )
    .await
    {
        Ok(raw) => match normalize_models_response_with_overrides(
            raw,
            &input.model_reasoning_overrides,
            &input.model_1m_overrides,
        ) {
            Ok(models) => Some(models),
            Err(error) if require_models => return Err(AppError::InvalidConfig(error)),
            Err(_) => None,
        },
        Err(error) if require_models => return Err(AppError::Proxy(error)),
        Err(_) => None,
    };

    let routes = normalized
        .as_ref()
        .map(|models| models.routes.clone())
        .or_else(|| {
            existing
                .as_ref()
                .map(|settings| settings.real_model_routes.clone())
        })
        .unwrap_or_default();
    let reasoning_efforts = normalized
        .as_ref()
        .map(|models| models.reasoning_effort_routes.clone())
        .or_else(|| {
            existing
                .as_ref()
                .map(|settings| settings.real_model_reasoning_efforts.clone())
        })
        .unwrap_or_default();
    let discovered_models = normalized
        .as_ref()
        .map(|models| {
            models
                .data
                .iter()
                .map(|model| model.provider_model_id.clone())
                .collect()
        })
        .or_else(|| {
            existing
                .as_ref()
                .map(|settings| settings.discovered_models.clone())
        })
        .unwrap_or_default();
    let inference_models = normalized
        .as_ref()
        .map(|models| build_inference_models(&models.data))
        .unwrap_or_default();
    let output = SaveConfigOutput {
        discovered_models: discovered_models.clone(),
    };
    let cache_models = normalized.clone();
    let cache_base_url = base_url.clone();
    let cache_auth_scheme = input.auth_scheme.clone();
    let cache_reasoning = input.model_reasoning_overrides.clone();
    let cache_m1 = input.model_1m_overrides.clone();

    run_config_io(move || {
        let stored_api_key = protect_secret(&real_api_key)?;
        let proxy_auth_token = match existing.as_ref().map(|s| s.proxy_auth_token.as_str()) {
            Some(token) if !token.is_empty() && token != crate::constants::PROXY_AUTH_TOKEN => {
                token.to_string()
            }
            _ => crate::config::generate_proxy_auth_token()?,
        };
        let settings = Settings {
            real_base_url: base_url,
            real_api_key: stored_api_key,
            real_auth_scheme: input.auth_scheme,
            real_model: input
                .real_model
                .or_else(|| existing.as_ref()?.real_model.clone()),
            real_model_sonnet: input
                .real_model_sonnet
                .or_else(|| existing.as_ref()?.real_model_sonnet.clone()),
            real_model_opus: input
                .real_model_opus
                .or_else(|| existing.as_ref()?.real_model_opus.clone()),
            real_model_haiku: input
                .real_model_haiku
                .or_else(|| existing.as_ref()?.real_model_haiku.clone()),
            real_model_routes: routes,
            real_model_reasoning_efforts: reasoning_efforts,
            discovered_models,
            model_reasoning_overrides: input.model_reasoning_overrides,
            model_1m_overrides: input.model_1m_overrides,
            proxy_auth_token: proxy_auth_token.clone(),
            active_port: Some(input.port),
            transport_type: input.transport_type,
            reasoning_replay_mode: input.reasoning_replay_mode,
            enable_quota_check_mock: input.enable_quota_check_mock,
            enable_prefix_detection: input.enable_prefix_detection,
            enable_title_generation_skip: input.enable_title_generation_skip,
            enable_suggestion_mode_skip: input.enable_suggestion_mode_skip,
            enable_filepath_extraction_mock: input.enable_filepath_extraction_mock,
            enable_web_server_tools: input.enable_web_server_tools,
            web_fetch_allowed_schemes: input.web_fetch_allowed_schemes,
            web_fetch_allow_private_networks: input.web_fetch_allow_private_networks,
            theme_mode: input.theme_mode,
        };
        crate::server::models_endpoint::clear_models_cache();
        crate::config::save_launcher_settings(&settings)?;
        let content = serde_json::to_string_pretty(&crate::launcher::claude_config(
            input.port,
            &inference_models,
            &proxy_auth_token,
        ))?;
        crate::launcher::write_config_to_all_paths(
            &format!("{}.json", crate::constants::CONFIG_ID),
            &content,
        )?;
        crate::launcher::remove_anthropic_base_url_env()?;
        crate::launcher::apply_3p_deployment_mode()?;
        crate::launcher::write_managed_meta_to_all_paths()?;
        Ok::<_, AppError>(())
    })
    .await?;

    if let Some(models) = cache_models {
        crate::server::models_endpoint::store_models_cache(
            &cache_base_url,
            &cache_auth_scheme,
            &cache_reasoning,
            &cache_m1,
            &models,
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppError, Settings};

    #[tokio::test(flavor = "current_thread")]
    async fn config_io_runs_on_blocking_pool() {
        let runtime_thread = std::thread::current().id();
        let io_thread = run_config_io(|| Ok(std::thread::current().id()))
            .await
            .unwrap();
        assert_ne!(io_thread, runtime_thread);
    }

    #[tokio::test]
    async fn config_io_propagates_operation_errors() {
        let result =
            run_config_io(|| -> AppResult<()> { Err(AppError::InvalidConfig("sentinel".into())) })
                .await;
        assert!(matches!(
            result,
            Err(AppError::InvalidConfig(message)) if message == "sentinel"
        ));
    }

    #[test]
    fn blank_key_propagates_existing_key_decryption_error() {
        let existing = Settings::default();
        let result = resolve_api_key_with("", Some(&existing), |_| {
            Err(AppError::Crypto("test".into()))
        });
        assert!(matches!(result, Err(AppError::Crypto(_))));
    }

    #[test]
    fn async_config_api_owns_its_input_and_output() {
        fn assert_send_static<T: Send + 'static>() {}
        assert_send_static::<SaveConfigInput>();
        assert_send_static::<SaveConfigOutput>();
        let _ = save_config_async;
        let _ = refresh_models_async;
    }
}
