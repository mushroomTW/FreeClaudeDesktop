#![allow(linker_messages)]

use freeclaude_proxy::{constants::DEFAULT_PORT, server};

#[tokio::main]
/// 啟動程式並執行主要流程。
async fn main() {
    let _logging_guard = server::init_logging();
    let port = std::env::var("FREECLAUDE_PROXY_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    if let Err(error) = server::run_server(port).await {
        eprintln!("無法啟動 freeclaude-proxy：{error}");
        std::process::exit(1);
    }
}
