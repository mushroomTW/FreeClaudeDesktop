pub mod config_service;
pub mod conversion;
pub mod core;
pub mod gateway_client;
pub mod models;
pub mod models_cache;
pub mod optimization;
pub mod platform;

pub use core::{config, constants, error};
pub use error::{AppError, AppResult};
pub use platform::{common, crypto, launcher};

// 重導出 CLI 與 Proxy 需要的公開 API。
pub use config::{
    Settings, generate_proxy_auth_token, get_launcher_settings, save_launcher_settings,
    to_public_config,
};
pub use config_service::{SaveConfigInput, save_config_async, unprotect_runtime_api_key};
pub use constants::CONFIG_ID;
pub use conversion::request_converter::anthropic_to_openai_request;
pub use conversion::response_converter::{
    build_inference_models, normalize_messages_url, normalize_models_response,
    normalize_models_response_with_overrides,
    normalize_models_response_with_overrides_and_prefer1m, openai_to_anthropic_response,
    prepare_proxy_body,
};
pub use crypto::{delete_stored_secret, protect_secret, unprotect_secret};
pub use gateway_client::{AsyncOpenAiGatewayFactory, GatewayClientFactory};
pub use launcher::{
    detect_claude_path, launch_claude, purge_application_data, reset_mirror_profile,
    restore_official_config, resync_from_official, update_config_port,
};
pub use models::openai::InferenceModel;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "method")]
pub enum DashboardRpcRequest {
    GetStatus,
    DetectClaude,
    ApplySettings {
        #[serde(rename = "baseUrl")]
        base_url: String,
        #[serde(rename = "authScheme")]
        auth_scheme: String,
        #[serde(rename = "apiKey")]
        api_key: Option<String>,
    },
    LaunchClaude,
    RestoreSettings,
    SyncFromOfficial,
    ResetMirrorProfile,
    FetchModels,
}

use std::sync::OnceLock;
use std::time::Duration;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// 執行 `http_client` 對應的處理流程。
pub fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(constants::HTTP_TIMEOUT_SECS))
            .timeout(Duration::from_secs(constants::HTTP_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// 解析驗證方案最終使用的請求標頭名稱（`"x-api-key"` 或 `"authorization"`）。
///
/// 這是 auth scheme 判定的單一來源：`apply_gateway_auth`（設定標頭）與
/// proxy 的 `build_upstream_request`（判斷該略過哪個既有標頭）皆共用此函式，
/// 避免 `auto` / `sso` 的判定邏輯散落多處而發生不一致。
pub fn resolve_auth_header_name(scheme: &str, url: &str) -> AppResult<&'static str> {
    let resolved = match scheme {
        "auto" => {
            if url::Url::parse(url)
                .map_err(|error| AppError::InvalidConfig(error.to_string()))?
                .host_str()
                == Some("api.anthropic.com")
            {
                "x-api-key"
            } else {
                "bearer"
            }
        }
        "x-api-key" | "bearer" | "sso" => scheme,
        _ => return Err(AppError::InvalidConfig("不支援的 Auth Scheme".to_string())),
    };

    Ok(if resolved == "x-api-key" {
        "x-api-key"
    } else {
        "authorization"
    })
}

/// 轉換或更新 `apply_gateway_auth` 所處理的內容。
pub fn apply_gateway_auth(
    request: reqwest::RequestBuilder,
    scheme: &str,
    key: &str,
    url: &str,
) -> AppResult<reqwest::RequestBuilder> {
    let header = resolve_auth_header_name(scheme, url)?;

    if key.is_empty() {
        Ok(request)
    } else if header == "x-api-key" {
        Ok(request.header("x-api-key", key))
    } else {
        Ok(request.bearer_auth(key))
    }
}

/// 判斷是否符合 `is_valid_proxy_bearer` 的條件。
pub fn is_valid_proxy_bearer(header: Option<&str>, token: &str) -> bool {
    header
        .and_then(|value| value.trim().strip_prefix("Bearer "))
        .map(str::trim)
        == Some(token)
}

/// 判斷是否符合 `is_valid_proxy_authorization` 的條件。
pub fn is_valid_proxy_authorization(header: Option<&str>) -> bool {
    is_valid_proxy_bearer(header, constants::PROXY_AUTH_TOKEN)
}

/// 判斷是否符合 `is_authorized_proxy_request` 的條件。
pub fn is_authorized_proxy_request(
    authorization: Option<&str>,
    x_api_key: Option<&str>,
    token: &str,
) -> bool {
    let token = token.trim();
    if token.is_empty() {
        return false;
    }
    is_valid_proxy_bearer(authorization, token)
        || x_api_key.map(str::trim).is_some_and(|value| value == token)
}
