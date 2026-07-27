use super::*;

#[test]
/// 驗證 `test_role_mapping` 的行為符合預期。
fn test_role_mapping() {
    let settings = {
        let mut settings = Settings::default();
        settings.gateway.real_base_url = "https://openrouter.ai/api".to_string();
        settings
    };
    // Verify User and Assistant roles map correctly
    let body = json!({
        "model": "claude-3-5-sonnet",
        "messages": [
            {
                "role": "user",
                "content": "Hello"
            },
            {
                "role": "assistant",
                "content": "Hi there!"
            }
        ]
    });

    let (converted, is_stream) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
    assert!(!is_stream);
    let val: Value = serde_json::from_str(&converted).unwrap();
    let msgs = val["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "Hello");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[1]["content"], "Hi there!");
}

#[test]
/// 驗證 `test_system_prompt_handling` 的行為符合預期。
fn test_system_prompt_handling() {
    let settings = Settings::default();
    // Test system prompt as a single string
    let body = json!({
        "model": "claude-3-5-sonnet",
        "system": "You are a helpful assistant.",
        "messages": [
            {
                "role": "user",
                "content": "Hello"
            }
        ]
    });

    let (converted, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
    let val: Value = serde_json::from_str(&converted).unwrap();
    let msgs = val["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "You are a helpful assistant.");
}

#[test]
/// 驗證 `other_anthropic_native_tools_still_fail_clearly_for_openai_gateways` 的行為符合預期。
fn other_anthropic_native_tools_still_fail_clearly_for_openai_gateways() {
    let settings = Settings::default();
    let body = json!({
        "model": "claude-test",
        "messages": [{"role": "user", "content": "run a command"}],
        "tools": [{
            "type": "bash_20250124",
            "name": "bash"
        }]
    });

    let err = anthropic_to_openai_request(&body.to_string(), &settings).unwrap_err();

    assert!(err.contains("Anthropic-native tool"));
}

#[test]
/// 驗證 `thinking_budget_clamps_to_model_reasoning_effort_levels` 的行為符合預期。
fn thinking_budget_clamps_to_model_reasoning_effort_levels() {
    let mut routes = std::collections::HashMap::new();
    routes.insert("claude-sonnet-4-6[0]".to_string(), "nim-medium".to_string());
    let mut efforts = std::collections::HashMap::new();
    efforts.insert(
        "claude-sonnet-4-6[0]".to_string(),
        vec!["none".to_string(), "low".to_string(), "medium".to_string()],
    );
    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings.models.real_model_reasoning_efforts = efforts;
        settings
    };
    let body = json!({
        "model": "claude-sonnet-4-6[0]",
        "messages": [{"role": "user", "content": "think"}],
        "thinking": {
            "type": "enabled",
            "budget_tokens": 4096
        }
    });

    let (converted, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
    let converted: Value = serde_json::from_str(&converted).unwrap();

    assert_eq!(converted["model"], "nim-medium");
    assert_eq!(converted["reasoning_effort"], "medium");
    assert!(converted.get("thinking").is_none());
}

#[test]
/// 驗證 `resolve_model_route_handles_1m_suffix_and_fallbacks` 的行為符合預期。
fn resolve_model_route_handles_1m_suffix_and_fallbacks() {
    let mut routes = std::collections::HashMap::new();
    routes.insert("claude-sonnet-4-6[0]".to_string(), "nim-medium".to_string());
    routes.insert("claude-opus-4-8[0]".to_string(), "gpt-4o".to_string());

    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings.models.discovered_models = vec!["nim-medium".to_string(), "gpt-4o".to_string()];
        settings.models.real_model = Some("default-model".to_string());
        settings
    };

    assert_eq!(
        resolve_model_route("claude-sonnet-4-6[0]", &settings),
        Some("nim-medium".to_string())
    );
    assert_eq!(
        resolve_model_route("claude-sonnet-4-6[0][1m]", &settings),
        Some("nim-medium".to_string())
    );
    assert_eq!(
        resolve_model_route("gpt-4o[1m]", &settings),
        Some("gpt-4o".to_string())
    );
}

#[test]
/// 驗證 `resolve_model_route_robust_bracket_and_fuzzy_matching` 的行為符合預期。
fn resolve_model_route_robust_bracket_and_fuzzy_matching() {
    let mut routes = std::collections::HashMap::new();
    routes.insert("1".to_string(), "nim-one".to_string());
    routes.insert("1[2]".to_string(), "nim-nested".to_string());

    routes.insert("gpt-4o-sonnet".to_string(), "target-gpt-sonnet".to_string());
    routes.insert(
        "claude-3-5-sonnet-real".to_string(),
        "target-claude-sonnet".to_string(),
    );
    routes.insert("z-sonnet".to_string(), "target-z-sonnet".to_string());

    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings.models.discovered_models = vec![];
        settings
    };

    assert_eq!(
        resolve_model_route("model[1[2]]", &settings),
        Some("nim-nested".to_string())
    );

    assert_eq!(
        resolve_model_route("model[1][2]", &settings),
        Some("nim-one".to_string())
    );

    assert_eq!(
        resolve_model_route("some-random-sonnet-request", &settings),
        Some("target-claude-sonnet".to_string())
    );
}

#[test]
/// 驗證 `resolve_model_route_applies_fallback_safety_net_for_local_aliases` 的行為符合預期。
fn resolve_model_route_applies_fallback_safety_net_for_local_aliases() {
    let mut routes = std::collections::HashMap::new();
    routes.insert("claude-sonnet-4-6[0]".to_string(), "nim-medium".to_string());

    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings.models.discovered_models = vec!["gpt-4o".to_string()];
        settings.models.real_model = None;
        settings
    };

    assert_eq!(
        resolve_model_route("claude-haiku-4-5[2]", &settings),
        Some("nim-medium".to_string())
    );

    let settings_empty_routes = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = std::collections::HashMap::new();
        settings.models.discovered_models = vec!["gpt-4o".to_string()];
        settings.models.real_model = None;
        settings
    };
    assert_eq!(
        resolve_model_route("claude-haiku-4-5[2]", &settings_empty_routes),
        Some("gpt-4o".to_string())
    );
}

#[test]
/// 驗證 `resolve_model_route_prefers_family_override_over_dynamic_route` 的行為符合預期。
fn resolve_model_route_prefers_family_override_over_dynamic_route() {
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "claude-haiku-4-5[2]".to_string(),
        "diffusiongemma-26b".to_string(),
    );
    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings.models.real_model_haiku = Some("nemotron-3-super-120b".to_string());
        settings
    };

    assert_eq!(
        resolve_model_route("claude-haiku-4-5[2]", &settings),
        Some("nemotron-3-super-120b".to_string())
    );
}
