pub mod handler;
pub mod models_endpoint;
pub mod router;
pub mod streaming;

use crate::{AppError, AppResult};
use std::sync::atomic::AtomicBool;
use std::sync::OnceLock;
use std::time::Duration;

pub static LAUNCHER_SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static TRAY_THREAD_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static TRAY_THREAD: std::sync::OnceLock<std::thread::Thread> = std::sync::OnceLock::new();

pub fn is_valid_proxy_bearer(header: Option<&str>, token: &str) -> bool {
    header
        .and_then(|value| value.trim().strip_prefix("Bearer "))
        .map(str::trim)
        == Some(token)
}

pub fn is_valid_proxy_authorization(header: Option<&str>) -> bool {
    is_valid_proxy_bearer(header, crate::constants::PROXY_AUTH_TOKEN)
}

pub fn is_authorized_proxy_request(
    authorization: Option<&str>,
    x_api_key: Option<&str>,
    token: &str,
) -> bool {
    let token = token.trim();
    if token.is_empty() {
        return false;
    }
    is_valid_proxy_bearer(authorization, token)
        || x_api_key.map(str::trim).is_some_and(|value| value == token)
}

pub fn app_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(crate) fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(crate::constants::HTTP_TIMEOUT_SECS))
            .timeout(Duration::from_secs(crate::constants::HTTP_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

pub(crate) fn apply_gateway_auth(
    request: reqwest::RequestBuilder,
    scheme: &str,
    key: &str,
    url: &str,
) -> AppResult<reqwest::RequestBuilder> {
    let scheme = match scheme {
        "auto" => {
            if url::Url::parse(url)
                .map_err(|error| AppError::InvalidConfig(error.to_string()))?
                .host_str()
                == Some("api.anthropic.com")
            {
                "x-api-key"
            } else {
                "bearer"
            }
        }
        "x-api-key" | "bearer" | "sso" => scheme,
        _ => return Err(AppError::InvalidConfig("不支援的 Auth Scheme".to_string())),
    };

    if key.is_empty() {
        Ok(request)
    } else if scheme == "x-api-key" {
        Ok(request.header("x-api-key", key))
    } else {
        Ok(request.bearer_auth(key))
    }
}

static SHUTDOWN_TX: std::sync::OnceLock<tokio::sync::watch::Sender<bool>> =
    std::sync::OnceLock::new();

pub fn trigger_shutdown() {
    if let Some(tx) = SHUTDOWN_TX.get() {
        let _ = tx.send(true);
    }
}

pub fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter, Registry};

    let local_dir = crate::common::local_app_data()
        .join("FreeClaudeLauncher")
        .join("logs");
    let _ = std::fs::create_dir_all(&local_dir);

    let file_appender = tracing_appender::rolling::daily(local_dir, "launcher.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(true);

    let stdout_layer = fmt::layer().with_target(false).with_ansi(true);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = Registry::default()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer);

    if tracing::subscriber::set_global_default(subscriber).is_err() {
        eprintln!("Failed to set global subscriber");
    }

    Some(guard)
}

async fn shutdown_signal() {
    let mut rx = if let Some(tx) = SHUTDOWN_TX.get() {
        tx.subscribe()
    } else {
        return;
    };
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            break;
        }
    }
    tracing::info!("Proxy server received shutdown signal. Exiting gracefully...");
}

pub fn start_server_background(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            if let Err(e) = run_server(port).await {
                tracing::error!("Proxy server failed: {:?}", e);
            }
        });
    });
    Ok(())
}

pub async fn run_server(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("127.0.0.1:{}", port);
    tracing::info!("==================================================");
    tracing::info!("FreeClaudeLauncher Rust Axum Async Server 已啟動");
    tracing::info!("本機服務: http://{}", addr);
    tracing::info!("API 代理: http://{}/v1/messages", addr);
    tracing::info!("==================================================");

    let (tx, _rx) = tokio::sync::watch::channel(false);
    let _ = SHUTDOWN_TX.set(tx);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router::create_router(port))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

#[cfg(test)]
mod gateway_auth_tests {
    use super::*;
    use crate::AppError;

    fn headers(scheme: &str, url: &str) -> reqwest::header::HeaderMap {
        apply_gateway_auth(reqwest::Client::new().get(url), scheme, "secret", url)
            .unwrap()
            .build()
            .unwrap()
            .headers()
            .clone()
    }

    #[test]
    fn auto_uses_x_api_key_for_anthropic() {
        let headers = headers("auto", "https://api.anthropic.com/v1/models");
        assert_eq!(headers["x-api-key"], "secret");
        assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
    }

    #[test]
    fn x_api_key_uses_named_header() {
        assert_eq!(
            headers("x-api-key", "https://example.com/v1/models")["x-api-key"],
            "secret"
        );
    }

    #[test]
    fn bearer_uses_authorization_header() {
        assert_eq!(
            headers("bearer", "https://example.com/v1/models")[reqwest::header::AUTHORIZATION],
            "Bearer secret"
        );
    }

    #[test]
    fn sso_uses_authorization_header() {
        assert_eq!(
            headers("sso", "https://example.com/v1/models")[reqwest::header::AUTHORIZATION],
            "Bearer secret"
        );
    }

    #[test]
    fn auto_uses_bearer_for_other_hosts() {
        assert_eq!(
            headers("auto", "https://example.com/v1/models")[reqwest::header::AUTHORIZATION],
            "Bearer secret"
        );
    }

    #[test]
    fn invalid_scheme_is_rejected() {
        let result = apply_gateway_auth(
            reqwest::Client::new().get("https://example.com"),
            "basic",
            "secret",
            "https://example.com",
        );
        assert!(matches!(result, Err(AppError::InvalidConfig(_))));
    }
}
