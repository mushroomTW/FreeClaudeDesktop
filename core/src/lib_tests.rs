use super::*;
use serde_json::{Value, json};

#[test]
#[allow(clippy::type_complexity)]
fn save_config_keeps_legacy_function_signature() {
    let _: fn(
        u16,
        &str,
        &str,
        &str,
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
        &str,
        &str,
        &str,
        &str,
        &str,
        &HashMap<String, String>,
        &HashMap<String, bool>,
        &HashMap<String, bool>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) -> AppResult<()> = save_config;
}

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
fn rewrites_model_from_saved_routes_underscore_key() {
    // Claude Desktop receives model IDs like "claude-opus-4-8_0" (underscore suffix for index).
    // The routes map must use the same underscore format as keys, otherwise lookup fails and the
    // unmapped alias gets forwarded to LiteLLM, causing "Invalid model name" errors.
    let mut routes = HashMap::new();
    routes.insert(
        "claude-opus-4-8_0".to_string(),
        "deepseek-v4-flash".to_string(),
    );
    routes.insert("claude-opus-4-8_3".to_string(), "glm-5.1".to_string());
    let settings = Settings {
        real_model_routes: routes,
        ..Settings::default()
    };

    let body = prepare_proxy_body(r#"{"model":"claude-opus-4-8_0","messages":[]}"#, &settings);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["model"],
        "deepseek-v4-flash"
    );

    let body2 = prepare_proxy_body(r#"{"model":"claude-opus-4-8_3","messages":[]}"#, &settings);
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

    let body = prepare_proxy_body(r#"{"model":"claude-haiku-4-5_2","messages":[]}"#, &settings);

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

    let public_cfg = to_public_config(&settings);
    assert_eq!(public_cfg["baseUrl"], "https://openrouter.ai/api");
    assert_eq!(public_cfg["authScheme"], "bearer");
    assert_eq!(public_cfg["hasApiKey"], true);
    assert!(public_cfg.get("realApiKey").is_none());
    assert!(public_cfg.get("apiKey").is_none());
    assert!(public_cfg.get("proxyAuthToken").is_none());
    assert!(public_cfg.get("discoveredModels").is_some());
    assert!(public_cfg.get("transportType").is_some());
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

    let (converted_body, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
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

    let (converted_body, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
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

    let (converted_body, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
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
