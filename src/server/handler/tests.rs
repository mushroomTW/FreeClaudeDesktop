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
