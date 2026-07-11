use crate::app::Message;
use std::sync::atomic::Ordering;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};

fn menu_message(id: &str) -> Option<Message> {
    match id {
        "quit" => Some(Message::TrayQuit),
        "show" => Some(Message::TrayShow),
        "hide" => Some(Message::TrayHide),
        _ => None,
    }
}

/// 建立系統匣圖示並執行訊息迴圈（在獨立 thread 執行）
pub fn run_tray_icon(tx: tokio::sync::mpsc::UnboundedSender<Message>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_tray_icon_inner(tx)));
    if let Err(e) = result {
        eprintln!("[tray] PANIC: {e:?}");
    }
}

fn run_tray_icon_inner(tx: tokio::sync::mpsc::UnboundedSender<Message>) {
    eprintln!("[tray] run_tray_icon 啟動");
    // 從 icon.ico 載入圖示（縮小為 32x32 適合系統匣）
    let icon_data = include_bytes!("../../icon.ico");
    let img = match image::load_from_memory(icon_data) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            eprintln!("[tray] 載入圖示失敗: {e}");
            return;
        }
    };
    let (_w, _h) = img.dimensions();
    eprintln!("[tray] 圖示載入成功 ({_w}x{_h})");

    // 縮小到 32x32（系統匣標準尺寸）
    let resized = image::imageops::resize(&img, 32, 32, image::imageops::FilterType::Lanczos3);
    let rgba = resized.into_raw();
    let icon = match Icon::from_rgba(rgba, 32, 32) {
        Ok(icon) => icon,
        Err(e) => {
            eprintln!("[tray] 建立 Icon 失敗: {e}");
            return;
        }
    };
    eprintln!("[tray] Icon 建立成功");

    // 建立選單
    let menu = Menu::new();
    let show_item = MenuItem::with_id("show", "顯示視窗", true, None);
    let hide_item = MenuItem::with_id("hide", "隱藏視窗", true, None);
    let quit_item = MenuItem::with_id("quit", "結束程式", true, None);
    if let Err(e) = menu.append(&show_item) {
        eprintln!("[tray] 加入 show 選單失敗: {e}");
        return;
    }
    if let Err(e) = menu.append(&hide_item) {
        eprintln!("[tray] 加入 hide 選單失敗: {e}");
        return;
    }
    if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
        eprintln!("[tray] 加入 separator 失敗: {e}");
        return;
    }
    if let Err(e) = menu.append(&quit_item) {
        eprintln!("[tray] 加入 quit 選單失敗: {e}");
        return;
    }
    eprintln!("[tray] 選單建立成功");

    // 建立系統匣圖示
    let _tray = match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("FreeClaudeLauncher")
        .with_icon(icon)
        .build()
    {
        Ok(tray) => {
            eprintln!("[tray] 系統匣圖示建立成功");
            tray
        }
        Err(e) => {
            eprintln!("[tray] 建立系統匣圖示失敗: {e}");
            return;
        }
    };

    // 註冊執行緒 ID / 控柄，用於事件驅動喚醒
    #[cfg(target_os = "windows")]
    unsafe {
        let tid = winapi::um::processthreadsapi::GetCurrentThreadId();
        crate::server::TRAY_THREAD_ID.store(tid, Ordering::Release);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = crate::server::TRAY_THREAD.set(std::thread::current());

    // 事件驅動之訊息迴圈 (不使用 100ms 輪詢，0% CPU)
    loop {
        #[cfg(target_os = "windows")]
        unsafe {
            use winapi::um::winuser::{DispatchMessageW, GetMessageW, TranslateMessage, MSG};
            let mut msg = std::mem::zeroed::<MSG>();
            // GetMessageW 會阻塞直到收到視窗或執行緒訊息 (例如滑鼠點擊、選單彈出、PostThreadMessage)
            let ret = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if ret <= 0 {
                return;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let receiver = MenuEvent::receiver();
            match receiver.recv_timeout(std::time::Duration::from_millis(250)) {
                Ok(event) => {
                    if let Some(message) = menu_message(event.id.0.as_str()) {
                        let quitting = matches!(message, Message::TrayQuit);
                        if tx.send(message).is_err() || quitting {
                            return;
                        }
                    }
                }
                Err(error) if error.is_disconnected() => return,
                Err(_) => {}
            }
        }

        // 處理選單事件
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(message) = menu_message(event.id.0.as_str()) {
                let quitting = matches!(message, Message::TrayQuit);
                if tx.send(message).is_err() || quitting {
                    return;
                }
            }
        }

        // 處理系統匣點擊事件（左鍵切換顯示）
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                let _ = tx.send(Message::TrayShow);
            }
        }

        // 處理單實例喚醒請求
        if crate::server::LAUNCHER_SHOW_REQUESTED.swap(false, Ordering::AcqRel) {
            let _ = tx.send(Message::TrayShow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_menu_ids_only() {
        assert!(matches!(menu_message("quit"), Some(Message::TrayQuit)));
        assert!(matches!(menu_message("show"), Some(Message::TrayShow)));
        assert!(matches!(menu_message("hide"), Some(Message::TrayHide)));
        assert!(menu_message("unknown").is_none());
    }
}
