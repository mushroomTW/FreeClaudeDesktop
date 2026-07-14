use super::*;

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
