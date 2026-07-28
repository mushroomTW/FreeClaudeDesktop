use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use free_claude_core::{
    AdminRpcRequest, SaveConfigInput, Settings, config_service::load_runtime_settings,
    to_public_config,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminSettingsUpdate {
    pub base_url: String,
    pub auth_scheme: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub real_model: Option<Option<String>>,
    #[serde(default)]
    pub real_model_sonnet: Option<Option<String>>,
    #[serde(default)]
    pub real_model_opus: Option<Option<String>>,
    #[serde(default)]
    pub real_model_haiku: Option<Option<String>>,
    #[serde(default)]
    pub real_model_routes: Option<HashMap<String, String>>,
    #[serde(default)]
    pub real_model_reasoning_efforts: Option<HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub discovered_models: Option<Vec<String>>,
    #[serde(default)]
    pub model_reasoning_overrides: Option<HashMap<String, String>>,
    #[serde(default)]
    pub model_1m_overrides: Option<HashMap<String, bool>>,
    #[serde(default)]
    pub model_1m_prefer_overrides: Option<HashMap<String, bool>>,
    #[serde(default)]
    pub model_visibility_overrides: Option<HashMap<String, bool>>,
    #[serde(default)]
    pub transport_type: Option<String>,
    #[serde(default)]
    pub reasoning_replay_mode: Option<String>,
    #[serde(default)]
    pub enable_quota_check_mock: Option<bool>,
    #[serde(default)]
    pub enable_prefix_detection: Option<bool>,
    #[serde(default)]
    pub enable_title_generation_skip: Option<bool>,
    #[serde(default)]
    pub enable_suggestion_mode_skip: Option<bool>,
    #[serde(default)]
    pub enable_filepath_extraction_mock: Option<bool>,
    #[serde(default)]
    pub enable_api_call_logging: Option<bool>,
    #[serde(default)]
    pub enable_web_server_tools: Option<bool>,
    #[serde(default)]
    pub web_fetch_allowed_schemes: Option<String>,
    #[serde(default)]
    pub web_fetch_allow_private_networks: Option<bool>,
    #[serde(default)]
    pub theme_mode: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub custom_claude_path: Option<Option<String>>,
}

pub(crate) fn validate_gateway_url(base_url: &str) -> Result<String, &'static str> {
    let base_url = base_url.trim().trim_end_matches('/');
    let parsed = Url::parse(base_url).map_err(|_| "Gateway URL 格式無效")?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
        return Err("Gateway URL 必須使用 HTTP 或 HTTPS");
    }
    if parsed.scheme() == "http"
        && !matches!(
            parsed.host_str(),
            Some("localhost") | Some("127.0.0.1") | Some("::1")
        )
    {
        return Err("非本機 Gateway 必須使用 HTTPS");
    }
    Ok(base_url.to_string())
}

pub(crate) fn normalize_custom_claude_path(path: Option<String>) -> Option<String> {
    path.and_then(|path| {
        let path = path.trim().to_string();
        (!path.is_empty()).then_some(path)
    })
}

fn apply_settings_update(
    settings: &mut Settings,
    input: AdminSettingsUpdate,
) -> Result<Value, (StatusCode, Json<Value>)> {
    settings.gateway.real_base_url = validate_gateway_url(&input.base_url)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
    settings.gateway.real_auth_scheme = input.auth_scheme.trim().to_ascii_lowercase();
    if !matches!(
        settings.gateway.real_auth_scheme.as_str(),
        "bearer" | "x-api-key"
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "authScheme 必須是 bearer 或 x-api-key" })),
        ));
    }
    if let Some(api_key) = input.api_key.map(|key| key.trim().to_string())
        && !api_key.is_empty()
    {
        settings.gateway.real_api_key = api_key;
    }

    apply_optional(&mut settings.models.real_model, input.real_model);
    apply_optional(
        &mut settings.models.real_model_sonnet,
        input.real_model_sonnet,
    );
    apply_optional(&mut settings.models.real_model_opus, input.real_model_opus);
    apply_optional(
        &mut settings.models.real_model_haiku,
        input.real_model_haiku,
    );
    apply_optional(
        &mut settings.models.real_model_routes,
        input.real_model_routes,
    );
    apply_optional(
        &mut settings.models.real_model_reasoning_efforts,
        input.real_model_reasoning_efforts,
    );
    apply_optional(
        &mut settings.models.discovered_models,
        input.discovered_models,
    );
    apply_optional(
        &mut settings.models.model_reasoning_overrides,
        input.model_reasoning_overrides,
    );
    apply_optional(
        &mut settings.models.model_1m_overrides,
        input.model_1m_overrides,
    );
    apply_optional(
        &mut settings.models.model_1m_prefer_overrides,
        input.model_1m_prefer_overrides,
    );
    apply_optional(
        &mut settings.models.model_visibility_overrides,
        input.model_visibility_overrides,
    );
    apply_optional(&mut settings.gateway.transport_type, input.transport_type);
    apply_optional(
        &mut settings.models.reasoning_replay_mode,
        input.reasoning_replay_mode,
    );
    apply_optional(
        &mut settings.optimizations.enable_quota_check_mock,
        input.enable_quota_check_mock,
    );
    apply_optional(
        &mut settings.optimizations.enable_prefix_detection,
        input.enable_prefix_detection,
    );
    apply_optional(
        &mut settings.optimizations.enable_title_generation_skip,
        input.enable_title_generation_skip,
    );
    apply_optional(
        &mut settings.optimizations.enable_suggestion_mode_skip,
        input.enable_suggestion_mode_skip,
    );
    apply_optional(
        &mut settings.optimizations.enable_filepath_extraction_mock,
        input.enable_filepath_extraction_mock,
    );
    apply_optional(
        &mut settings.optimizations.enable_api_call_logging,
        input.enable_api_call_logging,
    );
    apply_optional(
        &mut settings.optimizations.enable_web_server_tools,
        input.enable_web_server_tools,
    );
    apply_optional(
        &mut settings.optimizations.web_fetch_allowed_schemes,
        input.web_fetch_allowed_schemes,
    );
    apply_optional(
        &mut settings.optimizations.web_fetch_allow_private_networks,
        input.web_fetch_allow_private_networks,
    );
    apply_optional(&mut settings.ui.theme_mode, input.theme_mode);
    apply_optional(&mut settings.ui.language, input.language);
    if let Some(path) = input.custom_claude_path {
        settings.desktop.custom_claude_path = normalize_custom_claude_path(path);
    }
    Ok(to_public_config(settings))
}

fn apply_optional<T>(target: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *target = value;
    }
}

async fn load_settings() -> Result<Settings, Response> {
    match load_runtime_settings().await {
        Ok(Some(settings)) => Ok(settings),
        Ok(None) => Ok(Settings::default()),
        Err(error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response()),
    }
}

pub async fn handle_admin_settings(_headers: HeaderMap) -> Response {
    match load_settings().await {
        Ok(settings) => (StatusCode::OK, Json(to_public_config(&settings))).into_response(),
        Err(response) => response,
    }
}

pub async fn update_admin_settings(
    _headers: HeaderMap,
    Json(input): Json<AdminSettingsUpdate>,
) -> Response {
    let mut settings = match load_settings().await {
        Ok(settings) => settings,
        Err(response) => return response,
    };
    let new_api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string);
    let updated_config = match apply_settings_update(&mut settings, input) {
        Ok(config) => config,
        Err(response) => return response.into_response(),
    };

    let raw_api_key = match new_api_key {
        Some(key) => key,
        None => {
            match free_claude_core::unprotect_runtime_api_key(settings.gateway.real_api_key.clone())
                .await
            {
                Ok(key) => key,
                Err(error) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": error.to_string() })),
                    )
                        .into_response();
                }
            }
        }
    };
    let input = save_input_from_settings(&settings, raw_api_key);
    if let Err(error) = free_claude_core::save_config_async(input).await {
        tracing::error!("同步 3P Gateway 設定給 Claude Desktop 失敗: {error}");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response();
    }
    tracing::info!("<- 已成功為 Claude Desktop 部署與套用 3P Gateway 設定！");
    (StatusCode::OK, Json(updated_config)).into_response()
}

pub async fn handle_admin_status(_headers: HeaderMap) -> Response {
    match load_settings().await {
        Ok(settings) => (
            StatusCode::OK,
            Json(json!({
                "proxy": { "status": "ok", "port": settings.desktop.active_port },
                "settings": to_public_config(&settings),
            })),
        )
            .into_response(),
        Err(response) => response,
    }
}

pub async fn handle_admin_rpc(
    State(companion_state): State<super::companion::CompanionState>,
    _headers: HeaderMap,
    Json(request): Json<AdminRpcRequest>,
) -> Response {
    let settings = match load_settings().await {
        Ok(settings) => settings,
        Err(response) => return response,
    };
    if matches!(request, AdminRpcRequest::GetStatus) {
        return (
            StatusCode::OK,
            Json(json!({
                "result": {
                    "proxy": { "status": "ok", "port": settings.desktop.active_port },
                    "settings": to_public_config(&settings),
                }
            })),
        )
            .into_response();
    }
    if matches!(request, AdminRpcRequest::FetchModels) {
        super::models_endpoint::clear_models_cache();
        return super::models_endpoint::handle_models(HeaderMap::new())
            .await
            .into_response();
    }
    super::companion::forward_request(&companion_state, request).await
}

fn save_input_from_settings(settings: &Settings, api_key: String) -> SaveConfigInput {
    SaveConfigInput {
        port: settings
            .desktop
            .active_port
            .unwrap_or(free_claude_core::constants::DEFAULT_PORT),
        base_url: settings.gateway.real_base_url.clone(),
        api_key,
        auth_scheme: settings.gateway.real_auth_scheme.clone(),
        enable_quota_check_mock: settings.optimizations.enable_quota_check_mock,
        enable_prefix_detection: settings.optimizations.enable_prefix_detection,
        enable_title_generation_skip: settings.optimizations.enable_title_generation_skip,
        enable_suggestion_mode_skip: settings.optimizations.enable_suggestion_mode_skip,
        enable_filepath_extraction_mock: settings.optimizations.enable_filepath_extraction_mock,
        enable_api_call_logging: settings.optimizations.enable_api_call_logging,
        enable_web_server_tools: settings.optimizations.enable_web_server_tools,
        web_fetch_allow_private_networks: settings.optimizations.web_fetch_allow_private_networks,
        reasoning_replay_mode: settings.models.reasoning_replay_mode.clone(),
        transport_type: settings.gateway.transport_type.clone(),
        web_fetch_allowed_schemes: settings.optimizations.web_fetch_allowed_schemes.clone(),
        theme_mode: settings.ui.theme_mode.clone(),
        language: settings.ui.language.clone(),
        model_reasoning_overrides: settings.models.model_reasoning_overrides.clone(),
        model_1m_overrides: settings.models.model_1m_overrides.clone(),
        model_1m_prefer_overrides: settings.models.model_1m_prefer_overrides.clone(),
        model_visibility_overrides: settings.models.model_visibility_overrides.clone(),
        custom_claude_path: Some(settings.desktop.custom_claude_path.clone()),
        real_model: settings.models.real_model.clone(),
        real_model_sonnet: settings.models.real_model_sonnet.clone(),
        real_model_opus: settings.models.real_model_opus.clone(),
        real_model_haiku: settings.models.real_model_haiku.clone(),
    }
}
