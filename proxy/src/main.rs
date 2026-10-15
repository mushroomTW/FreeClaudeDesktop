#![allow(linker_messages)]

use free_claude_core::constants::DEFAULT_PORT;
use freeclaude_proxy::server;

/// 啟動程式並執行主要流程。
fn main() {
    free_claude_core::runtime::build_runtime()
        .expect("無法建立 Proxy 非同步執行環境")
        .block_on(run());
}

async fn run() {
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
