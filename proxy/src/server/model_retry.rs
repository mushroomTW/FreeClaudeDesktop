use super::upstream::build_upstream_request;
use axum::http::HeaderMap;
use free_claude_core::{
    Settings,
    conversion::response_converter::{
        normalize_models_response_with_overrides_and_prefer1m, rewrite_stale_model_request,
    },
};

/// 判斷上游錯誤是否代表模型路由已失效。
pub(crate) fn is_model_gone_or_invalid_error(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("has reached its end of life")
        || lower.contains("no longer available")
        || lower.contains("invalid model name")
        || lower.contains("invalid_model")
        || lower.contains("model_not_found")
        || lower.contains("call /v1/models")
        || lower.contains("model not found")
        || lower.contains("degraded function cannot be invoked")
}

/// 只有在尚未輸出且仍可重試時才允許模型備援。
pub(crate) fn may_retry_stale_model(
    output_started: bool,
    retry_available: bool,
    error: &str,
) -> bool {
    !output_started && retry_available && is_model_gone_or_invalid_error(error)
}

async fn refresh_settings_for_retry(settings: &Settings, api_key: &str) -> Option<Settings> {
    let raw = crate::server::models_endpoint::fetch_models_list_async(
        &settings.gateway.real_base_url,
        api_key,
        &settings.gateway.real_auth_scheme,
    )
    .await
    .ok()?;
    let normalized = normalize_models_response_with_overrides_and_prefer1m(
        raw,
        &settings.models.model_reasoning_overrides,
        &settings.models.model_1m_overrides,
        &settings.models.model_1m_prefer_overrides,
    )
    .ok()?;
    let mut refreshed = settings.clone();
    refreshed.models.real_model_routes = normalized.routes;
    refreshed.models.real_model_reasoning_efforts = normalized.reasoning_effort_routes;
    refreshed.models.discovered_models = normalized
        .data
        .into_iter()
        .map(|model| model.provider_model_id)
        .collect();
    Some(refreshed)
}

/// 刷新模型路由並使用備援模型重試上游請求。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn try_stale_model_retry(
    settings: &Settings,
    api_key: &str,
    proxy_body: &str,
    request_model: &str,
    target_url: &str,
    headers: &HeaderMap,
    is_anthropic_native: bool,
    error_text: &str,
    require_success: bool,
) -> Option<reqwest::Response> {
    if !may_retry_stale_model(false, true, error_text) {
        return None;
    }
    let retry_settings = refresh_settings_for_retry(settings, api_key)
        .await
        .unwrap_or_else(|| settings.clone());
    let rewrite = rewrite_stale_model_request(proxy_body, &retry_settings, request_model)?;
    tracing::warn!(
        "[model fallback] model error, retrying {} with {}",
        request_model,
        rewrite.fallback_model
    );
    let request = build_upstream_request(
        crate::server::http_client(),
        target_url,
        rewrite.updated_body.to_string(),
        headers,
        api_key,
        &retry_settings.gateway.real_auth_scheme,
        is_anthropic_native,
    )
    .ok()?;
    let response = request.send().await.ok()?;
    if require_success && !response.status().is_success() {
        return None;
    }
    Some(response)
}
