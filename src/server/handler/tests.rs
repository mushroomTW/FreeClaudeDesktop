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
