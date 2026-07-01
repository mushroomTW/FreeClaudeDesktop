pub mod handler;
pub mod models_endpoint;
pub mod router;
pub mod streaming;

use reqwest::blocking::Client;
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::OnceLock;

pub static LAUNCHER_SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static TRAY_THREAD_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static TRAY_THREAD: std::sync::OnceLock<std::thread::Thread> = std::sync::OnceLock::new();

pub fn is_valid_proxy_authorization(header: Option<&str>) -> bool {
    header
        .and_then(|value| value.trim().strip_prefix("Bearer "))
        .map(str::trim)
        == Some(crate::constants::PROXY_AUTH_TOKEN)
}

pub fn app_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
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
    
    let local_dir = crate::common::local_app_data().join("FreeClaudeLauncher").join("logs");
    let _ = std::fs::create_dir_all(&local_dir);
    
    let file_appender = tracing_appender::rolling::daily(local_dir, "launcher.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(true);
        
    let stdout_layer = fmt::layer()
        .with_target(false)
        .with_ansi(true);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

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

// Blocking version kept for lib.rs::save_config sync execution
fn blocking_http_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(std::time::Duration::from_secs(
                crate::constants::HTTP_TIMEOUT_SECS,
            ))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

pub fn fetch_models_list(
    base_url: &str,
    api_key: &str,
    auth_scheme: &str,
) -> Result<Value, String> {
    let url = crate::conversion::response_converter::normalize_models_url(base_url)?;
    let mut req = blocking_http_client().get(url);
    if auth_scheme == "x-api-key" {
        req = req.header("x-api-key", api_key);
    } else {
        req = req.bearer_auth(api_key);
    }
    let res = req.send().map_err(|e| format!("Request failed: {e}"))?;
    let status = res.status();
    let text = res.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("API responded with status {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("Failed to parse models response: {e}"))
}
