use super::*;
use std::ffi::OsString;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn set_test_app_dirs(root: &Path) -> (PathBuf, PathBuf, Vec<(&'static str, Option<OsString>)>) {
    let keys = [
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "HOME",
        "USERPROFILE",
    ];
    let old = keys
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();

    #[cfg(target_os = "windows")]
    {
        std::env::set_var("APPDATA", root.join("appdata"));
        std::env::set_var("LOCALAPPDATA", root.join("local"));
        std::env::set_var("USERPROFILE", root.join("profile"));
    }
    #[cfg(target_os = "macos")]
    {
        std::env::set_var("HOME", root.join("home"));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
        std::env::set_var("HOME", root.join("home"));
    }

    (official_app_data_dir(), mirror_profile_dir(), old)
}

fn restore_env(old: Vec<(&'static str, Option<OsString>)>) {
    for (key, value) in old {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

#[test]
fn meta_upsert_preserves_existing_entries() {
    let meta = json!({
        "appliedId": "other-id",
        "entries": [
            { "id": "other-id", "name": "Other Config" }
        ]
    });

    let updated = upsert_managed_meta_entry(meta);
    let entries = updated["entries"].as_array().unwrap();

    assert_eq!(updated["appliedId"], CONFIG_ID);
    assert!(entries.iter().any(|entry| entry["id"] == "other-id"));
    assert!(entries.iter().any(|entry| entry["id"] == CONFIG_ID));
}

#[test]
fn meta_remove_only_removes_managed_entry() {
    let meta = json!({
        "appliedId": CONFIG_ID,
        "entries": [
            { "id": "other-id", "name": "Other Config" },
            { "id": CONFIG_ID, "name": "FreeClaudeDesktop" }
        ]
    });

    let updated = remove_managed_meta_entry(meta);
    let entries = updated["entries"].as_array().unwrap();

    assert_eq!(updated["appliedId"], "other-id");
    assert!(entries.iter().any(|entry| entry["id"] == "other-id"));
    assert!(!entries.iter().any(|entry| entry["id"] == CONFIG_ID));
}

#[test]
fn deployment_mode_restore_keeps_previous_value() {
    let original = json!({ "deploymentMode": "custom" });

    let applied = apply_managed_deployment_mode(original);
    assert_eq!(applied["deploymentMode"], "3p");
    assert_eq!(applied["freeClaudeDesktopPreviousDeploymentMode"], "custom");

    let restored = restore_managed_deployment_mode(applied);
    assert_eq!(restored["deploymentMode"], "custom");
    assert!(
        restored
            .get("freeClaudeDesktopPreviousDeploymentMode")
            .is_none()
    );
}

#[test]
fn anthropic_base_url_env_restore_keeps_previous_values() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!("fcl_settings_env_{}", std::process::id()));
    let (_, _, old_env) = set_test_app_dirs(&root);
    let path = claude_settings_json_path();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "autoModeEnabled": false,
            "env": {
                "ANTHROPIC_BASE_URL": "https://old.example",
                "ENABLE_TOOL_SEARCH": "false",
                "KEEP_ME": "1"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    apply_anthropic_base_url_env(4321).unwrap();
    let applied: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(applied["autoModeEnabled"], false);
    assert_eq!(
        applied["env"]["ANTHROPIC_BASE_URL"],
        "http://127.0.0.1:4321"
    );
    assert_eq!(applied["env"]["ENABLE_TOOL_SEARCH"], "true");
    assert_eq!(applied["env"]["CLAUDE_CODE_ENABLE_AUTO_MODE"], "1");

    remove_anthropic_base_url_env().unwrap();
    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored["autoModeEnabled"], false);
    assert_eq!(restored["env"]["ANTHROPIC_BASE_URL"], "https://old.example");
    assert_eq!(restored["env"]["ENABLE_TOOL_SEARCH"], "false");
    assert_eq!(restored["env"]["KEEP_ME"], "1");
    assert!(
        restored["env"]
            .get("CLAUDE_CODE_ENABLE_AUTO_MODE")
            .is_none()
    );
    assert!(restored.get(PREVIOUS_CLAUDE_SETTINGS_KEY).is_none());

    restore_env(old_env);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn invalid_claude_settings_are_not_replaced_with_empty_json() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!("fcl_invalid_json_{}", std::process::id()));
    let (_, _, old_env) = set_test_app_dirs(&root);
    let path = claude_settings_json_path();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{broken").unwrap();
    let result = apply_anthropic_base_url_env(3000);
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{broken");
    restore_env(old_env);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gateway_port_rewrite_updates_config_value() {
    let updated = with_gateway_port(
        json!({
            "inferenceProvider": "gateway",
            "inferenceGatewayBaseUrl": "http://127.0.0.1:3000",
            "other": true
        }),
        4567,
    );

    assert_eq!(updated["inferenceGatewayBaseUrl"], "http://127.0.0.1:4567");
    assert_eq!(updated["other"], true);
}

#[test]
fn update_applied_claude_config_keeps_unit_return_signature() {
    let update: fn(u16, &[crate::models::openai::InferenceModel]) = update_applied_claude_config;
    let _ = update;
}

#[test]
fn strip_removed_computer_mcp_keeps_unrelated_servers() {
    let cleaned = strip_removed_computer_mcp(json!({
        "mcpServers": {
            "free-claude-computer": { "command": "old.exe" },
            "launcher-computer": { "command": "old.exe" },
            "custom": { "command": "node" }
        }
    }));
    assert!(cleaned["mcpServers"].get("free-claude-computer").is_none());
    assert!(cleaned["mcpServers"].get("launcher-computer").is_none());
    assert_eq!(cleaned["mcpServers"]["custom"]["command"], "node");
}

#[test]
fn claude_config_uses_supported_1m_variant_without_double_label() {
    let model = crate::models::openai::InferenceModel {
        name: "claude-sonnet-4-6[0][1m]".to_string(),
        label_override: "deepseek-v4-flash 1M".to_string(),
        provider_model_id: "deepseek-v4-flash".to_string(),
        display_name: "deepseek-v4-flash 1M".to_string(),
        max_input_tokens: Some(1_000_000),
        max_tokens: Some(8192),
        capabilities: serde_json::json!({}),
        supports1m: None,
        transport_type: None,
    };

    let config = claude_config(12345, &[model], "proxy-token");
    let models = config["inferenceModels"].as_array().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["name"], "claude-sonnet-4-6[0]");
    assert_eq!(models[0]["labelOverride"], "deepseek-v4-flash");
    assert_eq!(models[0]["displayName"], "deepseek-v4-flash");
    assert_eq!(models[0]["supports1m"], true);
}

#[test]
fn claude_config_enables_chat_and_extensions_by_default() {
    let config = claude_config(12345, &[], "proxy-token");
    assert_eq!(config["coworkTabEnabled"], true);
    assert_eq!(config["isClaudeCodeForDesktopEnabled"], true);
    assert_eq!(config["chatTabEnabled"], true);
    assert_eq!(config["isDesktopExtensionEnabled"], true);
    assert_eq!(config["extensions"]["enabled"], true);
}

#[test]
fn clean_json_text_strips_comments_bom_and_trailing_commas() {
    let raw = "\u{feff}{\n  // line comment\n  \"mcpServers\": {\n    /* block comment */\n    \"custom\": { \"command\": \"node\", },\n  },\n}";
    let cleaned = clean_json_text(raw);
    let parsed: Value = serde_json::from_str(&cleaned).expect("Failed to parse cleaned json");
    assert_eq!(parsed["mcpServers"]["custom"]["command"], "node");
}

#[test]
fn clean_json_text_robust_boundary_scenarios() {
    let raw = "\u{feff}{
        // line comment
        \"url\": \"http://example.com/api\",
        \"comment_block_in_str\": \"/* this is not a comment */\",
        \"escaped_quote_in_str\": \"hello \\\"world\\\"\",
        \"escaped_slash_in_str\": \"hello \\/ world\",
        \"unicode_escapes\": \"\\u0022hello\\u0022\",
        \"unicode_backslash\": \"\\u005c\",
        \"array_with_trailing\": [
            1,
            2,
            3,
        ],
        \"object_with_trailing\": {
            \"a\": 1,
            \"b\": 2,
        },
    }";
    let cleaned = clean_json_text(raw);
    let parsed: Value =
        serde_json::from_str(&cleaned).unwrap_or_else(|_| panic!("Failed to parse: {}", cleaned));
    assert_eq!(parsed["url"], "http://example.com/api");
    assert_eq!(
        parsed["comment_block_in_str"],
        "/* this is not a comment */"
    );
    assert_eq!(parsed["escaped_quote_in_str"], "hello \"world\"");
    assert_eq!(parsed["escaped_slash_in_str"], "hello / world");
    assert_eq!(parsed["unicode_escapes"], "\"hello\"");
    assert_eq!(parsed["unicode_backslash"], "\\");
    assert_eq!(parsed["array_with_trailing"].as_array().unwrap().len(), 3);
    assert_eq!(parsed["object_with_trailing"]["b"], 2);
}

#[test]
fn merge_mcp_servers_preserves_and_merges_custom_servers() {
    let mut servers = serde_json::Map::new();
    servers.insert("user_mcp".to_string(), json!({ "command": "python" }));

    let data = json!({
        "deploymentMode": "3p",
        "mcpServers": {
            "existing_mcp": { "command": "node" }
        }
    });

    let merged = merge_mcp_servers(data, &servers);
    assert_eq!(merged["mcpServers"]["existing_mcp"]["command"], "node");
    assert_eq!(merged["mcpServers"]["user_mcp"]["command"], "python");
}

#[test]
fn invalid_official_mcp_config_stops_deployment_write() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!("fcl_invalid_official_{}", std::process::id()));
    let (official, mirror_profile, old_env) = set_test_app_dirs(&root);
    let official_config = official.join("claude_desktop_config.json");
    let mirror_config = mirror_profile.join("claude_desktop_config.json");

    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(official_config.parent().unwrap()).unwrap();
    fs::create_dir_all(mirror_config.parent().unwrap()).unwrap();
    fs::write(&official_config, "{broken").unwrap();
    fs::write(&mirror_config, r#"{"deploymentMode":"1p"}"#).unwrap();

    let result = apply_3p_deployment_mode();
    let mirror_contents = fs::read_to_string(&mirror_config).unwrap();

    restore_env(old_env);
    let _ = fs::remove_dir_all(&root);

    assert!(result.is_err());
    assert_eq!(mirror_contents, r#"{"deploymentMode":"1p"}"#);
}

#[test]
fn mirror_profile_dir_returns_valid_path() {
    let mirror = mirror_profile_dir();
    assert!(mirror.to_string_lossy().contains("FreeClaudeDesktop"));
    assert!(mirror.to_string_lossy().contains("claude_profile"));
}

#[test]
fn official_app_data_dir_returns_valid_path() {
    let official = official_app_data_dir();
    assert!(official.to_string_lossy().contains("Claude"));
}

#[test]
fn copy_dir_all_recursively_copies_files() {
    let temp_src = std::env::temp_dir().join(format!("fcl_test_src_{}", std::process::id()));
    let temp_dst = std::env::temp_dir().join(format!("fcl_test_dst_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_src);
    let _ = fs::remove_dir_all(&temp_dst);

    let sub_dir = temp_src.join("subdir");
    fs::create_dir_all(&sub_dir).unwrap();
    fs::write(sub_dir.join("test.txt"), "hello").unwrap();

    copy_dir_all(&temp_src, &temp_dst).unwrap();
    assert_eq!(
        fs::read_to_string(temp_dst.join("subdir").join("test.txt")).unwrap(),
        "hello"
    );

    let _ = fs::remove_dir_all(&temp_src);
    let _ = fs::remove_dir_all(&temp_dst);
}

#[test]
fn resync_from_official_returns_copy_errors() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!("fcl_resync_error_{}", std::process::id()));
    let (official, mirror_profile, old_env) = set_test_app_dirs(&root);

    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&official).unwrap();
    fs::create_dir_all(mirror_profile.join("conflict")).unwrap();
    fs::write(official.join("conflict"), "official").unwrap();

    let result = resync_from_official();

    restore_env(old_env);
    let _ = fs::remove_dir_all(&root);

    assert!(result.is_err());
}

#[test]
fn reset_mirror_profile_restores_existing_profile_on_init_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!("fcl_reset_error_{}", std::process::id()));
    let (official, mirror_profile, old_env) = set_test_app_dirs(&root);

    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&mirror_profile).unwrap();
    fs::write(mirror_profile.join("keep.txt"), "keep").unwrap();
    fs::create_dir_all(official.parent().unwrap()).unwrap();
    fs::write(&official, "not a directory").unwrap();

    let result = reset_mirror_profile();

    restore_env(old_env);
    let restored = fs::read_to_string(mirror_profile.join("keep.txt"));
    let _ = fs::remove_dir_all(&root);

    assert!(result.is_err());
    assert_eq!(restored.unwrap(), "keep");
}
