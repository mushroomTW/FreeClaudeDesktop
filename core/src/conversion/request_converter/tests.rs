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
    routes.insert("claude-sonnet-5[0]".to_string(), "nim-medium".to_string());
    let mut efforts = std::collections::HashMap::new();
    efforts.insert(
        "claude-sonnet-5[0]".to_string(),
        vec!["none".to_string(), "low".to_string(), "medium".to_string()],
    );
    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings.models.real_model_reasoning_efforts = efforts;
        settings
    };
    let body = json!({
        "model": "claude-sonnet-5[0]",
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
    routes.insert("claude-sonnet-5[0]".to_string(), "nim-medium".to_string());
    routes.insert("claude-opus-5[0]".to_string(), "gpt-4o".to_string());

    let settings = {
        let mut settings = Settings::default();
        settings.models.real_model_routes = routes;
        settings.models.discovered_models = vec!["nim-medium".to_string(), "gpt-4o".to_string()];
        settings.models.real_model = Some("default-model".to_string());
        settings
    };

    assert_eq!(
        resolve_model_route("claude-sonnet-5[0]", &settings),
        Some("nim-medium".to_string())
    );
    assert_eq!(
        resolve_model_route("claude-sonnet-5[0][1m]", &settings),
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
    routes.insert("claude-sonnet-5[0]".to_string(), "nim-medium".to_string());

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

#[test]
fn anthropic_to_openai_covers_image_and_tool_flow() {
    let settings = Settings::default();
    let body = json!({
        "model": "claude-3-5-sonnet",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "hi"}, {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abcd"}}]},
            {"role": "assistant", "content": [{"type": "thinking", "thinking": "think"}, {"type": "text", "text": "hello"}, {"type": "tool_use", "id": "toolu_1", "name": "bash", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "output"}]}, {"type": "text", "text": "after"}]}
        ],
        "tools": [{"name": "bash", "description": "run", "input_schema": {"type": "object"}}]
    });
    let (converted, _) = anthropic_to_openai_request(&body.to_string(), &settings).unwrap();
    let v: Value = serde_json::from_str(&converted).unwrap();
    assert!(v["messages"].as_array().unwrap().len() >= 3);
}

#[test]
fn anthropic_to_openai_covers_all_optional_fields() {
    let mut s = Settings::default();
    s.models.real_model_reasoning_efforts.insert(
        "claude-test".to_string(),
        vec!["low".to_string(), "high".to_string()],
    );
    let body = json!({
        "model": "claude-test",
        "max_tokens": 100,
        "temperature": 0.7,
        "top_p": 0.9,
        "stream": true,
        "stop_sequences": ["stop"],
        "thinking": {"type": "enabled", "budget_tokens": 2000},
        "tool_choice": {"type": "auto"},
        "system": "sys",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "bash", "description": "d", "input_schema": {"type": "object"}}]
    });
    let (converted, is_stream) = anthropic_to_openai_request(&body.to_string(), &s).unwrap();
    assert!(is_stream);
    let v: Value = serde_json::from_str(&converted).unwrap();
    assert_eq!(v["temperature"], 0.7);
    assert_eq!(v["top_p"], 0.9);
    assert!(v.get("stop").is_some());
    assert!(v.get("reasoning_effort").is_some());
    // tool_choice any
    let body2 = json!({"model":"claude-test","messages":[{"role":"user","content":"hi"}],"tool_choice":{"type":"any"}});
    let (c2, _) = anthropic_to_openai_request(&body2.to_string(), &s).unwrap();
    let v2: Value = serde_json::from_str(&c2).unwrap();
    assert_eq!(v2["tool_choice"], "required");
    // tool_choice tool with name
    let body3 = json!({"model":"claude-test","messages":[{"role":"user","content":"hi"}],"tool_choice":{"type":"tool","name":"bash"}});
    let (c3, _) = anthropic_to_openai_request(&body3.to_string(), &s).unwrap();
    let v3: Value = serde_json::from_str(&c3).unwrap();
    assert_eq!(v3["tool_choice"]["function"]["name"], "bash");
    // system blocks
    let body4 = json!({"model":"claude-test","system":[{"type":"text","text":"sys block"}],"messages":[{"role":"user","content":"hi"}]});
    let (c4, _) = anthropic_to_openai_request(&body4.to_string(), &s).unwrap();
    let v4: Value = serde_json::from_str(&c4).unwrap();
    assert_eq!(v4["messages"][0]["role"], "system");
}

#[test]
fn convert_helpers_cover_text_and_empty() {
    let msg = crate::models::claude::ClaudeMessage {
        role: crate::models::claude::ClaudeRole::User,
        content: crate::models::claude::ClaudeMessageContent::Text("   ".to_string()),
    };
    assert!(convert_single_user_message(&msg).is_none());
    let msg2 = crate::models::claude::ClaudeMessage {
        role: crate::models::claude::ClaudeRole::User,
        content: crate::models::claude::ClaudeMessageContent::Blocks(vec![
            crate::models::claude::ClaudeContentBlock::Text {
                text: "hello".to_string(),
            },
        ]),
    };
    assert!(convert_single_user_message(&msg2).is_some());
    // image branch
    let msg_img = crate::models::claude::ClaudeMessage {
        role: crate::models::claude::ClaudeRole::User,
        content: crate::models::claude::ClaudeMessageContent::Blocks(vec![
            crate::models::claude::ClaudeContentBlock::Image {
                source: crate::models::claude::ClaudeImageSource {
                    source_type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: "abcd".to_string(),
                },
            },
        ]),
    };
    assert!(convert_single_user_message(&msg_img).is_some());
    // system role
    let msg_sys = crate::models::claude::ClaudeMessage {
        role: crate::models::claude::ClaudeRole::System,
        content: crate::models::claude::ClaudeMessageContent::Blocks(vec![
            crate::models::claude::ClaudeContentBlock::Text {
                text: "sys".to_string(),
            },
        ]),
    };
    assert!(convert_single_user_message(&msg_sys).is_some());
}

#[test]
fn cover_tool_result_image_and_unknown() {
    // Cover tool_result_content_to_text branches for image and unknown
    let content_image = crate::models::claude::ClaudeToolResultContent::Blocks(vec![
        serde_json::json!({"type":"image","source":{"type":"base64"}}),
    ]);
    assert_eq!(tool_result_content_to_text(&Some(content_image)), "");
    let content_unknown = crate::models::claude::ClaudeToolResultContent::Blocks(vec![
        serde_json::json!({"type":"unknown","data":1}),
    ]);
    let txt = tool_result_content_to_text(&Some(content_unknown));
    assert!(txt.contains("unknown"));
    let content_text_block = crate::models::claude::ClaudeToolResultContent::Blocks(vec![
        serde_json::json!({"type":"text","text":"hi"}),
    ]);
    assert_eq!(tool_result_content_to_text(&Some(content_text_block)), "hi");
    // safety net with routes
    let mut routes = std::collections::HashMap::new();
    routes.insert("a".to_string(), "b".to_string());
    let mut s = Settings::default();
    s.models.real_model_routes = routes.clone();
    s.models.discovered_models = vec!["x".to_string()];
    assert!(safety_net_route("claude-test-1", &s).is_some());
    assert!(safety_net_route("test[2]", &s).is_some());
    // try_bracket
    assert!(extract_bracket_inner("a[inner]").is_some());
    assert!(extract_bracket_inner("a[inner[2]]").is_some());
    // try_fuzzy with existing family
    let mut s2 = Settings::default();
    s2.models
        .real_model_routes
        .insert("my-sonnet-model".to_string(), "val".to_string());
    assert!(try_fuzzy_family(Some("sonnet"), &s2).is_some());
    assert!(try_fuzzy_family(Some("sonnet"), &Settings::default()).is_none());
    // fallback
    let mut s3 = Settings::default();
    s3.models.real_model = Some("global".to_string());
    assert_eq!(fallback_global_model(&s3), Some("global".to_string()));
    // is_indexed
    assert!(is_indexed_claude_alias("claude-test-1"));
    assert!(is_indexed_claude_alias("claude-test_1"));
    assert!(!is_indexed_claude_alias("other"));
    assert!(!is_indexed_claude_alias("claude-1"));
    // apply_tools with None
    let mut data = serde_json::json!({});
    let req_none = crate::models::claude::ClaudeMessagesRequest {
        model: "t".to_string(),
        messages: vec![],
        system: None,
        max_tokens: None,
        stop_sequences: None,
        tools: None,
        tool_choice: None,
        stream: None,
        thinking: None,
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
    };
    apply_tools_mapping(&mut data, &req_none).unwrap();
    assert!(data.get("tools").is_none());
    // apply_tool_choice with None
    let mut data2 = serde_json::json!({});
    apply_tool_choice_mapping(&mut data2, &None);
    assert!(data2.get("tool_choice").is_none());
}

#[test]
fn cover_all_new_helpers() {
    // thinking_budget_to_effort branches
    assert_eq!(thinking_budget_to_effort(9000), "max");
    assert_eq!(thinking_budget_to_effort(3000), "high");
    assert_eq!(thinking_budget_to_effort(1500), "medium");
    assert_eq!(thinking_budget_to_effort(500), "low");
    assert_eq!(effort_rank("none"), Some(0));
    assert_eq!(effort_rank("unknown"), None);
    assert_eq!(
        clamp_reasoning_effort("high", &["low".to_string(), "high".to_string()]),
        Some("high")
    );
    assert_eq!(
        clamp_reasoning_effort("unknown", &["low".to_string()]),
        None
    );
    // detect_family
    assert_eq!(detect_family("sonnet"), Some("sonnet"));
    assert_eq!(detect_family("opus"), Some("opus"));
    assert_eq!(detect_family("haiku"), Some("haiku"));
    assert_eq!(detect_family("none"), None);
    let s = Settings::default();
    assert!(family_override_model(&s, "sonnet").is_none());
    assert!(try_exact_routes("a", "b", &s).is_none());
    assert!(try_discovered_direct("a", "b", &s).is_none());
    assert!(extract_bracket_inner("no bracket").is_none());
    assert!(extract_bracket_inner("a[inner]b").is_some());
    assert_eq!(extract_bracket_inner("a[inner]b").unwrap(), "inner");
    assert!(try_fuzzy_family(None, &s).is_none());
    assert!(fallback_global_model(&s).is_none());
    assert!(!is_indexed_claude_alias("claude-1"));
    assert!(is_indexed_claude_alias("claude-test-1"));
    assert!(is_indexed_claude_alias("claude-test_1"));
    assert!(safety_net_route("plain", &s).is_none());
    assert!(safety_net_route("claude-1[2]", &s).is_none());
    // system
    assert!(extract_system_text(&ClaudeSystem::Text("  hello  ".to_string())).is_some());
    assert!(
        extract_system_text(&ClaudeSystem::Blocks(vec![
            crate::models::claude::ClaudeSystemContent {
                kind: "text".to_string(),
                text: "  hello  ".to_string()
            }
        ]))
        .is_some()
    );
    assert!(extract_system_text(&ClaudeSystem::Blocks(vec![])).is_none());
    assert!(build_system_message(&None).is_none());
    assert!(build_system_message(&Some(ClaudeSystem::Text("  ".to_string()))).is_none());
    assert!(build_system_message(&Some(ClaudeSystem::Text("hi".to_string()))).is_some());
    // tool helpers
    let tool = crate::models::claude::ClaudeTool {
        name: "   ".to_string(),
        description: None,
        input_schema: None,
    };
    assert!(build_single_openai_tool(&tool).is_ok());
    let tool2 = crate::models::claude::ClaudeTool {
        name: "bash".to_string(),
        description: None,
        input_schema: None,
    };
    assert!(build_single_openai_tool(&tool2).is_err());
    let tool3 = crate::models::claude::ClaudeTool {
        name: "bash".to_string(),
        description: Some("d".to_string()),
        input_schema: Some(json!({"type":"object"})),
    };
    assert!(build_single_openai_tool(&tool3).is_ok());
    assert_eq!(map_tool_choice_type("auto", &json!({})), json!("auto"));
    assert_eq!(map_tool_choice_type("any", &json!({})), json!("required"));
    assert_eq!(
        map_tool_choice_type("tool", &json!({"name":"bash"})),
        json!({"type":"function","function":{"name":"bash"}})
    );
    assert_eq!(map_tool_choice_type("tool", &json!({})), json!("auto"));
    assert_eq!(map_tool_choice_type("unknown", &json!({})), json!("auto"));
    assert_eq!(
        combined_text_from_content(&[json!({"text":"a"}), json!({"other":1})]),
        "a"
    );
    assert_eq!(combined_text_from_content(&[]), "");
    assert_eq!(
        role_string(&crate::models::claude::ClaudeRole::System),
        "system"
    );
    assert_eq!(
        role_string(&crate::models::claude::ClaudeRole::User),
        "user"
    );
    assert_eq!(
        role_string(&crate::models::claude::ClaudeRole::Assistant),
        "user"
    );
    // collect_assistant_parts
    let content = crate::models::claude::ClaudeMessageContent::Text("hello".to_string());
    let (th, txt, tc) = collect_assistant_parts(&content);
    assert_eq!(txt, "hello");
    assert!(th.is_empty());
    assert!(tc.is_empty());
    let content2 = crate::models::claude::ClaudeMessageContent::Blocks(vec![
        crate::models::claude::ClaudeContentBlock::Text {
            text: "hi".to_string(),
        },
        crate::models::claude::ClaudeContentBlock::Thinking {
            thinking: "think".to_string(),
        },
        crate::models::claude::ClaudeContentBlock::ToolUse {
            id: "1".to_string(),
            name: "bash".to_string(),
            input: json!({}),
        },
    ]);
    let (th2, txt2, tc2) = collect_assistant_parts(&content2);
    assert_eq!(th2, "think");
    assert_eq!(txt2, "hi");
    assert_eq!(tc2.len(), 1);
    // build_assistant_message
    let m = build_assistant_message("think".to_string(), "text".to_string(), vec![]);
    assert_eq!(m["content"], "<think>think</think>text");
    let m2 = build_assistant_message("".to_string(), "".to_string(), vec![]);
    assert_eq!(m2["content"], Value::Null);
    // tool_result
    assert_eq!(tool_result_content_to_text(&None), "");
    assert_eq!(
        tool_result_content_to_text(&Some(crate::models::claude::ClaudeToolResultContent::Text(
            "hi".to_string()
        ))),
        "hi"
    );
    assert_eq!(
        tool_result_content_to_text(&Some(
            crate::models::claude::ClaudeToolResultContent::Object(json!({"a":1}))
        )),
        "{\"a\":1}"
    );
    // build_tool_messages
    let blocks = vec![crate::models::claude::ClaudeContentBlock::ToolResult {
        tool_use_id: "1".to_string(),
        content: Some(crate::models::claude::ClaudeToolResultContent::Text(
            "out".to_string(),
        )),
    }];
    assert_eq!(build_tool_messages(&blocks).len(), 1);
    assert!(build_tool_messages(&[]).is_empty());
    // after tools
    let blocks2 = vec![crate::models::claude::ClaudeContentBlock::Text {
        text: "after".to_string(),
    }];
    assert!(extract_after_tools_user_text(&blocks2).is_some());
    assert!(extract_after_tools_user_text(&[]).is_none());
    // convert_user_blocks
    let (c, has_img) = convert_user_blocks(&[crate::models::claude::ClaudeContentBlock::Text {
        text: "hi".to_string(),
    }]);
    assert!(!has_img);
    assert_eq!(c.len(), 1);
    let (c2, has_img2) = convert_user_blocks(&[crate::models::claude::ClaudeContentBlock::Image {
        source: crate::models::claude::ClaudeImageSource {
            source_type: "base64".to_string(),
            media_type: "image/png".to_string(),
            data: "abcd".to_string(),
        },
    }]);
    assert!(has_img2);
    // handle_assistant
    let mut out = Vec::new();
    let msg_assistant = crate::models::claude::ClaudeMessage {
        role: crate::models::claude::ClaudeRole::Assistant,
        content: crate::models::claude::ClaudeMessageContent::Text("hi".to_string()),
    };
    handle_assistant_message(&msg_assistant, &mut out);
    assert_eq!(out.len(), 1);
    // try_handle_tool_followup
    let mut out2 = Vec::new();
    let mut i = 0usize;
    let msgs = vec![
        crate::models::claude::ClaudeMessage {
            role: crate::models::claude::ClaudeRole::Assistant,
            content: crate::models::claude::ClaudeMessageContent::Text("hi".to_string()),
        },
        crate::models::claude::ClaudeMessage {
            role: crate::models::claude::ClaudeRole::User,
            content: crate::models::claude::ClaudeMessageContent::Blocks(vec![
                crate::models::claude::ClaudeContentBlock::ToolResult {
                    tool_use_id: "1".to_string(),
                    content: Some(crate::models::claude::ClaudeToolResultContent::Text(
                        "out".to_string(),
                    )),
                },
            ]),
        },
    ];
    try_handle_tool_followup(&msgs, &mut i, &mut out2);
    assert_eq!(out2.len(), 1);
}

#[test]
fn apply_tools_and_tool_choice_cover_all() {
    let mut data = serde_json::json!({});
    let req = crate::models::claude::ClaudeMessagesRequest {
        model: "test".to_string(),
        messages: vec![],
        system: None,
        max_tokens: None,
        stop_sequences: None,
        tools: Some(vec![crate::models::claude::ClaudeTool {
            name: "bash".to_string(),
            description: Some("run".to_string()),
            input_schema: Some(serde_json::json!({"type":"object"})),
        }]),
        tool_choice: None,
        stream: None,
        thinking: None,
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
    };
    apply_tools_mapping(&mut data, &req).unwrap();
    assert!(data.get("tools").is_some());
    // empty name tool should be skipped
    let mut data2 = serde_json::json!({});
    let req2 = crate::models::claude::ClaudeMessagesRequest {
        model: "test".to_string(),
        messages: vec![],
        system: None,
        max_tokens: None,
        stop_sequences: None,
        tools: Some(vec![crate::models::claude::ClaudeTool {
            name: "   ".to_string(),
            description: None,
            input_schema: Some(serde_json::json!({})),
        }]),
        tool_choice: None,
        stream: None,
        thinking: None,
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
    };
    apply_tools_mapping(&mut data2, &req2).unwrap();
    assert!(data2.get("tools").is_none());
    // tool_choice
    let mut data3 = serde_json::json!({});
    apply_tool_choice_mapping(&mut data3, &Some(serde_json::json!({"type":"auto"})));
    assert_eq!(data3["tool_choice"], "auto");
    let mut data4 = serde_json::json!({});
    apply_tool_choice_mapping(&mut data4, &Some(serde_json::json!({"type":"any"})));
    assert_eq!(data4["tool_choice"], "required");
    let mut data5 = serde_json::json!({});
    apply_tool_choice_mapping(
        &mut data5,
        &Some(serde_json::json!({"type":"tool","name":"bash"})),
    );
    assert_eq!(data5["tool_choice"]["function"]["name"], "bash");
    let mut data6 = serde_json::json!({});
    apply_tool_choice_mapping(&mut data6, &Some(serde_json::json!({"type":"tool"})));
    assert_eq!(data6["tool_choice"], "auto");
    let mut data7 = serde_json::json!({});
    apply_tool_choice_mapping(&mut data7, &Some(serde_json::json!({"type":"unknown"})));
    assert_eq!(data7["tool_choice"], "auto");
    assert_eq!(
        map_tool_choice_type("auto", &serde_json::json!({})),
        serde_json::json!("auto")
    );
}
