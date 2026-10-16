use std::error::Error;

use free_claude_core::constants::DEFAULT_PORT;

/// 在單一常駐程序中啟動 Proxy 與 Companion。
pub async fn run() -> Result<(), Box<dyn Error>> {
    let port = std::env::var("FREECLAUDE_PROXY_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let _logging_guard = freeclaude_proxy::server::init_logging();
    let server = freeclaude_proxy::server::start_server_background(port)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let companion_result = crate::companion_daemon::companion_daemon().await;
    server
        .shutdown_and_join()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    companion_result
}
