pub mod admin_assets;
pub mod admin_settings;
pub mod api_log;
pub mod companion;
pub mod handler;
pub mod messages_probe;
pub mod model_retry;
pub mod models_endpoint;
pub mod optimization_response;
pub mod router;
pub mod streaming;
pub mod upstream;

/// 執行 `app_url` 對應的處理流程。
pub fn app_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub(crate) use free_claude_core::{apply_gateway_auth, http_client};

/// 執行 `init_logging` 對應的處理流程。
pub fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::{EnvFilter, Registry, fmt, prelude::*};

    let local_dir = free_claude_core::common::local_app_data()
        .join("FreeClaudeDesktop")
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

/// 停止或停用 `shutdown_signal` 流程。
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
    /// 執行 `port` 對應的處理流程。
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 停止或停用 `shutdown_and_join` 流程。
    pub fn shutdown_and_join(mut self) -> Result<(), ServerError> {
        let _ = self.shutdown.send(true);
        self.thread
            .take()
            .expect("server thread must exist")
            .join()
            .map_err(|_| std::io::Error::other("Proxy server thread panicked"))?
    }
}

/// 啟動或執行 `start_server_background` 流程。
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

/// 啟動或執行 `run_server` 流程。
pub async fn run_server(port: u16) -> Result<(), ServerError> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let (_tx, rx) = tokio::sync::watch::channel(false);
    serve(listener, port, rx).await
}

/// 執行 `serve` 對應的處理流程。
async fn serve(
    listener: tokio::net::TcpListener,
    port: u16,
    rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), ServerError> {
    let addr = listener.local_addr()?;
    tracing::info!("==================================================");
    tracing::info!("FreeClaudeDesktop Rust Axum Async Server 已啟動");
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
    use free_claude_core::AppError;

    /// 執行 `headers` 對應的處理流程。
    fn headers(scheme: &str, url: &str) -> reqwest::header::HeaderMap {
        apply_gateway_auth(reqwest::Client::new().get(url), scheme, "secret", url)
            .unwrap()
            .build()
            .unwrap()
            .headers()
            .clone()
    }

    #[test]
    /// 驗證 `auto_uses_x_api_key_for_anthropic` 的行為符合預期。
    fn auto_uses_x_api_key_for_anthropic() {
        let headers = headers("auto", "https://api.anthropic.com/v1/models");
        assert_eq!(headers["x-api-key"], "secret");
        assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
    }

    #[test]
    /// 驗證 `x_api_key_uses_named_header` 的行為符合預期。
    fn x_api_key_uses_named_header() {
        assert_eq!(
            headers("x-api-key", "https://example.com/v1/models")["x-api-key"],
            "secret"
        );
    }

    #[test]
    /// 驗證 `bearer_uses_authorization_header` 的行為符合預期。
    fn bearer_uses_authorization_header() {
        assert_eq!(
            headers("bearer", "https://example.com/v1/models")[reqwest::header::AUTHORIZATION],
            "Bearer secret"
        );
    }

    #[test]
    /// 驗證 `sso_uses_authorization_header` 的行為符合預期。
    fn sso_uses_authorization_header() {
        assert_eq!(
            headers("sso", "https://example.com/v1/models")[reqwest::header::AUTHORIZATION],
            "Bearer secret"
        );
    }

    #[test]
    /// 驗證 `auto_uses_bearer_for_other_hosts` 的行為符合預期。
    fn auto_uses_bearer_for_other_hosts() {
        assert_eq!(
            headers("auto", "https://example.com/v1/models")[reqwest::header::AUTHORIZATION],
            "Bearer secret"
        );
    }

    #[test]
    /// 驗證 `invalid_scheme_is_rejected` 的行為符合預期。
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
    /// 驗證 `occupied_port_is_reported_before_return` 的行為符合預期。
    fn occupied_port_is_reported_before_return() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(start_server_background(port).is_err());
    }

    #[test]
    /// 驗證 `shutdown_joins_server_and_releases_port` 的行為符合預期。
    fn shutdown_joins_server_and_releases_port() {
        let server = start_server_background(0).unwrap();
        let port = server.port();
        server.shutdown_and_join().unwrap();
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
}
