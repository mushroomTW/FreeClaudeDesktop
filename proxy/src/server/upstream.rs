use axum::http::HeaderMap;
use reqwest::Client;

const MAX_UPSTREAM_ERROR_BYTES: usize = 64 * 1024;

/// 建立送往 Gateway 的請求，並依傳輸類型篩選可轉送標頭。
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_upstream_request(
    client: &Client,
    target_url: &str,
    body: String,
    headers: &HeaderMap,
    api_key: &str,
    auth_scheme: &str,
    is_anthropic_native: bool,
) -> free_claude_core::AppResult<reqwest::RequestBuilder> {
    let mut request = client.post(target_url).body(body);
    let skip_header = if api_key.is_empty() {
        None
    } else {
        Some(free_claude_core::resolve_auth_header_name(
            auth_scheme,
            target_url,
        )?)
    };

    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if skip_header.is_some_and(|skip| lower == skip) {
            continue;
        }

        let forward = if is_anthropic_native {
            !matches!(lower.as_str(), "host" | "content-length" | "connection")
        } else {
            matches!(
                lower.as_str(),
                "content-type" | "accept" | "user-agent" | "accept-encoding" | "connection"
            ) || lower.starts_with("anthropic-")
        };
        if forward {
            request = request.header(name.clone(), value.clone());
        }
    }

    crate::server::apply_gateway_auth(request, auth_scheme, api_key, target_url)
}

/// 複製端對端安全的上游回應標頭。
pub(crate) fn copy_safe_response_headers(
    source: &reqwest::header::HeaderMap,
    target: &mut HeaderMap,
) {
    for (name, value) in source {
        if !matches!(
            name.as_str().to_ascii_lowercase().as_str(),
            "connection" | "content-length" | "transfer-encoding" | "content-encoding"
        ) {
            target.insert(name.clone(), value.clone());
        }
    }
}

/// 有上限地讀取上游錯誤本文，避免錯誤頁耗盡記憶體。
pub(crate) async fn read_bounded_error(response: reqwest::Response) -> String {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = MAX_UPSTREAM_ERROR_BYTES.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if bytes.len() == MAX_UPSTREAM_ERROR_BYTES {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
