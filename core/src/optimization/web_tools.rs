//! Web server tool handling (web_search / web_fetch).
//!
//! Provides local egress policy and validation for web_fetch tool calls
//! to prevent SSRF and enforce allowlists.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

const MAX_WEB_FETCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

#[derive(Debug)]
struct ResolvedTarget {
    url: url::Url,
    host: String,
    addr: SocketAddr,
}

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

pub fn policy_from_settings(settings: &crate::config::Settings) -> WebFetchEgressPolicy {
    WebFetchEgressPolicy {
        allow_schemes: settings
            .web_fetch_allowed_schemes
            .split(',')
            .map(str::trim)
            .filter(|scheme| !scheme.is_empty())
            .map(str::to_ascii_lowercase)
            .collect(),
        allow_private_networks: settings.web_fetch_allow_private_networks,
        enabled: settings.enable_web_server_tools,
    }
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

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL must include a host".to_string())?;
    if !policy.allow_private_networks {
        let normalized = host.to_ascii_lowercase();
        if is_private_address(&normalized)
            || normalized
                .parse::<IpAddr>()
                .is_ok_and(|ip| !is_public_ip(ip))
        {
            return Err(format!("Private network access is not allowed: {host}"));
        }
    }

    Ok(())
}

/// Check if a hostname is a private/local address.
fn is_private_address(host: &str) -> bool {
    // localhost variants
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.starts_with("localhost.")
        || host.ends_with(".local")
    {
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

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_ipv4(mapped);
            }
            let first = ip.segments()[0];
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8))
        }
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && a != 0
        && !(a == 100 && (64..=127).contains(&b))
        && !(a == 192 && b == 0 && c == 2)
        && !(a == 198 && (b == 18 || b == 19))
        && !(a == 198 && b == 51 && c == 100)
        && !(a == 203 && b == 0 && c == 113)
        && a < 240
}

async fn resolve_target(
    policy: &WebFetchEgressPolicy,
    url: url::Url,
) -> Result<ResolvedTarget, String> {
    validate_url(policy, url.as_str())?;
    let host = url
        .host_str()
        .ok_or_else(|| "URL must include a host".to_string())?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "URL has no usable port".to_string())?;
    let addrs: Vec<_> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|error| format!("DNS resolution failed for {host}: {error}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for {host}"));
    }
    if !policy.allow_private_networks && addrs.iter().any(|addr| !is_public_ip(addr.ip())) {
        return Err(format!("Private network access is not allowed: {host}"));
    }
    Ok(ResolvedTarget {
        url,
        host,
        addr: addrs[0],
    })
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

pub fn extract_latest_web_tool_call(body_str: &str) -> Option<(String, String, Value)> {
    let body: Value = serde_json::from_str(body_str).ok()?;
    let messages = body.get("messages").and_then(Value::as_array)?;
    for message in messages.iter().rev() {
        let blocks = message.get("content").and_then(Value::as_array)?;
        for block in blocks.iter().rev() {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let name = block.get("name").and_then(Value::as_str)?;
            if !is_web_server_tool(name) {
                continue;
            }
            let id = block
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("web_tool")
                .to_string();
            let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
            return Some((id, name.to_string(), input));
        }
    }
    None
}

pub async fn execute_web_tool(
    policy: &WebFetchEgressPolicy,
    tool_name: &str,
    input: &Value,
) -> Option<String> {
    match tool_name {
        "web_fetch" => Some(execute_web_fetch(policy, input).await),
        "web_search" => Some(execute_web_search(policy, input).await),
        _ => None,
    }
}

async fn execute_web_fetch(policy: &WebFetchEgressPolicy, input: &Value) -> String {
    let Some(url) = extract_web_fetch_url(input) else {
        return "web_fetch 缺少 url 參數。".to_string();
    };
    if let Err(error) = validate_url(policy, &url) {
        return error;
    }
    fetch_url(policy, &url).await
}

async fn execute_web_search(policy: &WebFetchEgressPolicy, input: &Value) -> String {
    let Some(query) = input
        .get("query")
        .or_else(|| input.get("q"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
    else {
        return "web_search 缺少 query 參數。".to_string();
    };
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let url = format!("https://duckduckgo.com/html/?q={encoded}");
    fetch_url(policy, &url).await
}

async fn fetch_url(policy: &WebFetchEgressPolicy, url: &str) -> String {
    let mut current = match url::Url::parse(url) {
        Ok(url) => url,
        Err(error) => return format!("Invalid URL: {error}"),
    };
    for redirects in 0..=MAX_REDIRECTS {
        let target = match resolve_target(policy, current).await {
            Ok(target) => target,
            Err(error) => return error,
        };
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(crate::constants::HTTP_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&target.host, target.addr)
            .build()
        {
            Ok(client) => client,
            Err(error) => return format!("web client build failed: {error}"),
        };
        let response = match client.get(target.url.clone()).send().await {
            Ok(response) => response,
            Err(error) => return format!("web request failed: {error}"),
        };
        if response.status().is_redirection() {
            if redirects == MAX_REDIRECTS {
                return format!("web request exceeded {MAX_REDIRECTS} redirects");
            }
            let Some(location) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
            else {
                return "web redirect missing Location header".to_string();
            };
            current = match target.url.join(location) {
                Ok(url) => url,
                Err(error) => return format!("invalid redirect URL: {error}"),
            };
            continue;
        }

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        let bytes = match collect_limited(response).await {
            Ok(bytes) => bytes,
            Err(error) => return error,
        };
        let text = String::from_utf8_lossy(&bytes);
        let body = truncate_chars(&strip_html_tags(&text), 20_000);
        return format!(
            "URL: {}\nStatus: {status}\nContent-Type: {content_type}\n\n{body}",
            target.url
        );
    }
    unreachable!()
}

async fn collect_limited(response: reqwest::Response) -> Result<Vec<u8>, String> {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("web response read failed: {error}"))?;
        if body.len().saturating_add(chunk.len()) > MAX_WEB_FETCH_BYTES {
            return Err("web response exceeds 2 MiB limit".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn strip_html_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut iter = text.chars();
    let truncated: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{truncated}\n\n[truncated]")
    } else {
        truncated
    }
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
        assert!(validate_url(&restricted_policy, "https://localhost.localdomain").is_err());
        assert!(validate_url(&restricted_policy, "https://example.com").is_ok());
    }

    #[tokio::test]
    async fn test_web_search_requires_query() {
        let policy = WebFetchEgressPolicy::default();
        let text = execute_web_tool(&policy, "web_search", &serde_json::json!({}))
            .await
            .unwrap();

        assert!(text.contains("缺少 query"));
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<h1>Hello</h1><p>World</p>"), "HelloWorld");
    }

    #[test]
    fn blocks_ipv4_mapped_private_ipv6() {
        let ip: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(!is_public_ip(ip));
    }

    #[test]
    fn blocks_documentation_and_benchmark_ranges() {
        for raw in ["192.0.2.1", "198.51.100.1", "203.0.113.1", "198.18.0.1"] {
            assert!(!is_public_ip(raw.parse().unwrap()), "{raw}");
        }
    }

    #[tokio::test]
    async fn stops_reading_after_two_mibibytes() {
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(|| async { vec![b'a'; MAX_WEB_FETCH_BYTES + 1] }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let policy = WebFetchEgressPolicy {
            allow_schemes: ["http".to_string()].into_iter().collect(),
            allow_private_networks: true,
            enabled: true,
        };
        let result = fetch_url(&policy, &format!("http://{addr}/")).await;
        assert!(result.contains("2 MiB"));
    }

    #[tokio::test]
    async fn accepts_exactly_two_mibibytes() {
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(|| async { vec![b'a'; MAX_WEB_FETCH_BYTES] }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let policy = WebFetchEgressPolicy {
            allow_schemes: ["http".to_string()].into_iter().collect(),
            allow_private_networks: true,
            enabled: true,
        };
        let result = fetch_url(&policy, &format!("http://{addr}/")).await;
        assert!(!result.contains("2 MiB limit"));
        assert!(result.contains("Status: 200 OK"));
    }

    #[tokio::test]
    async fn rejects_private_redirect_target_before_connecting() {
        let policy = WebFetchEgressPolicy {
            allow_schemes: ["http".to_string()].into_iter().collect(),
            allow_private_networks: false,
            enabled: true,
        };
        let target = url::Url::parse("http://127.0.0.1/private").unwrap();
        let error = resolve_target(&policy, target).await.unwrap_err();
        assert!(error.contains("Private network"));
    }
}
