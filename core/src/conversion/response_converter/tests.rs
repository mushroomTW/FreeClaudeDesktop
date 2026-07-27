use super::*;

#[test]
/// 驗證 `cors_origin_allows_only_local_http_port` 的行為符合預期。
fn cors_origin_allows_only_local_http_port() {
    // No origin → always allowed
    assert!(is_allowed_origin(None, 3000));
    // Local HTTP origins → allowed
    assert!(is_allowed_origin(Some("http://localhost:3000"), 3000));
    assert!(is_allowed_origin(Some("http://127.0.0.1:3000"), 3000));
    assert!(is_allowed_origin(Some("http://[::1]:3000"), 3000));
    // Claude Desktop origins → allowed
    assert!(is_allowed_origin(Some("https://claude.ai"), 3000));
    assert!(is_allowed_origin(Some("https://claude.com"), 3000));
    assert!(is_allowed_origin(Some("https://preview.claude.com"), 3000));
    assert!(is_allowed_origin(Some("app://localhost"), 3000));
    assert!(is_allowed_origin(Some("anthropic://desktop"), 3000));
    assert!(is_allowed_origin(Some("file://"), 3000));
    // Browser origins that are not Claude Desktop → blocked
    assert!(!is_allowed_origin(Some("https://evil.example"), 3000));
    assert!(!is_allowed_origin(Some("http://localhost:4000"), 3000));
}

#[test]
/// 驗證 `empty_tool_calls_do_not_force_tool_use_stop_reason` 的行為符合預期。
fn empty_tool_calls_do_not_force_tool_use_stop_reason() {
    let openai_res = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "hi",
                "tool_calls": []
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 1,
            "completion_tokens": 1
        }
    });

    let converted = openai_to_anthropic_response(&openai_res.to_string(), "claude-test").unwrap();

    assert_eq!(converted["stop_reason"], "end_turn");
    assert_eq!(converted["content"][0]["type"], "text");
    assert_eq!(converted["content"][0]["text"], "hi");
}

#[test]
/// 驗證含 UTF-8 BOM 的上游回應仍可正常轉換。
fn openai_response_accepts_utf8_bom() {
    let openai_res = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "hi"
            },
            "finish_reason": "stop"
        }]
    });
    let body = format!("\u{feff}{}", openai_res);

    let converted = openai_to_anthropic_response(&body, "claude-test").unwrap();

    assert_eq!(converted["content"][0]["text"], "hi");
}

#[test]
/// 驗證空白上游回應會回報可診斷的錯誤。
fn empty_openai_response_is_rejected() {
    let error = openai_to_anthropic_response(" \n\t", "claude-test").unwrap_err();

    assert_eq!(error, "上游 OpenAI 回應本文為空");
}

#[test]
/// 驗證 `models_default_to_no_thinking_capability` 的行為符合預期。
fn models_default_to_no_thinking_capability() {
    let normalized = normalize_models_response(json!({
        "data": [{
            "id": "nemotron-3-super-120b"
        }]
    }))
    .unwrap();

    assert_eq!(
        normalized.data[0].provider_model_id,
        "nemotron-3-super-120b"
    );
    assert_eq!(
        normalized.data[0].capabilities["thinking"]["supported"],
        false
    );
}

#[test]
/// 驗證 `models_use_litellm_model_info_thinking_capability` 的行為符合預期。
fn models_use_litellm_model_info_thinking_capability() {
    let normalized = normalize_models_response(json!({
        "data": [{
            "model_name": "claude-native",
            "model_info": {
                "supports_thinking": true
            }
        }]
    }))
    .unwrap();

    assert_eq!(normalized.data[0].provider_model_id, "claude-native");
    assert_eq!(
        normalized.data[0].capabilities["thinking"]["supported"],
        true
    );
}

#[test]
/// 驗證 `models_store_litellm_reasoning_effort_levels` 的行為符合預期。
fn models_store_litellm_reasoning_effort_levels() {
    let normalized = normalize_models_response(json!({
        "data": [{
            "model_name": "nim-high",
            "model_info": {
                "supports_reasoning_effort": true,
                "reasoning_effort_levels": ["none", "low", "high"]
            }
        }]
    }))
    .unwrap();

    assert_eq!(
        normalized.reasoning_effort_routes["claude-sonnet-4-6[0]"],
        vec!["none", "low", "high"]
    );
    assert_eq!(normalized.data[0].id, "claude-sonnet-4-6[0]");
    assert_eq!(
        normalized.data[0].capabilities["thinking"]["supported"],
        true
    );
}

#[test]
/// 驗證 `models_with_max_reasoning_stores_reasoning_effort_levels` 的行為符合預期。
fn models_with_max_reasoning_stores_reasoning_effort_levels() {
    let normalized = normalize_models_response(json!({
        "data": [{
            "model_name": "deepseek-v4-pro",
            "model_info": {
                "supports_reasoning_effort": true,
                "reasoning_effort_levels": ["none", "high", "max"]
            }
        }]
    }))
    .unwrap();

    assert_eq!(normalized.data[0].id, "claude-opus-4-8[0]");
    assert_eq!(
        normalized.reasoning_effort_routes["claude-opus-4-8[0]"],
        vec!["none", "high", "max"]
    );
}

#[test]
/// 驗證 `model_reasoning_override_enables_reasoning_alias` 的行為符合預期。
fn model_reasoning_override_enables_reasoning_alias() {
    let mut overrides = HashMap::new();
    overrides.insert("glm-5.2".to_string(), "high".to_string());

    let normalized = normalize_models_response_with_overrides(
        json!({
            "data": [{
                "model_name": "glm-5.2",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            }]
        }),
        &overrides,
        &HashMap::new(),
    )
    .unwrap();

    assert_eq!(normalized.data[0].id, "claude-sonnet-4-6[0]");
    assert_eq!(
        normalized.reasoning_effort_routes["claude-sonnet-4-6[0]"],
        vec!["none", "high"]
    );
}

#[test]
/// 驗證 `models_without_reasoning_use_provider_model_id` 的行為符合預期。
fn models_without_reasoning_use_provider_model_id() {
    let normalized = normalize_models_response(json!({
        "data": [{
            "model_name": "glm-5.2",
            "model_info": {
                "supports_reasoning_effort": false,
                "reasoning_effort_levels": ["none"]
            }
        }]
    }))
    .unwrap();

    assert_eq!(normalized.data[0].id, "claude-haiku-4-5[0]");
    assert_eq!(normalized.routes["claude-haiku-4-5[0]"], "glm-5.2");
}

#[test]
/// 驗證 `duplicate_litellm_deployments_are_deduped_by_model_name` 的行為符合預期。
fn duplicate_litellm_deployments_are_deduped_by_model_name() {
    let normalized = normalize_models_response(json!({
        "data": [
            {
                "model_name": "glm-5.2",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            },
            {
                "model_name": "glm-5.2",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            }
        ]
    }))
    .unwrap();

    assert_eq!(normalized.data.len(), 1);
    assert_eq!(normalized.data[0].id, "claude-haiku-4-5[0]");
}

#[test]
/// 驗證 `rewrites_stale_mapped_model_to_fallback_route` 的行為符合預期。
fn rewrites_stale_mapped_model_to_fallback_route() {
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "claude-opus-4-8[0]".to_string(),
        "deepseek-v4-flash".to_string(),
    );
    routes.insert("claude-opus-4-8[3]".to_string(), "glm-5.1".to_string());
    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings
    };

    let rewritten = rewrite_stale_model_request(
        r#"{"model":"glm-5.1","messages":[]}"#,
        &settings,
        "claude-opus-4-8[3]",
    )
    .unwrap();

    assert_eq!(rewritten.fallback_model, "deepseek-v4-flash");
    assert_eq!(rewritten.updated_body["model"], "deepseek-v4-flash");
}

#[test]
/// 驗證 `models_response_sets_supports1m_when_override_is_enabled` 的行為符合預期。
fn models_response_sets_supports1m_when_override_is_enabled() {
    let mut m1_overrides = std::collections::HashMap::new();
    m1_overrides.insert("deepseek-v4-flash".to_string(), true);

    let normalized = normalize_models_response_with_overrides(
        json!({
            "data": [{
                "id": "deepseek-v4-flash",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            }]
        }),
        &std::collections::HashMap::new(),
        &m1_overrides,
    )
    .unwrap();

    assert_eq!(normalized.data[0].id, "claude-haiku-4-5[0]");
    assert_eq!(normalized.data[0].name, "deepseek-v4-flash");
    assert_eq!(normalized.data[0].max_input_tokens, Some(1_000_000));
    assert_eq!(normalized.data[0].supports1m, Some(true));
    assert_eq!(
        normalized.routes["claude-haiku-4-5[0]"],
        "deepseek-v4-flash"
    );
}

#[test]
/// 驗證 `models_response_sets_prefer1m_only_for_1m_models` 的行為符合預期。
fn models_response_sets_prefer1m_only_for_1m_models() {
    let mut m1_overrides = std::collections::HashMap::new();
    m1_overrides.insert("deepseek-v4-flash".to_string(), true);
    let mut prefer1m_overrides = std::collections::HashMap::new();
    prefer1m_overrides.insert("deepseek-v4-flash".to_string(), true);

    let normalized = normalize_models_response_with_overrides_and_prefer1m(
        json!({
            "data": [{
                "id": "deepseek-v4-flash",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            }]
        }),
        &std::collections::HashMap::new(),
        &m1_overrides,
        &prefer1m_overrides,
    )
    .unwrap();

    assert_eq!(normalized.data[0].supports1m, Some(true));
    assert_eq!(normalized.data[0].prefer1m, Some(true));

    let without_support = normalize_models_response_with_overrides_and_prefer1m(
        json!({
            "data": [{
                "id": "standard-model",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            }]
        }),
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        &std::collections::HashMap::from([("standard-model".to_string(), true)]),
    )
    .unwrap();

    assert_eq!(without_support.data[0].supports1m, None);
    assert_eq!(without_support.data[0].prefer1m, None);
}

#[test]
/// 驗證 `model_info_max_input_tokens_enables_1m_support` 的行為符合預期。
fn model_info_max_input_tokens_enables_1m_support() {
    let normalized = normalize_models_response(json!({
        "data": [{
            "model_name": "nemotron-3-super-120b",
            "model_info": {
                "max_input_tokens": 1_000_000,
                "max_output_tokens": 65_536
            }
        }]
    }))
    .unwrap();

    assert_eq!(
        normalized.data[0].provider_model_id,
        "nemotron-3-super-120b"
    );
    assert_eq!(normalized.data[0].id, "claude-haiku-4-5[0]");
    assert_eq!(normalized.data[0].name, "nemotron-3-super-120b");
    assert_eq!(normalized.data[0].max_input_tokens, Some(1_000_000));
    assert_eq!(normalized.data[0].max_tokens, Some(65_536));
    assert_eq!(normalized.data[0].supports1m, Some(true));
}

#[test]
/// 驗證 `models_response_hides_same_name_200k_variant_when_1m_enabled` 的行為符合預期。
fn models_response_hides_same_name_200k_variant_when_1m_enabled() {
    // 上游同時回傳兩筆 id 不同但 name 相同的條目（200k 與 1m 變體）
    let mut m1_overrides = std::collections::HashMap::new();
    m1_overrides.insert("claude-sonnet-4-5-1m".to_string(), true);

    let normalized = normalize_models_response_with_overrides(
        json!({
            "data": [
                {
                    "id": "claude-sonnet-4-5",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "id": "claude-sonnet-4-5-1m",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                }
            ]
        }),
        &std::collections::HashMap::new(),
        &m1_overrides,
    )
    .unwrap();

    // 只剩被勾選 1M 的那一筆
    assert_eq!(normalized.data.len(), 1);
    assert_eq!(normalized.data[0].provider_model_id, "claude-sonnet-4-5-1m");
    assert_eq!(normalized.data[0].id, "claude-haiku-4-5[0]");
    assert_eq!(normalized.data[0].name, "Claude Sonnet 4.5");
    assert_eq!(normalized.data[0].supports1m, Some(true));
}

#[test]
/// 驗證 `models_response_keeps_all_when_no_1m_override` 的行為符合預期。
fn models_response_keeps_all_when_no_1m_override() {
    // 沒有任何 1M 勾選 → 不過濾，兩筆同名變體都保留
    let normalized = normalize_models_response_with_overrides(
        json!({
            "data": [
                {
                    "id": "claude-sonnet-4-5",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "id": "claude-sonnet-4-5-1m",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                }
            ]
        }),
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert_eq!(normalized.data.len(), 2);
}

#[test]
/// 驗證 `models_response_keeps_same_name_variants_when_neither_1m_enabled` 的行為符合預期。
fn models_response_keeps_same_name_variants_when_neither_1m_enabled() {
    // 同名族群中無任何 1M 勾選 → 不誤殺，兩筆都保留
    let mut m1_overrides = std::collections::HashMap::new();
    // 只勾選「另一個」不同名稱的模型為 1M
    m1_overrides.insert("glm-5.2".to_string(), true);

    let normalized = normalize_models_response_with_overrides(
        json!({
            "data": [
                {
                    "id": "claude-sonnet-4-5",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "id": "claude-sonnet-4-5-1m",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "id": "glm-5.2",
                    "name": "GLM 5.2",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                }
            ]
        }),
        &std::collections::HashMap::new(),
        &m1_overrides,
    )
    .unwrap();

    // Claude Sonnet 4.5 同名兩筆皆保留（族群里沒有 1M 勾選），glm-5.2 也保留
    assert_eq!(normalized.data.len(), 3);
}

#[test]
/// 驗證 `model_visibility_hides_model_and_its_routes_but_defaults_to_visible` 的行為符合預期。
fn model_visibility_hides_model_and_its_routes_but_defaults_to_visible() {
    let mut normalized = normalize_models_response(json!({
        "data": [
            {
                "model_name": "visible-model",
                "model_info": {
                    "supports_reasoning_effort": false,
                    "reasoning_effort_levels": ["none"]
                }
            },
            {
                "model_name": "hidden-model",
                "model_info": {
                    "supports_reasoning_effort": true,
                    "reasoning_effort_levels": ["none", "high"]
                }
            }
        ]
    }))
    .unwrap();
    let hidden_alias = normalized
        .data
        .iter()
        .find(|model| model.provider_model_id == "hidden-model")
        .unwrap()
        .id
        .clone();
    let mut visibility = HashMap::new();
    visibility.insert("hidden-model".to_string(), false);

    apply_model_visibility(&mut normalized, &visibility);

    assert_eq!(normalized.data.len(), 1);
    assert_eq!(normalized.data[0].provider_model_id, "visible-model");
    assert!(!normalized.routes.contains_key(&hidden_alias));
    assert!(
        !normalized
            .reasoning_effort_routes
            .contains_key(&hidden_alias)
    );
    assert_eq!(normalized.first_id, Some(normalized.data[0].id.clone()));
    assert_eq!(normalized.last_id, Some(normalized.data[0].id.clone()));
}

#[test]
/// 驗證 `models_response_keeps_sonnet_alias_for_1m_model_without_max_reasoning` 的行為符合預期。
fn models_response_keeps_sonnet_alias_for_1m_model_without_max_reasoning() {
    let mut m1_overrides = std::collections::HashMap::new();
    m1_overrides.insert("reasoning-model".to_string(), true);

    let normalized = normalize_models_response_with_overrides(
        json!({
            "data": [{
                "id": "reasoning-model",
                "model_info": {
                    "supports_reasoning_effort": true,
                    "reasoning_effort_levels": ["none", "high"]
                }
            }]
        }),
        &std::collections::HashMap::new(),
        &m1_overrides,
    )
    .unwrap();

    assert_eq!(normalized.data[0].id, "claude-sonnet-4-6[0]");
    assert_eq!(normalized.data[0].supports1m, Some(true));
}
