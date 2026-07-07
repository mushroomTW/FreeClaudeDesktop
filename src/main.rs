#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use free_claude_launcher::app::LauncherApp;
use iced::window;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// 從嵌入的 ico 載入視窗圖示
fn load_icon() -> Option<window::Icon> {
    let ico_data = include_bytes!("../icon.ico");
    let img = image::load_from_memory(ico_data).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

fn main() -> iced::Result {
    if std::env::args().any(|arg| {
        let trimmed = arg.trim_start_matches('-');
        trimmed == "mcp" || trimmed == "mcp-computer-server"
    }) {
        if let Err(error) = free_claude_launcher::mcp::run_computer_server() {
            eprintln!("Computer MCP server failed: {error}");
        }
        return Ok(());
    }

    // 1. 初始化日誌系統
    let _guard = free_claude_launcher::server::init_logging();
    tracing::info!("FreeClaudeLauncher 啟動...");

    // 2. 獲取預計埠號
    let mut test_port = free_claude_launcher::constants::DEFAULT_PORT;
    if let Some(settings) = free_claude_launcher::get_launcher_settings() {
        if let Some(port) = settings.active_port {
            test_port = port;
        }
    }

    // 3. 嘗試向舊實例發送喚醒請求
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(800))
        .build();
    let mut waked_up = false;
    if let Ok(client) = &client {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{}/__launcher_show", test_port))
            .send()
        {
            if resp.status().is_success() {
                if let Ok(text) = resp.text() {
                    if text == "ok" {
                        waked_up = true;
                    }
                }
            }
        }
    }

    if waked_up {
        tracing::info!("舊實例已被喚醒，新實例直接退出。");
        return Ok(());
    }

    // 4. 探測並選定最終的可用埠
    let final_port = match std::net::TcpListener::bind(format!("127.0.0.1:{}", test_port)) {
        Ok(listener) => {
            tracing::info!("綁定預計埠成功：{}", test_port);
            drop(listener);
            test_port
        }
        Err(_) => {
            tracing::warn!("預計埠 {} 已被佔用，嘗試分配隨機埠...", test_port);
            match std::net::TcpListener::bind("127.0.0.1:0") {
                Ok(listener) => {
                    let port = listener.local_addr().unwrap().port();
                    tracing::info!("成功分配隨機埠：{}", port);
                    drop(listener);
                    port
                }
                Err(e) => {
                    tracing::error!("分配隨機埠失敗，使用預設：{:?}", e);
                    free_claude_launcher::constants::DEFAULT_PORT
                }
            }
        }
    };

    // 5. 啟動背景代理伺服器
    if let Err(e) = free_claude_launcher::server::start_server_background(final_port) {
        tracing::error!("無法啟動背景代理伺服器: {:?}", e);
        #[cfg(target_os = "windows")]
        unsafe {
            use std::os::windows::ffi::OsStrExt;
            let title: Vec<u16> = std::ffi::OsStr::new("啟動失敗")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let msg_str = format!("無法啟動背景代理伺服器，可能已被其他程式佔用。\n詳細錯誤: {e}");
            let message: Vec<u16> = std::ffi::OsStr::new(&msg_str)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            winapi::um::winuser::MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                winapi::um::winuser::MB_OK | winapi::um::winuser::MB_ICONERROR,
            );
        }
        #[cfg(not(target_os = "windows"))]
        {
            eprintln!("無法啟動背景代理伺服器，可能已被其他程式佔用。\n詳細錯誤: {e}");
        }
        return Ok(());
    }

    // 6. 更新設定檔中的 active_port 與寫入 Claude 配置
    if let Some(mut settings) = free_claude_launcher::get_launcher_settings() {
        settings.active_port = Some(final_port);
        let _ = free_claude_launcher::save_launcher_settings(&settings);
        let _ = free_claude_launcher::update_config_port(final_port);
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<free_claude_launcher::app::Message>();

    // 在背景 thread 啟動系統匣圖示
    let tray_tx = tx.clone();
    std::thread::spawn(move || {
        free_claude_launcher::tray::run_tray_icon(tray_tx);
    });

    let tray_rx = Arc::new(Mutex::new(rx));

    let run_res = iced::application(
        move || LauncherApp::new(final_port, tray_rx.clone()),
        LauncherApp::update,
        free_claude_launcher::ui::view::view,
    )
    .subscription(LauncherApp::subscription)
    .title("FreeClaudeLauncher")
    .theme(LauncherApp::theme)
    .window(window::Settings {
        size: iced::Size::new(820.0, 700.0),
        resizable: false,
        icon: load_icon(),
        exit_on_close_request: false,
        ..Default::default()
    })
    .run();

    // 優雅關閉背景 proxy server
    free_claude_launcher::server::trigger_shutdown();
    std::thread::sleep(Duration::from_millis(150));

    run_res
}
