use crate::config_service::{load_runtime_settings, run_config_io, unprotect_runtime_api_key};
use crate::conversion::response_converter::{
    apply_model_visibility, build_inference_models, normalize_models_response_with_overrides,
};
use axum::{Json, http::HeaderMap, response::IntoResponse};
use serde_json::json;

// 模型快取與上游抓取邏輯統一放在 core::models_cache，作為單一來源。
// proxy 先前另存一份獨立的 static 快取與抓取函式，會與 core 的失效時機
// 不一致（例如某些寫入設定的路徑只清到其中一份）。此處改為重新導出 core
// 的實作，讓服務端與設定端共用同一份快取。
pub use free_claude_core::models_cache::{
    cached_models, clear_models_cache, fetch_models_list_async, fetch_models_list_typed,
    store_models_cache,
};

pub async fn handle_models(_headers: HeaderMap) -> impl IntoResponse {
    // 1. Load settings.
    let mut settings = match load_runtime_settings().await {
        Ok(Some(settings)) => settings,
        Ok(None) => {
            tracing::error!("<- 錯誤: Launcher 尚未配置");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Launcher has not been configured yet." })),
            )
                .into_response();
        }
        Err(error) => {
            tracing::error!("<- 錯誤: 讀取 Launcher 設定失敗: {error}");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    if let Some(models) = cached_models(
        &settings.real_base_url,
        &settings.real_auth_scheme,
        &settings.model_reasoning_overrides,
        &settings.model_1m_overrides,
        &settings.model_visibility_overrides,
    ) {
        return (axum::http::StatusCode::OK, Json(models)).into_response();
    }

    // 3. Decrypt API key
    let api_key = match unprotect_runtime_api_key(settings.real_api_key.clone()).await {
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
    let models_result = if settings.transport_type != "anthropic_messages"
        && settings.real_auth_scheme.eq_ignore_ascii_case("bearer")
    {
        fetch_models_list_typed(&settings, &api_key).await
    } else {
        fetch_models_list_async(
            &settings.real_base_url,
            &api_key,
            &settings.real_auth_scheme,
        )
        .await
    };

    match models_result {
        Ok(raw_models) => match normalize_models_response_with_overrides(
            raw_models,
            &settings.model_reasoning_overrides,
            &settings.model_1m_overrides,
        ) {
            Ok(mut normalized) => {
                let discovered_models = normalized
                    .data
                    .iter()
                    .map(|model| model.provider_model_id.clone())
                    .collect();
                apply_model_visibility(&mut normalized, &settings.model_visibility_overrides);
                tracing::info!("<- 獲取模型列表成功，模型數量: {}", normalized.data.len());
                settings.real_model_routes = normalized.routes.clone();
                settings.real_model_reasoning_efforts = normalized.reasoning_effort_routes.clone();
                settings.discovered_models = discovered_models;
                let inference_models = build_inference_models(&normalized.data);
                let port = settings
                    .active_port
                    .unwrap_or(crate::constants::DEFAULT_PORT);
                let settings_to_persist = settings.clone();
                let config_name = format!("{}.json", crate::constants::CONFIG_ID);
                if let Err(error) = run_config_io(move || {
                    crate::config::save_launcher_settings(&settings_to_persist)?;
                    let content = serde_json::to_string_pretty(&crate::launcher::claude_config(
                        port,
                        &inference_models,
                        &settings_to_persist.proxy_auth_token,
                    ))?;
                    crate::launcher::write_config_to_all_paths(&config_name, &content)
                })
                .await
                {
                    tracing::error!("<- 儲存模型設定失敗: {error}");
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": error.to_string() })),
                    )
                        .into_response();
                }

                store_models_cache(
                    &settings.real_base_url,
                    &settings.real_auth_scheme,
                    &settings.model_reasoning_overrides,
                    &settings.model_1m_overrides,
                    &settings.model_visibility_overrides,
                    &normalized,
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
        clear_models_cache();
        let empty_reasoning = HashMap::new();
        let empty_m1: HashMap<String, bool> = HashMap::new();
        let empty_visibility: HashMap<String, bool> = HashMap::new();
        store_models_cache(
            "http://localhost:4000",
            "bearer",
            &empty_reasoning,
            &empty_m1,
            &empty_visibility,
            &normalized_models(),
        );

        assert!(
            cached_models(
                "http://localhost:4000",
                "bearer",
                &empty_reasoning,
                &empty_m1,
                &empty_visibility
            )
            .is_some()
        );
        assert!(
            cached_models(
                "http://localhost:4001",
                "bearer",
                &empty_reasoning,
                &empty_m1,
                &empty_visibility
            )
            .is_none()
        );
        assert!(
            cached_models(
                "http://localhost:4000",
                "x-api-key",
                &empty_reasoning,
                &empty_m1,
                &empty_visibility
            )
            .is_none()
        );
        let hidden_visibility = HashMap::from([("glm-5.2".to_string(), false)]);
        assert!(
            cached_models(
                "http://localhost:4000",
                "bearer",
                &empty_reasoning,
                &empty_m1,
                &hidden_visibility
            )
            .is_none()
        );
    }
}
