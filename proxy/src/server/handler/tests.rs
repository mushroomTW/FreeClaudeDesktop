use super::*;

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
/// 驗證 Dashboard 頁面與拆分後資源的引用、MIME type 及快取標頭。
async fn dashboard_assets_are_embedded_and_served_with_expected_headers() {
    let dashboard = handle_dashboard_page().await.into_response();
    let dashboard_body = axum::body::to_bytes(dashboard.into_body(), usize::MAX)
        .await
        .expect("Dashboard HTML 應可讀取");
    let dashboard_html =
        String::from_utf8(dashboard_body.to_vec()).expect("Dashboard HTML 應為 UTF-8");
    assert!(dashboard_html.contains("href=\"/dashboard.css\""));
    assert!(dashboard_html.contains("src=\"/dashboard.js\""));
    assert!(dashboard_html.contains("id=\"restoreOfficialBtn\""));
    assert!(!dashboard_html.contains("<style>"));
    assert!(!dashboard_html.contains("<script>"));
    assert!(!dashboard_html.contains(" style="));
    assert!(!dashboard_html.contains(" onclick="));

    let css = handle_dashboard_css().await.into_response();
    assert_eq!(
        css.headers().get("content-type").unwrap(),
        "text/css; charset=utf-8"
    );
    assert_eq!(
        css.headers().get("cache-control").unwrap(),
        "no-cache, no-store, must-revalidate"
    );
    let css_body = axum::body::to_bytes(css.into_body(), usize::MAX)
        .await
        .expect("CSS 應可讀取");
    assert!(String::from_utf8_lossy(&css_body).contains(":root"));

    let js = handle_dashboard_js().await.into_response();
    assert_eq!(
        js.headers().get("content-type").unwrap(),
        "text/javascript; charset=utf-8"
    );
    let js_body = axum::body::to_bytes(js.into_body(), usize::MAX)
        .await
        .expect("JavaScript 應可讀取");
    let js_text = String::from_utf8_lossy(&js_body);
    assert!(js_text.contains("translations"));
    assert!(js_text.contains("RestoreSettings"));
}

#[test]
/// 驗證 `test_is_model_gone_or_invalid_error` 的行為符合預期。
fn test_is_model_gone_or_invalid_error() {
    assert!(is_model_gone_or_invalid_error("model not found"));
    assert!(is_model_gone_or_invalid_error("invalid model name"));
    assert!(is_model_gone_or_invalid_error(
        "DEGRADED function cannot be invoked"
    ));
    assert!(!is_model_gone_or_invalid_error("some normal error"));
}

#[test]
/// 驗證 `streaming_retry_is_allowed_only_before_output` 的行為符合預期。
fn streaming_retry_is_allowed_only_before_output() {
    assert!(may_retry_stale_model(false, true, "model_not_found"));
    assert!(!may_retry_stale_model(true, true, "model_not_found"));
    assert!(!may_retry_stale_model(false, false, "model_not_found"));
}

#[test]
/// 驗證只有格式受限的短連線探測會接受空白上游成功回應。
fn short_connection_probe_requires_constrained_shape() {
    assert!(is_short_connection_probe(
        r#"{"model":"test","messages":[{"role":"user","content":"Hi"}],"max_tokens":1}"#
    ));
    assert!(!is_short_connection_probe(
        r#"{"model":"test","messages":[{"role":"user","content":"Hi"}],"max_tokens":2}"#
    ));
    assert!(!is_short_connection_probe(
        r#"{"model":"test","messages":[{"role":"user","content":"This is a normal user message that must not be swallowed."}],"max_tokens":1}"#
    ));
    assert!(!is_short_connection_probe(
        r#"{"model":"test","messages":[{"role":"user","content":"Hi"}],"max_tokens":1,"tools":[]}"#
    ));
}

#[tokio::test]
/// 驗證探測請求遇到上游非 JSON 回應時會收到合法的 Claude 探測結果。
async fn invalid_probe_response_is_replaced_with_probe_success() {
    let request_body =
        r#"{"model":"test","messages":[{"role":"user","content":"Hi"}],"max_tokens":1}"#;
    let response = invalid_openai_response(
        reqwest::StatusCode::OK,
        "gateway temporarily returned plain text",
        "上游 OpenAI 回應不是有效 JSON",
        request_body,
        "test",
    );

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["type"], "message");
    assert_eq!(body["model"], "test");
}

#[tokio::test]
/// 驗證一般請求遇到上游非 JSON 回應時會收到 HTTP 502 與診斷資訊。
async fn invalid_normal_response_returns_bad_gateway_diagnostics() {
    let request_body = r#"{"model":"test","messages":[{"role":"user","content":"A normal request"}],"max_tokens":128}"#;
    let response = invalid_openai_response(
        reqwest::StatusCode::OK,
        "gateway temporarily returned plain text",
        "上游 OpenAI 回應不是有效 JSON",
        request_body,
        "test",
    );

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("不是有效的 OpenAI JSON")
    );
    assert_eq!(body["upstreamStatus"], 200);
    assert_eq!(
        body["responseBody"],
        "gateway temporarily returned plain text"
    );
}

#[tokio::test]
/// 驗證診斷本文會限制長度，避免回傳過大的上游內容。
async fn invalid_response_preview_is_bounded() {
    let request_body = r#"{"model":"test","messages":[{"role":"user","content":"A normal request"}],"max_tokens":128}"#;
    let response_body = "x".repeat(MAX_UPSTREAM_ERROR_PREVIEW_CHARS + 100);
    let response = invalid_openai_response(
        reqwest::StatusCode::OK,
        &response_body,
        "invalid JSON",
        request_body,
        "test",
    );

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body["responseBody"].as_str().unwrap().chars().count(),
        MAX_UPSTREAM_ERROR_PREVIEW_CHARS + 3
    );
}

#[test]
/// 驗證 `request_diagnostic_contains_no_user_content` 的行為符合預期。
fn request_diagnostic_contains_no_user_content() {
    let body = r#"{"messages":[{"role":"user","content":"TOP SECRET prompt"}],"max_tokens":42,"stream":true}"#;
    let diagnostic = request_diagnostic(body).unwrap();
    assert!(!diagnostic.contains("TOP SECRET"));
    assert!(!diagnostic.contains("prompt"));
    assert!(diagnostic.contains("msgs=1"));
    assert!(diagnostic.contains(&format!("body_len={}", body.len())));
}

#[test]
/// 驗證 `test_to_public_config_excludes_plaintext_api_key` 的行為符合預期。
fn test_to_public_config_excludes_plaintext_api_key() {
    let settings = {
        let mut settings = Settings::default();
        settings.gateway.real_api_key = "sk-test-123456789".to_string();
        settings
    };

    let public_cfg = to_public_config(&settings);

    // Verify hasApiKey is true and no plain text api key is leaked
    assert!(public_cfg.get("hasApiKey").unwrap().as_bool().unwrap());
    assert!(public_cfg.get("realApiKey").is_none());
    assert!(public_cfg.get("apiKey").is_none());
    assert!(public_cfg.get("proxyAuthToken").is_none());
    assert!(public_cfg.get("gateway").is_none());

    let serialized = serde_json::to_string(&public_cfg).unwrap();
    assert!(!serialized.contains("sk-test-123456789"));
}

#[test]
/// 驗證 `test_build_upstream_request_native_vs_openai` 的行為符合預期。
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
/// 驗證 dashboard 初次讀取扁平設定時會成功回應並完成遷移。
async fn dashboard_settings_migrates_flat_settings_file() {
    let _guard = TEST_LOCK.lock().await;
    let temp_dir = std::env::temp_dir().join(format!(
        "fc_test_flat_settings_{}",
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

    let settings_file = free_claude_core::config::settings_file();
    let settings_dir = settings_file.parent().expect("設定檔應該有父目錄");
    std::fs::create_dir_all(settings_dir).unwrap();
    let flat_settings = serde_json::json!({
        "baseUrl": "https://gateway.example.com/v1",
        "authScheme": "bearer",
        "hasApiKey": true,
        "transportType": "openai_chat",
        "activePort": 4321,
        "themeMode": "dark",
        "language": "zh-tw"
    });
    std::fs::write(
        &settings_file,
        serde_json::to_string(&flat_settings).unwrap(),
    )
    .unwrap();

    let response = handle_dashboard_settings(HeaderMap::new())
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let public: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(public["baseUrl"], "https://gateway.example.com/v1");
    assert_eq!(public["activePort"], 4321);
    assert_eq!(public["hasApiKey"], false);

    let persisted: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_file).unwrap()).unwrap();
    assert!(persisted.get("gateway").is_some());
    assert!(persisted.get("models").is_some());
    assert!(persisted.get("optimizations").is_some());
    assert!(persisted.get("desktop").is_some());
    assert!(persisted.get("ui").is_some());
    assert!(persisted.get("activePort").is_none());
    assert_eq!(persisted["desktop"]["activePort"], 4321);

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
/// 驗證無法辨識或型別錯誤的設定不會被覆寫。
async fn dashboard_settings_rejects_invalid_flat_settings_without_overwrite() {
    let _guard = TEST_LOCK.lock().await;
    let temp_dir = std::env::temp_dir().join(format!(
        "fc_test_invalid_flat_settings_{}",
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

    let settings_file = free_claude_core::config::settings_file();
    let settings_dir = settings_file.parent().expect("設定檔應該有父目錄");
    std::fs::create_dir_all(settings_dir).unwrap();
    let original = r#"{"activePort":"not-a-port"}"#;
    std::fs::write(&settings_file, original).unwrap();

    let response = handle_dashboard_settings(HeaderMap::new())
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(std::fs::read_to_string(&settings_file).unwrap(), original);

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
/// ?????? `test_companion_offline_fails` ????????????????????????
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

    let settings_file = free_claude_core::config::settings_file();
    let settings_dir = settings_file.parent().expect("設定檔目錄");
    std::fs::create_dir_all(settings_dir).unwrap();

    let settings = {
        let mut settings = Settings::default();
        settings.gateway.real_base_url = "https://api.anthropic.com".to_string();
        settings.gateway.real_api_key = "protected_key".to_string();
        settings.gateway.real_auth_scheme = "bearer".to_string();
        settings.gateway.proxy_auth_token = "test_token".to_string();
        settings.desktop.active_port = Some(3000);
        settings
    };
    let mock_settings = serde_json::to_string(&settings).unwrap();
    std::fs::write(&settings_file, mock_settings).unwrap();

    let companion_state = CompanionState::default();

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_token".parse().unwrap());

    let response = handle_dashboard_rpc(
        axum::extract::State(companion_state),
        headers,
        Json(DashboardRpcRequest::DetectClaude),
    )
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
/// 驗證 `test_companion_forwarding_success` 的行為符合預期。
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

    let settings_file = free_claude_core::config::settings_file();
    let settings_dir = settings_file.parent().expect("設定檔目錄");
    std::fs::create_dir_all(settings_dir).unwrap();

    let settings = {
        let mut settings = Settings::default();
        settings.gateway.real_base_url = "https://api.anthropic.com".to_string();
        settings.gateway.real_api_key = "protected_key".to_string();
        settings.gateway.real_auth_scheme = "bearer".to_string();
        settings.gateway.proxy_auth_token = "test_token".to_string();
        settings.desktop.active_port = Some(3000);
        settings
    };
    let mock_settings = serde_json::to_string(&settings).unwrap();
    std::fs::write(&settings_file, mock_settings).unwrap();

    let companion_state = CompanionState::default();
    let (tx, mut rx) = mpsc::unbounded_channel::<ProxyToCompanionMessage>();
    {
        let mut active = companion_state.active().lock().await;
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

    let response = handle_dashboard_rpc(
        axum::extract::State(companion_state.clone()),
        headers,
        Json(DashboardRpcRequest::DetectClaude),
    )
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
        let mut active = companion_state.active().lock().await;
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
#[test]
fn cover_handler_new_helpers_for_85() {
    // log_headers with various sensitive and non-sensitive headers
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer token".parse().unwrap());
    headers.insert("x-api-key", "key".parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("custom", "value".parse().unwrap());
    headers.insert("origin", "http://example.com".parse().unwrap());
    log_headers(&headers);
    let empty_headers = HeaderMap::new();
    log_headers(&empty_headers);

    // is_anthropic_native
    let mut s = Settings::default();
    s.gateway.transport_type = "anthropic_messages".to_string();
    assert!(is_anthropic_native(&s));
    s.gateway.transport_type = "".to_string();
    s.gateway.real_base_url = "https://api.anthropic.com/v1".to_string();
    assert!(is_anthropic_native(&s));
    s.gateway.real_base_url = "https://example.com".to_string();
    assert!(!is_anthropic_native(&s));

    // extract_req_model
    assert_eq!(extract_req_model(r#"{"model":"test-model"}"#), "test-model");
    assert_eq!(extract_req_model("invalid json"), "unknown");
    assert_eq!(extract_req_model(r#"{"no_model":1}"#), "unknown");

    // build_target_url
    let mut s2 = Settings::default();
    s2.gateway.real_base_url = "https://api.openai.com/v1".to_string();
    assert!(build_target_url(&s2, true).is_ok());
    assert!(build_target_url(&s2, false).is_ok());
    s2.gateway.real_base_url = "not a url".to_string();
    assert!(build_target_url(&s2, true).is_err());

    // build_proxy_body
    let s3 = Settings::default();
    let body = r#"{"model":"test","messages":[{"role":"user","content":"hi"}]}"#;
    let (b, _is_stream) = build_proxy_body(body, &s3, true).unwrap();
    assert!(!b.is_empty());
    let (b2, is_stream2) = build_proxy_body(body, &s3, false).unwrap();
    assert!(!b2.is_empty());
    assert!(!is_stream2);

    // handle_probe_response with no probe
    let result = handle_probe_response(
        "no probe",
        "test",
        &s3,
        1,
        &serde_json::json!({}),
        std::time::Instant::now(),
    );
    assert!(result.is_none());
}

/// 由純文字建立可測試的 reqwest::Response（使用 http::Response 轉換）。
fn fake_response(status: u16, body: &str, ctype: &str) -> reqwest::Response {
    reqwest::Response::from(
        axum::http::Response::builder()
            .status(status)
            .header("content-type", ctype)
            .body(reqwest::Body::from(body.to_string()))
            .unwrap(),
    )
}

#[tokio::test]
async fn openai_non_stream_success_covers_all_branches() {
    let settings = Settings::default();
    let headers = HeaderMap::new();
    let probe_body = r#"{"model":"m","messages":[{"role":"user","content":"Hi"}],"max_tokens":1}"#;
    let normal_body = r#"{"model":"m","messages":[{"role":"user","content":"A normal request"}],"max_tokens":128}"#;

    let ok_json = r#"{"choices":[{"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#;
    let resp = fake_response(200, ok_json, "application/json");
    let out = handle_openai_non_stream_success(
        resp,
        StatusCode::OK,
        200,
        normal_body,
        "m",
        &settings,
        "key",
        normal_body,
        "http://x/chat/completions",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::OK);

    let resp = fake_response(200, "", "application/json");
    let out = handle_openai_non_stream_success(
        resp,
        StatusCode::OK,
        200,
        probe_body,
        "m",
        &settings,
        "key",
        probe_body,
        "http://x/chat/completions",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::OK);

    let resp = fake_response(200, "", "application/json");
    let out = handle_openai_non_stream_success(
        resp,
        StatusCode::OK,
        200,
        normal_body,
        "m",
        &settings,
        "key",
        normal_body,
        "http://x/chat/completions",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::BAD_GATEWAY);

    let resp = fake_response(200, "gateway said plain text", "text/plain");
    let out = handle_openai_non_stream_success(
        resp,
        StatusCode::OK,
        200,
        normal_body,
        "m",
        &settings,
        "key",
        normal_body,
        "http://x/chat/completions",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::BAD_GATEWAY);

    let resp = fake_response(200, "gateway said plain text", "text/plain");
    let out = handle_openai_non_stream_success(
        resp,
        StatusCode::OK,
        200,
        probe_body,
        "m",
        &settings,
        "key",
        probe_body,
        "http://x/chat/completions",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::OK);
}

#[tokio::test]
async fn openai_error_covers_retry_and_fallback() {
    let settings = Settings::default();
    let headers = HeaderMap::new();
    let body = r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":8}"#;

    let resp = fake_response(
        429,
        r#"{"error":{"message":"rate limited"}}"#,
        "application/json",
    );
    let out = handle_openai_error(
        resp,
        StatusCode::TOO_MANY_REQUESTS,
        429,
        &settings,
        "key",
        body,
        "m",
        "http://unreachable.invalid/x",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::TOO_MANY_REQUESTS);

    let resp = fake_response(500, "boom", "text/plain");
    let out = handle_openai_error(
        resp,
        StatusCode::INTERNAL_SERVER_ERROR,
        500,
        &settings,
        "key",
        body,
        "m",
        "http://unreachable.invalid/x",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn anthropic_passthrough_covers_success_and_error() {
    let settings = Settings::default();
    let headers = HeaderMap::new();
    let body = r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":8}"#;

    let resp = fake_response(
        200,
        r#"{"id":"msg_1","type":"message"}"#,
        "application/json",
    );
    let out = handle_anthropic_passthrough(
        resp,
        StatusCode::OK,
        &settings,
        "key",
        body,
        "m",
        "http://x/messages",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::OK);
    let txt = axum::body::to_bytes(out.into_body(), 8192).await.unwrap();
    assert!(String::from_utf8_lossy(&txt).contains("msg_1"));

    let resp = fake_response(500, "upstream broke", "text/plain");
    let out = handle_anthropic_passthrough(
        resp,
        StatusCode::INTERNAL_SERVER_ERROR,
        &settings,
        "key",
        body,
        "m",
        "http://unreachable.invalid/messages",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let txt = axum::body::to_bytes(out.into_body(), 8192).await.unwrap();
    assert!(String::from_utf8_lossy(&txt).contains("upstream broke"));

    let resp = fake_response(
        429,
        r#"{"type":"error","error":{"type":"rate_limit","message":"slow down"}}"#,
        "application/json",
    );
    let out = handle_anthropic_passthrough(
        resp,
        StatusCode::TOO_MANY_REQUESTS,
        &settings,
        "key",
        body,
        "m",
        "http://unreachable.invalid/messages",
        &headers,
    )
    .await;
    assert_eq!(out.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn openai_stream_response_covers_success_and_failure() {
    let settings = Settings::default();
    let headers = HeaderMap::new();
    let body = r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":8}"#;

    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n";
    let resp = fake_response(200, sse, "text/event-stream");
    let out = handle_openai_stream_response(
        resp,
        200,
        StatusCode::OK,
        &settings,
        "key",
        body,
        "m",
        "http://x/chat/completions",
        &headers,
        true,
    )
    .await;
    assert_eq!(out.status(), StatusCode::OK);
    assert!(
        out.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/event-stream")
    );

    let resp = fake_response(500, "boom", "text/plain");
    let out = handle_openai_stream_response(
        resp,
        500,
        StatusCode::INTERNAL_SERVER_ERROR,
        &settings,
        "key",
        body,
        "m",
        "http://unreachable.invalid/chat/completions",
        &headers,
        true,
    )
    .await;
    assert_eq!(out.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn dispatch_upstream_covers_network_error() {
    let settings = Settings::default();
    let headers = HeaderMap::new();
    let body = r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":8}"#;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .unwrap();
    let req = client
        .post("http://127.0.0.1:1/x")
        .header("content-type", "application/json")
        .body(body.to_string());

    let out = dispatch_upstream(
        req,
        &settings,
        "key",
        body,
        "m",
        "http://127.0.0.1:1/x",
        &headers,
        body,
        true,
        false,
        false,
        1,
        &serde_json::json!({}),
        std::time::Instant::now(),
    )
    .await;
    assert_eq!(out.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn dispatch_upstream_covers_anthropic_path() {
    let mut settings = Settings::default();
    settings.optimizations.enable_api_call_logging = true;
    let headers = HeaderMap::new();
    let body = r#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":8}"#;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .unwrap();
    let req = client
        .post("http://127.0.0.1:1/x")
        .header("content-type", "application/json")
        .body(body.to_string());

    let out = dispatch_upstream(
        req,
        &settings,
        "key",
        body,
        "m",
        "http://127.0.0.1:1/x",
        &headers,
        body,
        false,
        false,
        true,
        2,
        &serde_json::json!({"a":1}),
        std::time::Instant::now(),
    )
    .await;
    assert_eq!(out.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn api_request_summary_covers_branches() {
    let s = api_request_summary(
        r#"{"model":"m","messages":[{"role":"user"}],"tools":[{}],"system":"x","max_tokens":5,"stream":true}"#,
    );
    assert_eq!(s["model"], "m");
    assert_eq!(s["toolCount"], 1);
    assert_eq!(s["hasSystem"], true);
    assert_eq!(s["validJson"], true);
    assert_eq!(s["stream"], true);
    assert_eq!(s["maxTokens"], 5);
    let s = api_request_summary("garbage");
    assert_eq!(s["validJson"], false);
    assert_eq!(s["model"], "unknown");
    assert_eq!(s["messageCount"], 0);
}

#[test]
fn redacted_url_and_outcome_cover_branches() {
    assert_eq!(redacted_url("https://h/p?q=1#f"), "https://h/p");
    assert_eq!(redacted_url("https://h/p"), "https://h/p");

    record_api_outcome(ApiOutcome {
        enabled: false,
        call_id: 0,
        request: &json!({}),
        target_url: "http://x",
        transport: "local",
        outcome: "test",
        elapsed_ms: 0,
        status: None,
        content_type: None,
        error: None,
    });
    record_api_outcome(ApiOutcome {
        enabled: true,
        call_id: 99,
        request: &json!({"k":"v"}),
        target_url: "http://x/p?secret=1",
        transport: "local",
        outcome: "probe",
        elapsed_ms: 3,
        status: Some(200),
        content_type: Some("application/json"),
        error: Some("boom"),
    });
}

#[test]
fn reasoning_mode_and_sse_cover_branches() {
    let mut s = Settings::default();
    s.models.reasoning_replay_mode = "inline".to_string();
    assert!(matches!(
        reasoning_mode_from(&s),
        Some(ReasoningReplayMode::Inline)
    ));
    s.models.reasoning_replay_mode = "separate".to_string();
    assert!(matches!(
        reasoning_mode_from(&s),
        Some(ReasoningReplayMode::Separate)
    ));
    s.models.reasoning_replay_mode = "other".to_string();
    assert!(reasoning_mode_from(&s).is_none());

    let (tx, rx) = mpsc::channel::<Result<axum::body::Bytes, std::convert::Infallible>>(2);
    drop(tx);
    let resp = sse_stream_response(rx);
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn root_icon_healthz_cover() {
    let r = handle_root().await.into_response();
    assert_eq!(r.status(), StatusCode::OK);
    let b = axum::body::to_bytes(r.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&b).contains("proxy is running"));

    let r = handle_app_icon().await.into_response();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers().get("content-type").unwrap(), "image/png");

    let j = handle_healthz().await;
    assert_eq!(j.0["status"], "ok");
}
