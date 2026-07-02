//! Web server tool handling (web_search / web_fetch).
//!
//! Provides local egress policy and validation for web_fetch tool calls
//! to prevent SSRF and enforce allowlists.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Egress policy for local web_fetch tool execution.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WebFetchEgressPolicy {
    #[serde(default)]
    pub allow_schemes: HashSet<String>,
    #[serde(default = "crate::config::default_true")]
    pub allow_private_networks: bool,
    #[serde(default = "crate::config::default_true")]
    pub enabled: bool,
}

impl Default for WebFetchEgressPolicy {
    fn default() -> Self {
        Self {
            allow_schemes: default_web_fetch_schemes(),
            allow_private_networks: true,
            enabled: true,
        }
    }
}

fn default_web_fetch_schemes() -> HashSet<String> {
    let mut set = HashSet::new();
    set.insert("https".to_string());
    set
}

/// Validate a URL against the egress policy.
/// Returns Ok(()) if allowed, Err(message) if blocked.
pub fn validate_url(policy: &WebFetchEgressPolicy, url: &str) -> Result<(), String> {
    if !policy.enabled {
        return Err("Web fetch is disabled by policy".to_string());
    }

    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;

    let scheme = parsed.scheme();
    if !policy.allow_schemes.contains(scheme) {
        return Err(format!(
            "URL scheme '{}' is not allowed. Allowed: {:?}",
            scheme, policy.allow_schemes
        ));
    }

    if !policy.allow_private_networks {
        if let Some(host) = parsed.host_str() {
            let normalized = host.to_lowercase();
            if is_private_address(&normalized) {
                return Err(format!("Private network access is not allowed: {host}"));
            }
        }
    }

    Ok(())
}

/// Check if a hostname is a private/local address.
fn is_private_address(host: &str) -> bool {
    // localhost variants
    if host == "localhost" || host.ends_with(".local") {
        return true;
    }

    // IPv4 private ranges
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() == 4 {
        if let Ok(first) = parts[0].parse::<u8>() {
            // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            if first == 10
                || (first == 172
                    && parts.len() > 1
                    && parts[1].parse::<u8>().unwrap_or(0) >= 16
                    && parts[1].parse::<u8>().unwrap_or(255) <= 31)
                || (first == 192 && parts.len() > 1 && parts[1] == "168")
            {
                return true;
            }
            // 127.0.0.0/8
            if first == 127 {
                return true;
            }
        }
    }

    // IPv6 loopback
    if host == "::1" || host == "::ffff:127.0.0.1" {
        return true;
    }

    false
}

/// Extract a URL from a web_fetch tool call's arguments JSON
pub fn extract_web_fetch_url(args: &Value) -> Option<String> {
    args.get("url")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

/// Check if a tool name is a web server tool
pub fn is_web_server_tool(tool_name: &str) -> bool {
    let name = tool_name.to_lowercase();
    name == "web_fetch" || name == "web_search"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_address() {
        assert!(is_private_address("localhost"));
        assert!(is_private_address("127.0.0.1"));
        assert!(is_private_address("10.0.0.1"));
        assert!(is_private_address("192.168.1.1"));
        assert!(!is_private_address("example.com"));
        assert!(!is_private_address("8.8.8.8"));
    }

    #[test]
    fn test_validate_url() {
        let policy = WebFetchEgressPolicy::default();
        assert!(validate_url(&policy, "https://example.com").is_ok());
        assert!(validate_url(&policy, "http://example.com").is_err());
        // localhost is only blocked when allow_private_networks is false
        let restricted_policy = WebFetchEgressPolicy {
            allow_schemes: default_web_fetch_schemes(),
            allow_private_networks: false,
            enabled: true,
        };
        assert!(validate_url(&restricted_policy, "https://localhost").is_err());
        assert!(validate_url(&restricted_policy, "https://192.168.1.1").is_err());
        assert!(validate_url(&restricted_policy, "https://example.com").is_ok());
    }
}
