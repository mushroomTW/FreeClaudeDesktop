pub mod config_service;
pub mod conversion;
pub mod core;
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
    Settings, generate_proxy_auth_token, get_launcher_settings, save_launcher_settings,
    to_public_config,
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
    LAUNCHER_SHOW_REQUESTED, is_authorized_proxy_request, is_valid_proxy_authorization, run_server,
    start_server_background,
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
    _: bool,
    web_fetch_allow_private_networks: bool,
    reasoning_replay_mode: &str,
    transport_type: &str,
    web_fetch_allowed_schemes: &str,
    theme_mode: &str,
    language: &str,
    model_reasoning_overrides: &HashMap<String, String>,
    model_1m_overrides: &HashMap<String, bool>,
    model_visibility_overrides: &HashMap<String, bool>,
    real_model: Option<String>,
    real_model_sonnet: Option<String>,
    real_model_opus: Option<String>,
    real_model_haiku: Option<String>,
) -> AppResult<()> {
    let input = config_service::SaveConfigInput {
        port,
        base_url: base_url.to_string(),
        api_key: api_key.to_string(),
        auth_scheme: auth_scheme.to_string(),
        enable_quota_check_mock,
        enable_prefix_detection,
        enable_title_generation_skip,
        enable_suggestion_mode_skip,
        enable_filepath_extraction_mock,
        enable_web_server_tools,
        web_fetch_allow_private_networks,
        reasoning_replay_mode: reasoning_replay_mode.to_string(),
        transport_type: transport_type.to_string(),
        web_fetch_allowed_schemes: web_fetch_allowed_schemes.to_string(),
        theme_mode: theme_mode.to_string(),
        language: language.to_string(),
        model_reasoning_overrides: model_reasoning_overrides.clone(),
        model_1m_overrides: model_1m_overrides.clone(),
        model_visibility_overrides: model_visibility_overrides.clone(),
        real_model,
        real_model_sonnet,
        real_model_opus,
        real_model_haiku,
    };
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()?.block_on(config_service::save_config_async(input))
    })
    .join()
    .map_err(|_| AppError::Launcher("設定執行緒異常結束".to_string()))??;
    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
