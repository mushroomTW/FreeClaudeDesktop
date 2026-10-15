use free_claude_core::{Settings, anthropic_to_openai_request};
use serde_json::{Value, json};
use std::collections::HashMap;

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings
        .models
        .real_model_routes
        .insert("claude-opus-5[0]".into(), "reasoning-model".into());
    settings.models.real_model_reasoning_efforts.insert(
        "claude-opus-5[0]".into(),
        ["none", "low", "medium", "high", "max"]
            .map(String::from)
            .to_vec(),
    );
    settings
}

fn convert(body: Value) -> Value {
    let (body, _) = anthropic_to_openai_request(&body.to_string(), &settings()).unwrap();
    serde_json::from_str(&body).unwrap()
}

#[test]
fn desktop_compat_thinking_modes_and_explicit_effort() {
    // 使用實際 JSON 介面驗證新版與舊版請求，避免型別預設值掩蓋問題。
    for (thinking, output, expected) in [
        (json!({"type":"adaptive"}), json!({"effort":"max"}), "max"),
        (json!({"type":"disabled"}), json!({"effort":"max"}), "none"),
        (json!({"enabled":false}), Value::Null, "none"),
        (
            json!({"type":"enabled","budget_tokens":4096}),
            Value::Null,
            "high",
        ),
    ] {
        let value = convert(json!({"model":"claude-opus-5[0]",
            "messages":[{"role":"user","content":"測試"}],
            "thinking":thinking,"output_config":output}));
        assert_eq!(value["reasoning_effort"], expected);
    }
}

#[test]
fn desktop_compat_1m_keeps_reasoning_capabilities() {
    for model in [
        "claude-opus-5[0]",
        "claude-opus-5[0][1m]",
        "claude-opus-5[0][1M]",
    ] {
        let value = convert(json!({"model":model,
            "messages":[{"role":"user","content":"測試"}],
            "thinking":{"type":"enabled","budget_tokens":4096}}));
        assert_eq!(value["model"], "reasoning-model");
        assert_eq!(value["reasoning_effort"], "high");
    }
}

#[test]
fn desktop_compat_screenshots_follow_all_tool_results() {
    let value = convert(json!({"model":"claude-opus-5[0]","messages":[
        {"role":"assistant","content":[
            {"type":"tool_use","id":"shot","name":"app_screenshot","input":{}},
            {"type":"tool_use","id":"info","name":"app_info","input":{}}]},
        {"role":"user","content":[
            {"type":"tool_result","tool_use_id":"shot","content":[
                {"type":"text","text":"Screenshot size: 100x100"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aW1hZ2U="}}]},
            {"type":"tool_result","tool_use_id":"info","content":"視窗資訊"},
            {"type":"text","text":"點擊右上角"},
            {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"dXNlcg=="}}]}
    ]}));
    let messages = value["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "shot");
    assert_eq!(messages[2]["tool_call_id"], "info");
    assert_eq!(messages[3]["role"], "user");
    let content = messages[3]["content"]
        .as_array()
        .expect("截圖必須保留在多模態內容中");
    assert!(
        content
            .iter()
            .any(|part| part["image_url"]["url"] == "data:image/png;base64,aW1hZ2U=")
    );
    assert!(content.iter().any(|part| part["text"] == "點擊右上角"));
    assert!(
        content
            .iter()
            .any(|part| part["image_url"]["url"] == "data:image/jpeg;base64,dXNlcg==")
    );
    assert!(content.iter().any(|part| {
        part["text"]
            .as_str()
            .is_some_and(|text| text.contains("shot"))
    }));
}

#[test]
fn desktop_compat_explicit_false_1m_preference_survives_config() {
    let normalized = free_claude_core::normalize_models_response_with_overrides_and_prefer1m(
        json!({"data":[{"id":"long-context","max_input_tokens":1000000}]}),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::from([("long-context".into(), false)]),
    )
    .unwrap();
    assert_eq!(normalized.data[0].prefer1m, Some(false));
    let models = free_claude_core::build_inference_models(&normalized.data);
    let config = free_claude_core::launcher::claude_config(3000, &models, "test-token");
    assert_eq!(config["inferenceModels"][0]["supports1m"], true);
    assert_eq!(config["inferenceModels"][0]["prefer1m"], false);
}

#[test]
fn desktop_compat_profile_and_mcp_share_desktop_directory() {
    let profile = free_claude_core::launcher::mirror_profile_dir();
    assert!(profile.ends_with("Claude-3p"));
    assert_eq!(free_claude_core::launcher::user_data_dir(), profile);
    assert_eq!(
        free_claude_core::launcher::config_lib_dir(),
        profile.join("configLibrary")
    );
    assert_eq!(
        free_claude_core::launcher::mcp_config_paths(),
        vec![profile.join("claude_desktop_config.json")]
    );
}

#[test]
fn desktop_compat_1m_without_preference_keeps_standard_variant() {
    let normalized = free_claude_core::normalize_models_response(json!({
        "data":[{"id":"long-context","max_input_tokens":1000000}]
    }))
    .unwrap();
    assert_eq!(normalized.data[0].prefer1m, None);
    let models = free_claude_core::build_inference_models(&normalized.data);
    let config = free_claude_core::launcher::claude_config(3000, &models, "test-token");
    assert_eq!(config["inferenceModels"][0]["prefer1m"], false);
}

#[test]
fn desktop_compat_adaptive_defaults_and_effort_without_thinking() {
    let adaptive = convert(json!({"model":"claude-opus-5[0]",
        "messages":[{"role":"user","content":"測試"}],"thinking":{"type":"adaptive"}}));
    assert!(adaptive.get("reasoning_effort").is_none());
    let explicit = convert(json!({"model":"claude-opus-5[0]",
        "messages":[{"role":"user","content":"測試"}],"output_config":{"effort":"xhigh"}}));
    assert_eq!(explicit["reasoning_effort"], "max");
}

#[test]
fn desktop_compat_screenshot_formats_and_invalid_image() {
    for (image, expected) in [
        (
            json!({"type":"image","source":{"type":"url","url":"https://example.test/shot.png"}}),
            "https://example.test/shot.png",
        ),
        (
            json!({"type":"image","mimeType":"image/png","data":"aW1hZ2U="}),
            "data:image/png;base64,aW1hZ2U=",
        ),
    ] {
        let value = convert(json!({"model":"claude-opus-5[0]","messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"shot","name":"screenshot","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"shot","content":[image]}]}
        ]}));
        assert_eq!(
            value["messages"][2]["content"][1]["image_url"]["url"],
            expected
        );
    }
    let invalid = json!({"model":"claude-opus-5[0]","messages":[
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"shot","content":[{"type":"image"}]}]}
    ]});
    assert!(
        anthropic_to_openai_request(&invalid.to_string(), &settings())
            .unwrap_err()
            .contains("image")
    );
}
