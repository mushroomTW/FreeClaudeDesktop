pub mod conversion;
pub mod core;
pub mod mcp;
pub mod models;
pub mod optimization;
pub mod platform;
pub mod runtime;
pub mod server;
pub mod ui;

pub use core::{config, constants, error};
pub use error::{AppError, AppResult};
pub use platform::{common, crypto, launcher};
pub use runtime::{app, tray};

use std::collections::HashMap;

// 重導出 main.rs 和外部需要的 API，保持向後相容
pub use config::{
    generate_proxy_auth_token, get_launcher_settings, save_launcher_settings, to_public_config,
    Settings,
};
pub use constants::CONFIG_ID;
pub use conversion::request_converter::anthropic_to_openai_request;
pub use conversion::response_converter::{
    build_inference_models, normalize_messages_url, normalize_models_response,
    normalize_models_response_with_overrides, openai_to_anthropic_response, prepare_proxy_body,
};
pub use crypto::{protect_secret, unprotect_secret};
pub use launcher::{
    detect_claude_path, launch_claude, restore_official_config, update_config_port,
};
pub use models::openai::InferenceModel;
pub use server::{
    is_authorized_proxy_request, is_valid_proxy_authorization, run_server, start_server_background,
    LAUNCHER_SHOW_REQUESTED,
};

/// 儲存配置，獲取模型列表，並生成 Claude Desktop 配置
#[allow(clippy::too_many_arguments)]
pub fn save_config(
    port: u16,
    base_url: &str,
    api_key: &str,
    auth_scheme: &str,
    enable_quota_check_mock: bool,
    enable_prefix_detection: bool,
    enable_title_generation_skip: bool,
    enable_suggestion_mode_skip: bool,
    enable_filepath_extraction_mock: bool,
    enable_web_server_tools: bool,
    enable_computer_mcp_server: bool,
    web_fetch_allow_private_networks: bool,
    reasoning_replay_mode: &str,
    transport_type: &str,
    web_fetch_allowed_schemes: &str,
    theme_mode: &str,
    model_reasoning_overrides: &HashMap<String, String>,
    model_1m_overrides: &HashMap<String, bool>,
    real_model: Option<String>,
    real_model_sonnet: Option<String>,
    real_model_opus: Option<String>,
    real_model_haiku: Option<String>,
) -> AppResult<()> {
    let existing = get_launcher_settings();
    let real_api_key = if api_key.trim().is_empty() {
        existing
            .as_ref()
            .and_then(|s| unprotect_secret(&s.real_api_key).ok())
            .unwrap_or_default()
    } else {
        api_key.trim().to_string()
    };
    if base_url.trim().is_empty() {
        return Err(AppError::InvalidConfig("缺少 Gateway Base URL".to_string()));
    }
    if auth_scheme != "bearer" && auth_scheme != "x-api-key" {
        return Err(AppError::InvalidConfig("不支援的 Auth Scheme".to_string()));
    }
    normalize_messages_url(base_url).map_err(AppError::InvalidConfig)?;

    let mut inference_models = Vec::new();
    let mut routes = HashMap::new();
    let mut reasoning_efforts = HashMap::new();
    let mut discovered_models = Vec::new();
    if let Ok(raw_models) = server::fetch_models_list(base_url, &real_api_key, auth_scheme) {
        if let Ok(normalized) = normalize_models_response_with_overrides(
            raw_models,
            model_reasoning_overrides,
            model_1m_overrides,
        ) {
            routes = normalized.routes.clone();
            reasoning_efforts = normalized.reasoning_effort_routes.clone();
            discovered_models = normalized
                .data
                .iter()
                .map(|model| model.provider_model_id.clone())
                .collect();
            inference_models = build_inference_models(&normalized.data);
            server::models_endpoint::store_models_cache(
                base_url,
                auth_scheme,
                model_reasoning_overrides,
                model_1m_overrides,
                &normalized,
            );
        }
    }
    let stored_api_key = protect_secret(&real_api_key)?;
    let proxy_auth_token = match existing.as_ref().map(|s| s.proxy_auth_token.as_str()) {
        Some(token) if !token.is_empty() && token != constants::PROXY_AUTH_TOKEN => {
            token.to_string()
        }
        _ => generate_proxy_auth_token()?,
    };

    let settings = Settings {
        real_base_url: base_url.trim().to_string(),
        real_api_key: stored_api_key,
        real_auth_scheme: auth_scheme.to_string(),
        real_model: real_model.or_else(|| existing.as_ref().and_then(|s| s.real_model.clone())),
        real_model_sonnet: real_model_sonnet
            .or_else(|| existing.as_ref().and_then(|s| s.real_model_sonnet.clone())),
        real_model_opus: real_model_opus
            .or_else(|| existing.as_ref().and_then(|s| s.real_model_opus.clone())),
        real_model_haiku: real_model_haiku
            .or_else(|| existing.as_ref().and_then(|s| s.real_model_haiku.clone())),
        real_model_routes: if routes.is_empty() {
            existing
                .as_ref()
                .map(|s| s.real_model_routes.clone())
                .unwrap_or_default()
        } else {
            routes
        },
        real_model_reasoning_efforts: if reasoning_efforts.is_empty() {
            existing
                .as_ref()
                .map(|s| s.real_model_reasoning_efforts.clone())
                .unwrap_or_default()
        } else {
            reasoning_efforts
        },
        discovered_models: if discovered_models.is_empty() {
            existing
                .as_ref()
                .map(|s| s.discovered_models.clone())
                .unwrap_or_default()
        } else {
            discovered_models
        },
        model_reasoning_overrides: model_reasoning_overrides.clone(),
        model_1m_overrides: model_1m_overrides.clone(),
        proxy_auth_token: proxy_auth_token.clone(),
        active_port: Some(port),
        transport_type: transport_type.to_string(),
        reasoning_replay_mode: reasoning_replay_mode.to_string(),
        enable_quota_check_mock,
        enable_prefix_detection,
        enable_title_generation_skip,
        enable_suggestion_mode_skip,
        enable_filepath_extraction_mock,
        enable_web_server_tools,
        enable_computer_mcp_server,
        web_fetch_allowed_schemes: web_fetch_allowed_schemes.to_string(),
        web_fetch_allow_private_networks,
        theme_mode: theme_mode.to_string(),
    };
    crate::server::models_endpoint::clear_models_cache();
    save_launcher_settings(&settings)?;

    let content = serde_json::to_string_pretty(&launcher::claude_config(
        port,
        &inference_models,
        &proxy_auth_token,
    ))
    .unwrap();
    launcher::write_config_to_all_paths(&format!("{CONFIG_ID}.json"), &content)?;
    let _ = launcher::remove_anthropic_base_url_env();
    launcher::apply_3p_deployment_mode()?;
    launcher::apply_computer_mcp_server_config(enable_computer_mcp_server)?;
    launcher::write_managed_meta_to_all_paths()?;
    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
