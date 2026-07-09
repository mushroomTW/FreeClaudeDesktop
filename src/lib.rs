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
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn normalizes_provider_urls_to_messages_endpoint() {
        assert_eq!(
            normalize_messages_url("https://openrouter.ai/api").unwrap(),
            "https://openrouter.ai/api/v1/messages"
        );
        assert_eq!(
            normalize_messages_url("https://api.anthropic.com/v1/").unwrap(),
            "https://api.anthropic.com/v1/messages"
        );
        assert!(normalize_messages_url("http://evil.example").is_err());
    }

    #[test]
    fn rewrites_model_from_saved_routes() {
        let mut routes = HashMap::new();
        routes.insert(
            "anthropic/claude-sonnet-4-5".to_string(),
            "openai/gpt-oss-20b:free".to_string(),
        );
        let settings = Settings {
            real_model_routes: routes,
            ..Settings::default()
        };

        let body = prepare_proxy_body(
            r#"{"model":"anthropic/claude-sonnet-4-5","messages":[]}"#,
            &settings,
        );

        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["model"],
            "openai/gpt-oss-20b:free"
        );
    }

    #[test]
    fn rewrites_model_from_saved_routes_bracket_key() {
        // Claude Desktop sends model IDs like "claude-opus-4-8[0]" (bracket suffix for thinking level).
        // The routes map must use the same bracket format as keys, otherwise lookup fails and the
        // unmapped alias gets forwarded to LiteLLM, causing "Invalid model name" errors.
        let mut routes = HashMap::new();
        routes.insert(
            "claude-opus-4-8[0]".to_string(),
            "deepseek-v4-flash".to_string(),
        );
        routes.insert("claude-opus-4-8[3]".to_string(), "glm-5.1".to_string());
        let settings = Settings {
            real_model_routes: routes,
            ..Settings::default()
        };

        let body = prepare_proxy_body(r#"{"model":"claude-opus-4-8[0]","messages":[]}"#, &settings);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["model"],
            "deepseek-v4-flash"
        );

        let body2 =
            prepare_proxy_body(r#"{"model":"claude-opus-4-8[3]","messages":[]}"#, &settings);
        assert_eq!(
            serde_json::from_str::<Value>(&body2).unwrap()["model"],
            "glm-5.1"
        );
    }

    #[test]
    fn prepare_proxy_body_falls_back_for_unmapped_local_alias() {
        let settings = Settings {
            real_model_haiku: Some("nemotron-3-super-120b".to_string()),
            ..Settings::default()
        };

        let body = prepare_proxy_body(
            r#"{"model":"claude-haiku-4-5[2]","messages":[]}"#,
            &settings,
        );

        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["model"],
            "nemotron-3-super-120b"
        );
    }

    #[test]
    fn public_config_hides_api_key() {
        let settings = Settings {
            real_base_url: "https://openrouter.ai/api".to_string(),
            real_api_key: "secret".to_string(),
            real_auth_scheme: "bearer".to_string(),
            ..Settings::default()
        };

        assert_eq!(
            to_public_config(&settings),
            json!({
                "baseUrl": "https://openrouter.ai/api",
                "authScheme": "bearer",
                "hasApiKey": true
            })
        );
    }

    #[test]
    fn validates_proxy_authorization_header() {
        assert!(is_valid_proxy_authorization(Some(
            "Bearer local-proxy-token"
        )));
        assert!(!is_valid_proxy_authorization(None));
        assert!(!is_valid_proxy_authorization(Some("Bearer wrong")));
        assert!(!is_valid_proxy_authorization(Some("local-proxy-token")));
    }

    #[test]
    fn validates_proxy_x_api_key_against_configured_token() {
        assert!(is_authorized_proxy_request(
            None,
            Some("secret-token"),
            "secret-token"
        ));
        assert!(is_authorized_proxy_request(
            Some("Bearer secret-token"),
            None,
            "secret-token"
        ));
        assert!(!is_authorized_proxy_request(
            None,
            Some("anything"),
            "secret-token"
        ));
        assert!(!is_authorized_proxy_request(
            Some("Bearer wrong"),
            Some("anything"),
            "secret-token"
        ));
    }

    #[test]
    fn protects_and_restores_api_key() {
        assert_eq!(unprotect_secret("legacy-key").unwrap(), "legacy-key");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(unprotect_secret("dpapi:1234abcd").unwrap(), "");
    }

    #[test]
    fn test_anthropic_to_openai_request_multimodal_image() {
        let body = json!({
            "model": "anthropic/claude-3-5-sonnet",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Describe this image"
                        },
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/jpeg",
                                "data": "BASE64DATAHERE"
                            }
                        }
                    ]
                }
            ]
        });

        let settings = Settings {
            real_base_url: "https://api.openai.com/v1".to_string(),
            ..Settings::default()
        };

        let (converted_body, _) =
            anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        let converted: Value = serde_json::from_str(&converted_body).unwrap();

        let messages = converted["messages"].as_array().unwrap();
        let msg = &messages[0];
        assert_eq!(msg["role"], "user");
        let content = msg["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Describe this image");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/jpeg;base64,BASE64DATAHERE"
        );
    }

    #[test]
    fn test_anthropic_to_openai_request_tool_result_ordering() {
        let body = json!({
            "model": "anthropic/claude-3-5-sonnet",
            "messages": [
                {
                    "role": "user",
                    "content": "Call tools"
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_use",
                            "id": "tool_1",
                            "name": "my_tool_1",
                            "input": {"x": 10}
                        },
                        {
                            "type": "tool_use",
                            "id": "tool_2",
                            "name": "my_tool_2",
                            "input": {"y": 20}
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "tool_1",
                            "content": "Tool success output"
                        },
                        {
                            "type": "tool_result",
                            "tool_use_id": "tool_2",
                            "content": {
                                "status": "success",
                                "data": [1, 2, 3]
                            }
                        }
                    ]
                }
            ]
        });

        let settings = Settings {
            real_base_url: "https://api.openai.com/v1".to_string(),
            ..Settings::default()
        };

        let (converted_body, _) =
            anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        let converted: Value = serde_json::from_str(&converted_body).unwrap();

        let messages = converted["messages"].as_array().unwrap();

        // 期望序列：
        // 0: user
        // 1: assistant (tool_calls)
        // 2: tool (tool_call_id: tool_1, content: "Tool success output")
        // 3: tool (tool_call_id: tool_2, content: "{"data":[1,2,3],"status":"success"}" (序列化字串))
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Call tools");

        assert_eq!(messages[1]["role"], "assistant");
        let tool_calls = messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0]["id"], "tool_1");
        assert_eq!(tool_calls[1]["id"], "tool_2");

        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "tool_1");
        assert_eq!(messages[2]["content"], "Tool success output");

        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "tool_2");
        // 容錯序列化為 JSON 字串
        let parsed_content: Value =
            serde_json::from_str(messages[3]["content"].as_str().unwrap()).unwrap();
        assert_eq!(parsed_content["status"], "success");
    }

    #[test]
    fn test_anthropic_to_openai_thinking_conversion() {
        let body = json!({
            "model": "anthropic/claude-3-5-sonnet",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                }
            ],
            "thinking": {
                "type": "enabled",
                "budget_tokens": 1024
            }
        });

        let settings = Settings {
            real_base_url: "https://integrate.api.nvidia.com/v1".to_string(),
            ..Settings::default()
        };

        let (converted_body, _) =
            anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
        let converted: Value = serde_json::from_str(&converted_body).unwrap();

        // 驗證 thinking 欄位是否已被移除
        assert!(converted.get("thinking").is_none());
    }

    #[test]
    fn test_openai_to_anthropic_thinking_response_conversion() {
        let openai_res = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 12,
                "total_tokens": 21
            },
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "This is the final response.",
                        "reasoning_content": "This is the reasoning process."
                    },
                    "finish_reason": "stop",
                    "index": 0
                }
            ]
        });

        let converted =
            openai_to_anthropic_response(&openai_res.to_string(), "anthropic/claude-3-5-sonnet")
                .unwrap();

        let content = converted.get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 2);

        let block0 = &content[0];
        assert_eq!(block0["type"], "thinking");
        assert_eq!(block0["thinking"], "This is the reasoning process.");

        let block1 = &content[1];
        assert_eq!(block1["type"], "text");
        assert_eq!(block1["text"], "This is the final response.");
    }
}
