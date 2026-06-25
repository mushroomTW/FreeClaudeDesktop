#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use native_windows_gui as nwg;
use nwg::NativeUi;
use serde_json::Value;
use std::{cell::RefCell, ops::Deref, path::Path, rc::Rc};

const PROVIDERS: [&str; 4] = ["OpenRouter", "NVIDIA", "Anthropic", "自訂"];

#[derive(Default)]
struct LauncherApp {
    window: nwg::Window,
    icon: nwg::Icon,
    tray: nwg::TrayNotification,
    tray_menu: nwg::Menu,
    tray_open: nwg::MenuItem,
    tray_exit: nwg::MenuItem,

    title_font: nwg::Font,
    section_font: nwg::Font,
    label_font: nwg::Font,
    control_font: nwg::Font,
    status_font: nwg::Font,

    title: nwg::Label,
    subtitle: nwg::Label,
    status: nwg::Label,
    settings_section: nwg::Label,
    provider_label: nwg::Label,
    provider: nwg::ComboBox<String>,
    base_url_label: nwg::Label,
    base_url: nwg::TextInput,
    api_key_label: nwg::Label,
    api_key: nwg::TextInput,
    auth_label: nwg::Label,
    auth_scheme: nwg::ComboBox<String>,
    custom_path_check: nwg::CheckBox,
    custom_path: nwg::TextInput,
    save_launch: nwg::Button,
    save_only: nwg::Button,
    restore: nwg::Button,
}

impl LauncherApp {
    fn show_window(&self) {
        self.window.set_visible(true);
        self.window.set_focus();
    }

    fn hide_to_tray(&self) {
        self.window.set_visible(false);
        self.tray.show(
            "Proxy 仍在背景執行",
            Some("FreeClaudeLauncher"),
            Some(nwg::TrayNotificationFlags::INFO_ICON),
            None,
        );
    }

    fn show_tray_menu(&self) {
        let (x, y) = nwg::GlobalCursor::position();
        self.tray_menu.popup(x, y);
    }

    fn refresh_status(&self) {
        match free_claude_launcher::detect_claude_path() {
            Some(path) => self.status.set_text(&format!(
                "已偵測 Claude Desktop\n{}",
                compact_path(&path, 66)
            )),
            None => self
                .status
                .set_text("尚未找到 Claude.exe，可使用下方自訂路徑"),
        }
    }

    fn apply_provider_template(&self) {
        match self.provider.selection().unwrap_or(0) {
            0 => {
                self.base_url.set_text("https://openrouter.ai/api");
                self.auth_scheme.set_selection(Some(0));
            }
            1 => {
                self.base_url
                    .set_text("https://integrate.api.nvidia.com/v1");
                self.auth_scheme.set_selection(Some(0));
            }
            2 => {
                self.base_url.set_text("https://api.anthropic.com");
                self.auth_scheme.set_selection(Some(1));
            }
            _ => {}
        }
    }

    fn update_custom_path_state(&self) {
        let enabled = self.custom_path_check.check_state() == nwg::CheckBoxState::Checked;
        self.custom_path.set_enabled(enabled);
    }

    fn load_saved_config(&self) {
        if let Some(settings) = free_claude_launcher::get_launcher_settings() {
            self.base_url.set_text(&settings.real_base_url);
            self.auth_scheme
                .set_selection(Some(if settings.real_auth_scheme == "x-api-key" {
                    1
                } else {
                    0
                }));
            self.api_key
                .set_placeholder_text(Some("已儲存 API Key，留空沿用"));
            if settings.real_base_url.contains("openrouter.ai") {
                self.provider.set_selection(Some(0));
            } else if settings.real_base_url.contains("integrate.api.nvidia.com") {
                self.provider.set_selection(Some(1));
            } else if settings.real_base_url.contains("api.anthropic.com") {
                self.provider.set_selection(Some(2));
            } else {
                self.provider.set_selection(Some(3));
            }
        }
    }

    fn auth_value(&self) -> &'static str {
        if self.auth_scheme.selection() == Some(1) {
            "x-api-key"
        } else {
            "bearer"
        }
    }

    fn save_current_config(&self) -> Result<(), String> {
        let result = free_claude_launcher::save_config(
            &self.base_url.text(),
            &self.api_key.text(),
            self.auth_value(),
        );
        json_result(result)
    }

    fn save_only(&self) {
        match self.save_current_config() {
            Ok(()) => {
                self.api_key.set_text("");
                self.api_key
                    .set_placeholder_text(Some("已儲存 API Key，留空沿用"));
                nwg::modal_info_message(&self.window, "已儲存", "設定已寫入 Claude。");
            }
            Err(error) => {
                nwg::modal_error_message(&self.window, "儲存失敗", &error);
            }
        };
    }

    fn save_and_launch(&self) {
        if let Err(error) = self.save_current_config() {
            nwg::modal_error_message(&self.window, "儲存失敗", &error);
            return;
        }

        let custom = if self.custom_path_check.check_state() == nwg::CheckBoxState::Checked {
            Some(self.custom_path.text())
        } else {
            None
        };
        match free_claude_launcher::launch_claude(custom.as_deref()) {
            Ok(path) => {
                self.api_key.set_text("");
                self.api_key
                    .set_placeholder_text(Some("已儲存 API Key，留空沿用"));
                self.refresh_status();
                nwg::modal_info_message(
                    &self.window,
                    "已啟動",
                    &format!("Claude Desktop 已啟動。\n{path}"),
                );
            }
            Err(error) => {
                nwg::modal_error_message(&self.window, "啟動失敗", &error);
            }
        };
    }

    fn restore_official(&self) {
        let params = nwg::MessageParams {
            title: "還原官方設定",
            content: "這會移除目前的 Gateway 設定，改回 Claude 官方預設。是否繼續？",
            buttons: nwg::MessageButtons::YesNo,
            icons: nwg::MessageIcons::Warning,
        };
        if nwg::modal_message(&self.window, &params) != nwg::MessageChoice::Yes {
            return;
        }

        match json_result(free_claude_launcher::restore_official_config()) {
            Ok(()) => {
                self.api_key.set_text("");
                self.api_key.set_placeholder_text(Some("輸入 API Key"));
                nwg::modal_info_message(&self.window, "已還原", "Claude 設定已回到官方預設。");
            }
            Err(error) => {
                nwg::modal_error_message(&self.window, "還原失敗", &error);
            }
        }
    }
}

struct LauncherAppUi {
    inner: Rc<LauncherApp>,
    handler: RefCell<Option<nwg::EventHandler>>,
}

impl NativeUi<LauncherAppUi> for LauncherApp {
    fn build_ui(mut data: LauncherApp) -> Result<LauncherAppUi, nwg::NwgError> {
        use nwg::Event as E;

        nwg::Font::builder()
            .family("Segoe UI")
            .size(20)
            .weight(700)
            .build(&mut data.title_font)?;
        nwg::Font::builder()
            .family("Segoe UI")
            .size(14)
            .weight(700)
            .build(&mut data.section_font)?;
        nwg::Font::builder()
            .family("Segoe UI")
            .size(13)
            .weight(600)
            .build(&mut data.label_font)?;
        nwg::Font::builder()
            .family("Segoe UI")
            .size(13)
            .build(&mut data.control_font)?;
        nwg::Font::builder()
            .family("Segoe UI")
            .size(12)
            .build(&mut data.status_font)?;

        nwg::Icon::builder()
            .source_bin(Some(include_bytes!("../icon.ico")))
            .build(&mut data.icon)?;

        nwg::Window::builder()
            .flags(
                nwg::WindowFlags::WINDOW
                    | nwg::WindowFlags::MINIMIZE_BOX
                    | nwg::WindowFlags::VISIBLE,
            )
            .size((560, 455))
            .position((340, 180))
            .title("FreeClaudeLauncher")
            .icon(Some(&data.icon))
            .build(&mut data.window)?;

        nwg::Label::builder()
            .text("Free Claude Launcher")
            .position((24, 18))
            .size((360, 30))
            .font(Some(&data.title_font))
            .parent(&data.window)
            .build(&mut data.title)?;

        nwg::Label::builder()
            .text("本機 Proxy：127.0.0.1:3000")
            .position((26, 52))
            .size((260, 22))
            .font(Some(&data.status_font))
            .parent(&data.window)
            .build(&mut data.subtitle)?;

        nwg::Label::builder()
            .text("正在檢查 Claude Desktop...")
            .position((24, 82))
            .size((510, 46))
            .font(Some(&data.status_font))
            .background_color(Some([245, 247, 250]))
            .parent(&data.window)
            .build(&mut data.status)?;

        nwg::Label::builder()
            .text("連線設定")
            .position((24, 145))
            .size((120, 24))
            .font(Some(&data.section_font))
            .parent(&data.window)
            .build(&mut data.settings_section)?;

        build_label(
            &data.window,
            &mut data.provider_label,
            "API 供應商",
            34,
            184,
            &data.label_font,
        )?;
        nwg::ComboBox::builder()
            .collection(PROVIDERS.iter().map(|value| value.to_string()).collect())
            .selected_index(Some(0))
            .position((145, 178))
            .size((385, 120))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.provider)?;

        build_label(
            &data.window,
            &mut data.base_url_label,
            "Gateway URL",
            34,
            224,
            &data.label_font,
        )?;
        nwg::TextInput::builder()
            .text("https://openrouter.ai/api")
            .position((145, 218))
            .size((385, 27))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.base_url)?;

        build_label(
            &data.window,
            &mut data.api_key_label,
            "API Key",
            34,
            264,
            &data.label_font,
        )?;
        nwg::TextInput::builder()
            .placeholder_text(Some("輸入 API Key"))
            .password(Some('*'))
            .position((145, 258))
            .size((385, 27))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.api_key)?;

        build_label(
            &data.window,
            &mut data.auth_label,
            "驗證方式",
            34,
            304,
            &data.label_font,
        )?;
        nwg::ComboBox::builder()
            .collection(vec!["bearer".to_string(), "x-api-key".to_string()])
            .selected_index(Some(0))
            .position((145, 298))
            .size((385, 85))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.auth_scheme)?;

        nwg::CheckBox::builder()
            .text("使用自訂 Claude.exe 路徑")
            .position((145, 335))
            .size((260, 24))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.custom_path_check)?;

        nwg::TextInput::builder()
            .placeholder_text(Some("C:\\Users\\...\\Claude.exe"))
            .position((145, 363))
            .size((385, 27))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.custom_path)?;
        data.custom_path.set_enabled(false);

        nwg::Button::builder()
            .text("儲存並啟動 Claude")
            .position((24, 405))
            .size((250, 34))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.save_launch)?;

        nwg::Button::builder()
            .text("僅儲存")
            .position((292, 405))
            .size((114, 34))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.save_only)?;

        nwg::Button::builder()
            .text("還原官方")
            .position((420, 405))
            .size((110, 34))
            .font(Some(&data.control_font))
            .parent(&data.window)
            .build(&mut data.restore)?;

        nwg::TrayNotification::builder()
            .parent(&data.window)
            .icon(Some(&data.icon))
            .tip(Some("FreeClaudeLauncher"))
            .build(&mut data.tray)?;

        nwg::Menu::builder()
            .popup(true)
            .parent(&data.window)
            .build(&mut data.tray_menu)?;

        nwg::MenuItem::builder()
            .text("開啟")
            .parent(&data.tray_menu)
            .build(&mut data.tray_open)?;

        nwg::MenuItem::builder()
            .text("結束")
            .parent(&data.tray_menu)
            .build(&mut data.tray_exit)?;

        let ui = LauncherAppUi {
            inner: Rc::new(data),
            handler: RefCell::new(None),
        };

        ui.inner.load_saved_config();
        ui.inner.refresh_status();

        let evt_ui = Rc::downgrade(&ui.inner);
        let handle_events = move |evt, _evt_data, handle| {
            let Some(app) = evt_ui.upgrade() else {
                return;
            };
            match evt {
                E::OnWindowClose if handle == app.window => app.hide_to_tray(),
                E::OnButtonClick if handle == app.save_launch => app.save_and_launch(),
                E::OnButtonClick if handle == app.save_only => app.save_only(),
                E::OnButtonClick if handle == app.restore => app.restore_official(),
                E::OnButtonClick if handle == app.custom_path_check => {
                    app.update_custom_path_state()
                }
                E::OnComboxBoxSelection if handle == app.provider => app.apply_provider_template(),
                E::OnContextMenu if handle == app.tray => app.show_tray_menu(),
                E::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) if handle == app.tray => {
                    app.show_window()
                }
                E::OnMenuItemSelected if handle == app.tray_open => app.show_window(),
                E::OnMenuItemSelected if handle == app.tray_exit => nwg::stop_thread_dispatch(),
                _ => {}
            }
        };
        *ui.handler.borrow_mut() = Some(nwg::full_bind_event_handler(
            &ui.inner.window.handle,
            handle_events,
        ));

        Ok(ui)
    }
}

impl Drop for LauncherAppUi {
    fn drop(&mut self) {
        if let Some(handler) = self.handler.borrow_mut().take() {
            nwg::unbind_event_handler(&handler);
        }
    }
}

impl Deref for LauncherAppUi {
    type Target = LauncherApp;

    fn deref(&self) -> &LauncherApp {
        &self.inner
    }
}

fn build_label(
    parent: &nwg::Window,
    out: &mut nwg::Label,
    text: &str,
    x: i32,
    y: i32,
    font: &nwg::Font,
) -> Result<(), nwg::NwgError> {
    nwg::Label::builder()
        .text(text)
        .position((x, y))
        .size((105, 24))
        .font(Some(font))
        .parent(parent)
        .build(out)
}

fn compact_path(path: &Path, max_chars: usize) -> String {
    let text = path.display().to_string();
    if text.chars().count() <= max_chars {
        return text;
    }

    let tail: String = text
        .chars()
        .rev()
        .take(max_chars.saturating_sub(4))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...\\{tail}")
}

fn json_result(value: Value) -> Result<(), String> {
    if value.get("success").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("未知錯誤")
            .to_string())
    }
}

fn run_app() -> Result<(), String> {
    nwg::init().map_err(|error| error.to_string())?;
    nwg::Font::set_global_family("Segoe UI").map_err(|error| error.to_string())?;
    free_claude_launcher::start_server_background().map_err(|error| error.to_string())?;

    let _ui = LauncherApp::build_ui(Default::default()).map_err(|error| error.to_string())?;
    nwg::dispatch_thread_events();
    Ok(())
}

fn main() {
    if let Err(error) = run_app() {
        nwg::simple_message("FreeClaudeLauncher", &error);
    }
}
