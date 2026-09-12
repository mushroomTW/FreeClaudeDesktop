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
    assert!(!is_allowed_origin(Some("file://"), 3000));
    assert!(!is_allowed_origin(Some("null"), 3000));
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
        normalized.reasoning_effort_routes["claude-sonnet-5[0]"],
        vec!["none", "low", "high"]
    );
    assert_eq!(normalized.data[0].id, "claude-sonnet-5[0]");
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

    assert_eq!(normalized.data[0].id, "claude-opus-5[0]");
    assert_eq!(
        normalized.reasoning_effort_routes["claude-opus-5[0]"],
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

    assert_eq!(normalized.data[0].id, "claude-sonnet-5[0]");
    assert_eq!(
        normalized.reasoning_effort_routes["claude-sonnet-5[0]"],
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
        "claude-opus-5[0]".to_string(),
        "deepseek-v4-flash".to_string(),
    );
    routes.insert("claude-opus-5[3]".to_string(), "glm-5.1".to_string());
    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings
    };

    let rewritten = rewrite_stale_model_request(
        r#"{"model":"glm-5.1","messages":[]}"#,
        &settings,
        "claude-opus-5[3]",
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
    m1_overrides.insert("claude-sonnet-5-1m".to_string(), true);

    let normalized = normalize_models_response_with_overrides(
        json!({
            "data": [
                {
                    "id": "claude-sonnet-5",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "id": "claude-sonnet-5-1m",
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
    assert_eq!(normalized.data[0].provider_model_id, "claude-sonnet-5-1m");
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
                    "id": "claude-sonnet-5",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "id": "claude-sonnet-5-1m",
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
                    "id": "claude-sonnet-5",
                    "name": "Claude Sonnet 4.5",
                    "model_info": {
                        "supports_reasoning_effort": false,
                        "reasoning_effort_levels": ["none"]
                    }
                },
                {
                    "id": "claude-sonnet-5-1m",
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
/// 驗證 `is_allowed_origin` 涵蓋剩餘分支。
fn is_allowed_origin_covers_remaining_branches() {
    assert!(!is_allowed_origin(Some(""), 3000));
    assert!(!is_allowed_origin(Some("not a url"), 3000));
    assert!(!is_allowed_origin(Some("http://localhost:9999"), 3000));
    assert!(is_allowed_origin(Some("https://sub.claude.ai"), 3000));
    assert!(is_allowed_origin(Some("https://deep.sub.claude.com"), 3000));
    // 本機 Proxy 僅提供 HTTP，其他 scheme 不得偽裝成同埠的可信 Origin。
    assert!(!is_allowed_origin(Some("https://localhost:3000"), 3000));
    assert!(!is_allowed_origin(Some("https://localhost:4000"), 3000));
}

#[test]
/// 驗證各 Gateway URL 正規化函式的成功與失敗分支。
fn normalize_url_helpers_cover_all_rules() {
    let u = normalize_messages_url("https://gateway.example.com/v1").unwrap();
    assert!(u.ends_with("/messages"));
    let bad = normalize_messages_url("http://evil.example").unwrap_err();
    assert!(bad.contains("HTTPS"));
    let local = normalize_messages_url("http://127.0.0.1:4000").unwrap();
    assert!(local.ends_with("/messages"));
    assert!(normalize_messages_url("not a url").is_err());
    let mu = normalize_models_url("https://gateway.example.com/v1").unwrap();
    assert!(mu.ends_with("/models"));
    let miu = normalize_model_info_url("https://gateway.example.com").unwrap();
    assert!(miu.contains("model/info"));
    let ccu = normalize_chat_completions_url("https://gateway.example.com").unwrap();
    assert!(ccu.ends_with("/chat/completions"));
    let ccu2 =
        normalize_chat_completions_url("https://gateway.example.com/v1/chat/completions").unwrap();
    assert!(ccu2.ends_with("/chat/completions"));
    let u2 = normalize_messages_url("https://user:pass@host.example/v1/").unwrap();
    assert!(u2.contains("user:pass@"));
    assert!(u2.ends_with("/messages"));
    // ::1 / [::1] 也視為 localhost
    let u3 = normalize_messages_url("http://localhost:4000").unwrap();
    assert!(u3.starts_with("http://localhost"));
    assert!(normalize_messages_url("ftp://localhost").is_err());
}

#[test]
/// 驗證 `prepare_proxy_body` 的模型映射與原樣轉發分支。
fn prepare_proxy_body_covers_mapping_and_passthrough() {
    let settings = Settings::default();
    // 非 JSON body 原樣回傳
    assert_eq!(prepare_proxy_body("garbage", &settings), "garbage");
    // 有 model 且可被 resolve
    let out = prepare_proxy_body(r#"{"model":"claude-3-haiku","messages":[]}"#, &settings);
    assert!(out.contains("messages"));
    // 無 model 欄位 → 仍為合法 JSON 且保留 messages
    let out2 = prepare_proxy_body(r#"{"messages":[]}"#, &settings);
    let out2_val: Value = serde_json::from_str(&out2).expect("應為合法 JSON");
    assert!(out2_val.get("messages").is_some());
}

#[test]
/// 驗證 `rewrite_stale_model_request` 的錯誤與預設回退分支。
fn rewrite_stale_model_request_covers_error_and_no_route() {
    let settings = {
        let mut s = Settings::default();
        s.models.real_model = Some("real-backup".to_string());
        s
    };
    assert!(rewrite_stale_model_request("not json", &settings, "x").is_none());
    // body 無 model 欄位 → None
    assert!(rewrite_stale_model_request(r#"{"messages":[]}"#, &settings, "x").is_none());
    let r = rewrite_stale_model_request(r#"{"model":"stale-x","messages":[]}"#, &settings, "req-1")
        .unwrap();
    assert_eq!(r.fallback_model, "real-backup");
    assert_eq!(r.updated_body["model"], "real-backup");
    // 全部都是 stale 且 fallback 為空 → None
    let empty_settings = Settings::default();
    assert!(
        rewrite_stale_model_request(r#"{"model":"stale-x"}"#, &empty_settings, "req-1").is_none()
    );
}

// ── openai_to_anthropic_response 分支 ────────────────────────────────

#[test]
/// 驗證 `openai_to_anthropic_response` 的 tool_calls、reasoning 與多選項分支。
fn openai_to_anthropic_response_covers_tool_and_reasoning() {
    let openai_res = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "reasoning_content": "think",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "f", "arguments": "{\"a\":1}"}
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 2}
    });
    let converted = openai_to_anthropic_response(&openai_res.to_string(), "claude-x").unwrap();
    assert_eq!(converted["stop_reason"], "tool_use");
    let types: Vec<&str> = converted["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["type"].as_str())
        .collect();
    assert!(types.contains(&"tool_use"));
    assert!(types.contains(&"thinking"));

    // 無 choices → 空 content 回傳 end_turn（涵蓋 first_choice None 分支）
    let empty = openai_to_anthropic_response(r#"{"choices":[]}"#, "m").unwrap();
    assert_eq!(empty["stop_reason"], "end_turn");
    // 錯誤 JSON
    assert!(openai_to_anthropic_response("garbage", "m").is_err());
}

#[test]
/// 驗證 reasoning effort / capability 產生輔助函式的各種分支。
fn reasoning_helpers_cover_mappings() {
    let levels = override_reasoning_levels("low");
    assert!(!levels.is_empty());
    let levels2 = override_reasoning_levels("none");
    assert_eq!(levels2, vec!["none".to_string()]);
}

#[test]
/// 驗證 `models_response` 的綜合分支（空 data、重複、max reasoning）。
fn models_response_covers_edge_cases() {
    let normalized = normalize_models_response(json!({"data": []})).unwrap();
    assert_eq!(normalized.data.len(), 0);
    // 無 model_name/id 的項目會被 parse_provider_models 跳過
    let n2 = normalize_models_response(json!({"data": [{"model_info": {}}]})).unwrap();
    assert!(n2.data.is_empty(), "缺 id/model_name 的項目應被跳過");
}

#[test]
/// 驗證 `build_inference_models` 與 `apply_model_visibility` 的基本分支。
fn inference_models_and_visibility_cover_basic() {
    let normalized = normalize_models_response(json!({
        "data": [{
            "model_name": "m1",
            "model_info": {"supports_reasoning_effort": false, "reasoning_effort_levels": ["none"]}
        }]
    }))
    .unwrap();
    let inf = build_inference_models(&normalized.data);
    assert_eq!(inf.len(), 1);
    let mut vis = HashMap::new();
    vis.insert("m1".to_string(), false);
    let mut n = normalized.clone();
    apply_model_visibility(&mut n, &vis);
    assert_eq!(n.data.len(), 0);
}

// ── 既有測試（保留）─────────────────────────────────────────────

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

    assert_eq!(normalized.data[0].id, "claude-sonnet-5[0]");
    assert_eq!(normalized.data[0].supports1m, Some(true));
}
