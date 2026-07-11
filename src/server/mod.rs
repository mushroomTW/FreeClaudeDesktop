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

async fn shutdown_signal(mut rx: tokio::sync::watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            break;
        }
    }
    tracing::info!("Proxy server received shutdown signal. Exiting gracefully...");
}

type ServerError = Box<dyn std::error::Error + Send + Sync>;

pub struct ServerHandle {
    port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<Result<(), ServerError>>>,
}

impl ServerHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn shutdown_and_join(mut self) -> Result<(), ServerError> {
        let _ = self.shutdown.send(true);
        self.thread
            .take()
            .expect("server thread must exist")
            .join()
            .map_err(|_| std::io::Error::other("Proxy server thread panicked"))?
    }
}

pub fn start_server_background(port: u16) -> Result<ServerHandle, ServerError> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let runtime = tokio::runtime::Runtime::new()?;
    let (shutdown, rx) = tokio::sync::watch::channel(false);
    let thread = std::thread::spawn(move || {
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            serve(listener, port, rx).await
        })
    });
    Ok(ServerHandle {
        port,
        shutdown,
        thread: Some(thread),
    })
}

pub async fn run_server(port: u16) -> Result<(), ServerError> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let (_tx, rx) = tokio::sync::watch::channel(false);
    serve(listener, port, rx).await
}

async fn serve(
    listener: tokio::net::TcpListener,
    port: u16,
    rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), ServerError> {
    let addr = listener.local_addr()?;
    tracing::info!("==================================================");
    tracing::info!("FreeClaudeLauncher Rust Axum Async Server 已啟動");
    tracing::info!("本機服務: http://{}", addr);
    tracing::info!("API 代理: http://{}/v1/messages", addr);
    tracing::info!("==================================================");

    axum::serve(listener, router::create_router(port))
        .with_graceful_shutdown(shutdown_signal(rx))
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn occupied_port_is_reported_before_return() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(start_server_background(port).is_err());
    }

    #[test]
    fn shutdown_joins_server_and_releases_port() {
        let server = start_server_background(0).unwrap();
        let port = server.port();
        server.shutdown_and_join().unwrap();
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
}
