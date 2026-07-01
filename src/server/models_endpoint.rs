use crate::config::{get_launcher_settings, save_launcher_settings};
use crate::conversion::response_converter::{build_inference_models, normalize_models_response};
use crate::crypto::unprotect_secret;
use axum::{http::HeaderMap, response::IntoResponse, Json};
use serde_json::{json, Value};

pub async fn handle_models(headers: HeaderMap) -> impl IntoResponse {
    // 1. Validate authorization
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok());
    let x_api_key_header = headers.get("x-api-key").and_then(|h| h.to_str().ok());
    let is_authorized =
        x_api_key_header.is_some() || crate::server::is_valid_proxy_authorization(auth_header);
    if !is_authorized {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized" })),
        )
            .into_response();
    }

    // 2. Load settings
    let Some(mut settings) = get_launcher_settings() else {
        tracing::error!("<- 錯誤: Launcher 尚未配置");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Launcher has not been configured yet." })),
        )
            .into_response();
    };

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
        Ok(raw_models) => match normalize_models_response(raw_models) {
            Ok(normalized) => {
                tracing::info!("<- 獲取模型列表成功，模型數量: {}", normalized.data.len());
                settings.real_model_routes = normalized.routes.clone();
                let _ = save_launcher_settings(&settings);

                let inference_models = build_inference_models(&normalized.data);
                let port = settings
                    .active_port
                    .unwrap_or(crate::constants::DEFAULT_PORT);
                let content = serde_json::to_string_pretty(&crate::launcher::claude_config(
                    port,
                    &inference_models,
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
    let url = crate::conversion::response_converter::normalize_models_url(base_url)?;
    let client = reqwest::Client::new();
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
