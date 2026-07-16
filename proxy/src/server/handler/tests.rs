use super::*;

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[test]
fn test_is_model_gone_or_invalid_error() {
    assert!(is_model_gone_or_invalid_error("model not found"));
    assert!(is_model_gone_or_invalid_error("invalid model name"));
    assert!(is_model_gone_or_invalid_error(
        "DEGRADED function cannot be invoked"
    ));
    assert!(!is_model_gone_or_invalid_error("some normal error"));
}

#[test]
fn streaming_retry_is_allowed_only_before_output() {
    assert!(may_retry_stale_model(false, true, "model_not_found"));
    assert!(!may_retry_stale_model(true, true, "model_not_found"));
    assert!(!may_retry_stale_model(false, false, "model_not_found"));
}

#[test]
fn request_diagnostic_contains_no_user_content() {
    let body = r#"{"messages":[{"role":"user","content":"TOP SECRET prompt"}],"max_tokens":42,"stream":true}"#;
    let diagnostic = request_diagnostic(body).unwrap();
    assert!(!diagnostic.contains("TOP SECRET"));
    assert!(!diagnostic.contains("prompt"));
    assert!(diagnostic.contains("msgs=1"));
    assert!(diagnostic.contains(&format!("body_len={}", body.len())));
}

#[test]
fn test_to_public_config_excludes_plaintext_api_key() {
    let settings = Settings {
        real_api_key: "sk-test-123456789".to_string(),
        ..Settings::default()
    };

    let public_cfg = to_public_config(&settings);

    // Verify hasApiKey is true and no plain text api key is leaked
    assert!(public_cfg.get("hasApiKey").unwrap().as_bool().unwrap());
    assert!(public_cfg.get("realApiKey").is_none());
    assert!(public_cfg.get("apiKey").is_none());
    assert!(public_cfg.get("proxyAuthToken").is_none());

    let serialized = serde_json::to_string(&public_cfg).unwrap();
    assert!(!serialized.contains("sk-test-123456789"));
}

#[test]
fn test_build_upstream_request_native_vs_openai() {
    let client = reqwest::Client::new();
    let target_url = "https://api.anthropic.com/v1/messages";
    let body = "{}".to_string();

    let mut headers = HeaderMap::new();
    headers.insert("host", "api.anthropic.com".parse().unwrap());
    headers.insert("content-length", "2".parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("x-api-key", "dummy-key-from-client".parse().unwrap());
    headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
    headers.insert("custom-header-foo", "bar-value".parse().unwrap());

    // Case 1: is_anthropic_native = true
    let req_native = build_upstream_request(
        &client,
        target_url,
        body.clone(),
        &headers,
        "new-api-key",
        "x-api-key",
        true,
    )
    .unwrap()
    .build()
    .unwrap();

    let h_native = req_native.headers();
    assert_eq!(h_native.get("x-api-key").unwrap(), "new-api-key");
    assert_eq!(h_native.get("anthropic-version").unwrap(), "2023-06-01");
    assert_eq!(h_native.get("custom-header-foo").unwrap(), "bar-value");
    assert_eq!(h_native.get("content-type").unwrap(), "application/json");
    assert!(h_native.get("host").is_none());
    assert!(h_native.get("content-length").is_none());

    // Case 2: is_anthropic_native = false
    let req_openai = build_upstream_request(
        &client,
        target_url,
        body.clone(),
        &headers,
        "new-api-key",
        "x-api-key",
        false,
    )
    .unwrap()
    .build()
    .unwrap();

    let h_openai = req_openai.headers();
    assert_eq!(h_openai.get("x-api-key").unwrap(), "new-api-key");
    assert_eq!(h_openai.get("anthropic-version").unwrap(), "2023-06-01");
    assert_eq!(h_openai.get("content-type").unwrap(), "application/json");
    assert!(h_openai.get("custom-header-foo").is_none());
    assert!(h_openai.get("host").is_none());
    assert!(h_openai.get("content-length").is_none());
}

#[tokio::test]
async fn test_companion_offline_fails() {
    let _guard = TEST_LOCK.lock().await;
    let temp_dir = std::env::temp_dir().join(format!(
        "fc_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let old_local = std::env::var_os("LOCALAPPDATA");
    let old_home = std::env::var_os("HOME");
    let old_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");

    unsafe {
        std::env::set_var("LOCALAPPDATA", &temp_dir);
        std::env::set_var("HOME", &temp_dir);
        std::env::set_var("XDG_CONFIG_HOME", &temp_dir);
    }

    let settings_file = crate::config::settings_file();
    let settings_dir = settings_file.parent().expect("設定檔目錄");
    std::fs::create_dir_all(settings_dir).unwrap();

    let settings = Settings {
        real_base_url: "https://api.anthropic.com".to_string(),
        real_api_key: "protected_key".to_string(),
        real_auth_scheme: "bearer".to_string(),
        proxy_auth_token: "test_token".to_string(),
        active_port: Some(3000),
        ..Settings::default()
    };
    let mock_settings = serde_json::to_string(&settings).unwrap();
    std::fs::write(&settings_file, mock_settings).unwrap();

    {
        let mut active = ACTIVE_COMPANION.lock().await;
        *active = None;
    }

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_token".parse().unwrap());

    let response = handle_admin_rpc(headers, Json(AdminRpcRequest::DetectClaude))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let _ = std::fs::remove_dir_all(&temp_dir);
    unsafe {
        if let Some(val) = old_local {
            std::env::set_var("LOCALAPPDATA", val);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }
        if let Some(val) = old_home {
            std::env::set_var("HOME", val);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(val) = old_xdg_config_home {
            std::env::set_var("XDG_CONFIG_HOME", val);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
}

#[tokio::test]
async fn test_companion_forwarding_success() {
    let _guard = TEST_LOCK.lock().await;
    let temp_dir = std::env::temp_dir().join(format!(
        "fc_test_fwd_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let old_local = std::env::var_os("LOCALAPPDATA");
    let old_home = std::env::var_os("HOME");
    let old_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");

    unsafe {
        std::env::set_var("LOCALAPPDATA", &temp_dir);
        std::env::set_var("HOME", &temp_dir);
        std::env::set_var("XDG_CONFIG_HOME", &temp_dir);
    }

    let settings_file = crate::config::settings_file();
    let settings_dir = settings_file.parent().expect("設定檔目錄");
    std::fs::create_dir_all(settings_dir).unwrap();

    let settings = Settings {
        real_base_url: "https://api.anthropic.com".to_string(),
        real_api_key: "protected_key".to_string(),
        real_auth_scheme: "bearer".to_string(),
        proxy_auth_token: "test_token".to_string(),
        active_port: Some(3000),
        ..Settings::default()
    };
    let mock_settings = serde_json::to_string(&settings).unwrap();
    std::fs::write(&settings_file, mock_settings).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel::<ProxyToCompanionMessage>();
    {
        let mut active = ACTIVE_COMPANION.lock().await;
        *active = Some(ActiveCompanion { tx });
    }

    let mock_companion_task = tokio::spawn(async move {
        if let Some(msg) = rx.recv().await {
            let payload: Value = serde_json::from_str(&msg.payload).unwrap();
            assert_eq!(payload["method"], "DetectClaude");
            assert!(payload.get("token").is_none());

            let _ = msg.response_tx.send(Ok(serde_json::json!({
                "path": "mock_host_path"
            })));
        }
    });

    let headers = HeaderMap::new();

    let response = handle_admin_rpc(headers, Json(AdminRpcRequest::DetectClaude))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let body_val: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_val["result"]["path"], "mock_host_path");

    mock_companion_task.await.unwrap();
    {
        let mut active = ACTIVE_COMPANION.lock().await;
        *active = None;
    }
    let _ = std::fs::remove_dir_all(&temp_dir);
    unsafe {
        if let Some(val) = old_local {
            std::env::set_var("LOCALAPPDATA", val);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }
        if let Some(val) = old_home {
            std::env::set_var("HOME", val);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(val) = old_xdg_config_home {
            std::env::set_var("XDG_CONFIG_HOME", val);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
}
