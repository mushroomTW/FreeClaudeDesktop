use crate::conversion::response_converter::{
    apply_model_visibility, build_inference_models, normalize_messages_url,
    normalize_models_response_with_overrides_and_prefer1m,
};
use crate::crypto::{protect_secret, unprotect_secret};
use crate::{AppError, AppResult, Settings};
use std::collections::HashMap;
use url::Url;

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
    pub enable_api_call_logging: bool,
    pub enable_web_server_tools: bool,
    pub web_fetch_allow_private_networks: bool,
    pub reasoning_replay_mode: String,
    pub transport_type: String,
    pub web_fetch_allowed_schemes: String,
    pub theme_mode: String,
    pub language: String,
    pub model_reasoning_overrides: HashMap<String, String>,
    pub model_1m_overrides: HashMap<String, bool>,
    pub model_1m_prefer_overrides: HashMap<String, bool>,
    pub model_visibility_overrides: HashMap<String, bool>,
    pub custom_claude_path: Option<Option<String>>,
    pub real_model: Option<String>,
    pub real_model_sonnet: Option<String>,
    pub real_model_opus: Option<String>,
    pub real_model_haiku: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SaveConfigOutput {
    pub discovered_models: Vec<String>,
}

/// 啟動或執行 `run_config_io` 流程。
pub async fn run_config_io<T, F>(operation: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| AppError::Launcher(error.to_string()))?
}

/// 讀取 `load_runtime_settings` 所需的資料。
pub async fn load_runtime_settings() -> AppResult<Option<Settings>> {
    run_config_io(crate::config::load_launcher_settings).await
}

/// 清理或還原 `unprotect_runtime_api_key` 所管理的資料。
pub async fn unprotect_runtime_api_key(stored: String) -> AppResult<String> {
    run_config_io(move || unprotect_secret(&stored)).await
}

/// 解析並選出 `resolve_api_key` 的結果。
fn resolve_api_key(api_key: &str, existing: Option<&Settings>) -> AppResult<String> {
    resolve_api_key_with(api_key, existing, unprotect_secret)
}

/// 解析並選出 `resolve_api_key_with` 的結果。
fn resolve_api_key_with(
    api_key: &str,
    existing: Option<&Settings>,
    decrypt: impl FnOnce(&str) -> AppResult<String>,
) -> AppResult<String> {
    if api_key.trim().is_empty() {
        existing
            .map(|settings| decrypt(&settings.gateway.real_api_key))
            .transpose()
            .map(Option::unwrap_or_default)
    } else {
        Ok(api_key.trim().to_string())
    }
}

/// 為 OpenRouter 網址建立官方的目前金鑰驗證端點。
fn openrouter_key_validation_url(base_url: &str) -> Option<String> {
    let parsed = Url::parse(base_url).ok()?;
    if !matches!(
        parsed.host_str(),
        Some("openrouter.ai") | Some("www.openrouter.ai")
    ) {
        return None;
    }
    let host = parsed.host_str()?;
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Some(format!("{}://{host}{port}/api/v1/key", parsed.scheme()))
}

/// 使用 OpenRouter 官方端點確認目前設定的 API key 可被接受。
async fn validate_openrouter_api_key(
    base_url: &str,
    api_key: &str,
    auth_scheme: &str,
) -> AppResult<()> {
    let Some(url) = openrouter_key_validation_url(base_url) else {
        return Ok(());
    };
    if api_key.is_empty() {
        return Err(AppError::InvalidConfig(
            "OpenRouter API key 不可為空".to_string(),
        ));
    }

    let request =
        crate::apply_gateway_auth(crate::http_client().get(&url), auth_scheme, api_key, &url)?;
    let response = request
        .send()
        .await
        .map_err(|error| AppError::Proxy(error.to_string()))?;
    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "OpenRouter 拒絕此憑證".to_string());
    Err(AppError::InvalidConfig(format!(
        "OpenRouter API key 驗證失敗（HTTP {}）：{message}",
        status.as_u16()
    )))
}

/// 儲存 `save_config_async` 所處理的資料。
pub async fn save_config_async(input: SaveConfigInput) -> AppResult<SaveConfigOutput> {
    save_or_refresh(input, false).await
}

/// 執行 `refresh_models_async` 對應的處理流程。
pub async fn refresh_models_async(input: SaveConfigInput) -> AppResult<SaveConfigOutput> {
    save_or_refresh(input, true).await
}

/// 儲存 `save_or_refresh` 所處理的資料。
async fn save_or_refresh(
    input: SaveConfigInput,
    require_models: bool,
) -> AppResult<SaveConfigOutput> {
    let base_url = input.base_url.trim().to_string();
    if base_url.is_empty() {
        return Err(AppError::InvalidConfig("缺少 Gateway Base URL".to_string()));
    }
    normalize_messages_url(&base_url).map_err(AppError::InvalidConfig)?;
    let _ = crate::apply_gateway_auth(
        crate::http_client().get(&base_url),
        &input.auth_scheme,
        "",
        &base_url,
    )?;

    let existing = load_runtime_settings().await?;
    let api_key_input = input.api_key.clone();
    let key_existing = existing.clone();
    let real_api_key =
        run_config_io(move || resolve_api_key(&api_key_input, key_existing.as_ref())).await?;

    validate_openrouter_api_key(&base_url, &real_api_key, &input.auth_scheme).await?;

    let mut normalized = match crate::models_cache::fetch_models_list_async(
        &base_url,
        &real_api_key,
        &input.auth_scheme,
    )
    .await
    {
        Ok(raw) => match normalize_models_response_with_overrides_and_prefer1m(
            raw,
            &input.model_reasoning_overrides,
            &input.model_1m_overrides,
            &input.model_1m_prefer_overrides,
        ) {
            Ok(models) => Some(models),
            Err(error) if require_models => return Err(AppError::InvalidConfig(error)),
            Err(_) => None,
        },
        Err(error) if require_models => return Err(AppError::Proxy(error)),
        Err(_) => None,
    };

    // Launcher 清單保留所有上游模型；顯示設定只影響 Claude Desktop 輸出。
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
                .map(|settings| settings.models.discovered_models.clone())
        })
        .unwrap_or_default();
    if let Some(models) = normalized.as_mut() {
        apply_model_visibility(models, &input.model_visibility_overrides);
    }

    let routes = normalized
        .as_ref()
        .map(|models| models.routes.clone())
        .or_else(|| {
            existing
                .as_ref()
                .map(|settings| settings.models.real_model_routes.clone())
        })
        .unwrap_or_default();
    let reasoning_efforts = normalized
        .as_ref()
        .map(|models| models.reasoning_effort_routes.clone())
        .or_else(|| {
            existing
                .as_ref()
                .map(|settings| settings.models.real_model_reasoning_efforts.clone())
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
    let cache_prefer_1m = input.model_1m_prefer_overrides.clone();
    let cache_visibility = input.model_visibility_overrides.clone();

    run_config_io(move || {
        let stored_api_key = protect_secret(&real_api_key)?;
        let proxy_auth_token = match existing
            .as_ref()
            .map(|s| s.gateway.proxy_auth_token.as_str())
        {
            Some(token) if !token.is_empty() && token != crate::constants::PROXY_AUTH_TOKEN => {
                token.to_string()
            }
            _ => crate::config::generate_proxy_auth_token()?,
        };
        let custom_claude_path = input.custom_claude_path.unwrap_or_else(|| {
            existing
                .as_ref()
                .and_then(|settings| settings.desktop.custom_claude_path.clone())
        });
        let settings = Settings {
            gateway: crate::config::GatewaySettings {
                real_base_url: base_url,
                real_api_key: stored_api_key,
                real_auth_scheme: input.auth_scheme,
                transport_type: input.transport_type,
                proxy_auth_token: proxy_auth_token.clone(),
            },
            models: crate::config::ModelSettings {
                real_model: input
                    .real_model
                    .or_else(|| existing.as_ref()?.models.real_model.clone()),
                real_model_sonnet: input
                    .real_model_sonnet
                    .or_else(|| existing.as_ref()?.models.real_model_sonnet.clone()),
                real_model_opus: input
                    .real_model_opus
                    .or_else(|| existing.as_ref()?.models.real_model_opus.clone()),
                real_model_haiku: input
                    .real_model_haiku
                    .or_else(|| existing.as_ref()?.models.real_model_haiku.clone()),
                real_model_routes: routes,
                real_model_reasoning_efforts: reasoning_efforts,
                discovered_models,
                model_reasoning_overrides: input.model_reasoning_overrides,
                model_1m_overrides: input.model_1m_overrides,
                model_1m_prefer_overrides: input.model_1m_prefer_overrides,
                model_visibility_overrides: input.model_visibility_overrides,
                reasoning_replay_mode: input.reasoning_replay_mode,
            },
            optimizations: crate::config::OptimizationSettings {
                enable_quota_check_mock: input.enable_quota_check_mock,
                enable_prefix_detection: input.enable_prefix_detection,
                enable_title_generation_skip: input.enable_title_generation_skip,
                enable_suggestion_mode_skip: input.enable_suggestion_mode_skip,
                enable_filepath_extraction_mock: input.enable_filepath_extraction_mock,
                enable_api_call_logging: input.enable_api_call_logging,
                enable_web_server_tools: input.enable_web_server_tools,
                web_fetch_allowed_schemes: input.web_fetch_allowed_schemes,
                web_fetch_allow_private_networks: input.web_fetch_allow_private_networks,
            },
            desktop: crate::config::DesktopSettings {
                custom_claude_path,
                active_port: Some(input.port),
            },
            ui: crate::config::UiSettings {
                theme_mode: input.theme_mode,
                language: input.language,
            },
        };
        crate::models_cache::clear_models_cache();
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
        crate::models_cache::store_models_cache(
            &cache_base_url,
            &cache_auth_scheme,
            &cache_reasoning,
            &cache_m1,
            &cache_prefer_1m,
            &cache_visibility,
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
    /// 驗證 `config_io_runs_on_blocking_pool` 的行為符合預期。
    async fn config_io_runs_on_blocking_pool() {
        let runtime_thread = std::thread::current().id();
        let io_thread = run_config_io(|| Ok(std::thread::current().id()))
            .await
            .unwrap();
        assert_ne!(io_thread, runtime_thread);
    }

    #[tokio::test]
    /// 驗證 `config_io_propagates_operation_errors` 的行為符合預期。
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
    /// 驗證 OpenRouter 網址會對應至官方目前金鑰端點。
    fn openrouter_urls_use_current_key_validation_endpoint() {
        assert_eq!(
            openrouter_key_validation_url("https://openrouter.ai/api/v1"),
            Some("https://openrouter.ai/api/v1/key".to_string())
        );
        assert_eq!(
            openrouter_key_validation_url("https://gateway.example/v1"),
            None
        );
    }

    #[test]
    /// 驗證 `blank_key_propagates_existing_key_decryption_error` 的行為符合預期。
    fn blank_key_propagates_existing_key_decryption_error() {
        let existing = Settings::default();
        let result = resolve_api_key_with("", Some(&existing), |_| {
            Err(AppError::Crypto("test".into()))
        });
        assert!(matches!(result, Err(AppError::Crypto(_))));
    }

    #[test]
    /// 驗證 `async_config_api_owns_its_input_and_output` 的行為符合預期。
    fn async_config_api_owns_its_input_and_output() {
        /// 執行 `assert_send_static` 對應的處理流程。
        fn assert_send_static<T: Send + 'static>() {}
        assert_send_static::<SaveConfigInput>();
        assert_send_static::<SaveConfigOutput>();
        let _ = save_config_async;
        let _ = refresh_models_async;
    }
}
