#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use iced::widget::{
    button, checkbox, column, container, pick_list, row, rule, text, text_input,
};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Shadow, Task, Theme,
};
use iced::font::Weight;
use iced::theme::Palette;
use iced::window::{self, Id, Mode};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent, MouseButton};

// ════════════════════════════════════════════════════════════════
//  色彩系統 — 深色紫藍主題
// ════════════════════════════════════════════════════════════════

const CLR_BG: Color = Color::from_rgb(0.098, 0.102, 0.176);
const CLR_CARD: Color = Color::from_rgb(0.141, 0.149, 0.227);
const CLR_PRIMARY: Color = Color::from_rgb(0.478, 0.408, 1.0);
const CLR_PRIMARY_HOVER: Color = Color::from_rgb(0.57, 0.51, 1.0);
const CLR_PRIMARY_PRESS: Color = Color::from_rgb(0.40, 0.34, 0.87);
const CLR_TEXT: Color = Color::from_rgb(0.906, 0.914, 0.961);
const CLR_TEXT_DIM: Color = Color::from_rgb(0.533, 0.549, 0.647);
const CLR_SUCCESS: Color = Color::from_rgb(0.298, 0.831, 0.494);
const CLR_DANGER: Color = Color::from_rgb(1.0, 0.380, 0.380);
const CLR_DANGER_HOVER: Color = Color::from_rgb(1.0, 0.46, 0.46);
const CLR_WARNING: Color = Color::from_rgb(1.0, 0.694, 0.298);
const CLR_BORDER: Color = Color::from_rgb(0.208, 0.220, 0.329);
const CLR_BTN_SEC: Color = Color::from_rgb(0.173, 0.184, 0.278);
const CLR_BTN_SEC_HOVER: Color = Color::from_rgb(0.216, 0.227, 0.329);

const PROVIDERS: [&str; 4] = ["OpenRouter", "NVIDIA", "Anthropic", "自訂"];
const AUTH_SCHEMES: [&str; 2] = ["bearer", "x-api-key"];

// ════════════════════════════════════════════════════════════════
//  應用程式狀態
// ════════════════════════════════════════════════════════════════

struct LauncherApp {
    provider: Option<String>,
    base_url: String,
    api_key: String,
    api_key_placeholder: String,
    auth_scheme: Option<String>,
    use_custom_path: bool,
    custom_path: String,
    status_text: String,
    status_ok: bool,
    toast: Option<Toast>,
    confirming_restore: bool,
    // 系統匣狀態
    is_hidden: bool,
    window_id: Option<Id>,
    quit_requested: Arc<AtomicBool>,
    show_requested: Arc<AtomicBool>,
    hide_requested: Arc<AtomicBool>,
}

struct Toast {
    message: String,
    is_success: bool,
}

// ════════════════════════════════════════════════════════════════
//  訊息定義
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
enum Message {
    ProviderSelected(String),
    BaseUrlChanged(String),
    ApiKeyChanged(String),
    AuthSchemeSelected(String),
    CustomPathToggled(bool),
    CustomPathChanged(String),
    SaveAndLaunch,
    SaveOnly,
    RestoreRequested,
    ConfirmRestore,
    CancelRestore,
    DismissToast,
    // 系統匣訊息
    None,
    WindowOpened(Id),
    CloseRequested(Id),
    TrayQuit,
    TrayShow,
    TrayHide,
}

// ════════════════════════════════════════════════════════════════
//  應用程式邏輯
// ════════════════════════════════════════════════════════════════

impl LauncherApp {
    /// 初始化：偵測 Claude、載入已儲存的設定
    fn new(
        quit_requested: Arc<AtomicBool>,
        show_requested: Arc<AtomicBool>,
        hide_requested: Arc<AtomicBool>,
    ) -> (Self, Task<Message>) {
        let (status_text, status_ok) = match free_claude_launcher::detect_claude_path() {
            Some(path) => (
                format!("已偵測 Claude Desktop\n{}", compact_path(&path, 60)),
                true,
            ),
            None => (
                "尚未找到 Claude.exe，可使用下方自訂路徑".into(),
                false,
            ),
        };

        let mut app = Self {
            provider: Some("OpenRouter".into()),
            base_url: "https://openrouter.ai/api".into(),
            api_key: String::new(),
            api_key_placeholder: "輸入 API Key".into(),
            auth_scheme: Some("bearer".into()),
            use_custom_path: false,
            custom_path: String::new(),
            status_text,
            status_ok,
            toast: None,
            confirming_restore: false,
            is_hidden: false,
            window_id: None,
            quit_requested,
            show_requested,
            hide_requested,
        };

        // 載入已儲存的設定
        if let Some(settings) = free_claude_launcher::get_launcher_settings() {
            app.base_url = settings.real_base_url.clone();
            app.auth_scheme = Some(
                if settings.real_auth_scheme == "x-api-key" {
                    "x-api-key"
                } else {
                    "bearer"
                }
                .into(),
            );
            app.api_key_placeholder = "已儲存 API Key，留空沿用".into();

            if settings.real_base_url.contains("openrouter.ai") {
                app.provider = Some("OpenRouter".into());
            } else if settings.real_base_url.contains("integrate.api.nvidia.com") {
                app.provider = Some("NVIDIA".into());
            } else if settings.real_base_url.contains("api.anthropic.com") {
                app.provider = Some("Anthropic".into());
            } else {
                app.provider = Some("自訂".into());
            }
        }

        (app, Task::none())
    }

    /// 自訂深色紫藍主題
    fn theme(&self) -> Theme {
        Theme::custom(
            "FreeClaudeLauncher",
            Palette {
                background: CLR_BG,
                text: CLR_TEXT,
                primary: CLR_PRIMARY,
                success: CLR_SUCCESS,
                warning: CLR_WARNING,
                danger: CLR_DANGER,
            },
        )
    }

    /// 訂閱：監聽視窗關閉請求 + 輪詢系統匣命令
    fn subscription(&self) -> iced::Subscription<Message> {
        let quit = self.quit_requested.clone();
        let show = self.show_requested.clone();
        let hide = self.hide_requested.clone();

        iced::Subscription::batch(vec![
            window::open_events().map(Message::WindowOpened),
            window::close_requests().map(Message::CloseRequested),
            Self::tray_poll_subscription(quit, show, hide),
        ])
    }

    /// 系統匣輪詢訂閱 — 使用 run_with 避免 Subscription::map 的 non-capturing 限制
    fn tray_poll_subscription(
        quit: Arc<AtomicBool>,
        show: Arc<AtomicBool>,
        hide: Arc<AtomicBool>,
    ) -> iced::Subscription<Message> {
        use std::hash::{Hash, Hasher};

        struct TrayState {
            quit: Arc<AtomicBool>,
            show: Arc<AtomicBool>,
            hide: Arc<AtomicBool>,
        }

        impl Hash for TrayState {
            fn hash<H: Hasher>(&self, state: &mut H) {
                Arc::as_ptr(&self.quit).hash(state);
                Arc::as_ptr(&self.show).hash(state);
                Arc::as_ptr(&self.hide).hash(state);
            }
        }

        iced::Subscription::run_with(
            TrayState { quit, show, hide },
            |state| {
                let q = state.quit.clone();
                let s = state.show.clone();
                let h = state.hide.clone();

                futures::stream::unfold(
                    (q, s, h),
                    move |(q, s, h)| async move {
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        if q.swap(false, Ordering::AcqRel) {
                            Some((Message::TrayQuit, (q, s, h)))
                        } else if s.swap(false, Ordering::AcqRel) || free_claude_launcher::LAUNCHER_SHOW_REQUESTED.swap(false, Ordering::AcqRel) {
                            Some((Message::TrayShow, (q, s, h)))
                        } else if h.swap(false, Ordering::AcqRel) {
                            Some((Message::TrayHide, (q, s, h)))
                        } else {
                            Some((Message::None, (q, s, h)))
                        }
                    },
                )
            },
        )
    }

    fn auth_value(&self) -> &'static str {
        if self.auth_scheme.as_deref() == Some("x-api-key") {
            "x-api-key"
        } else {
            "bearer"
        }
    }

    fn save_config(&self) -> Result<(), String> {
        json_result(free_claude_launcher::save_config(
            &self.base_url,
            &self.api_key,
            self.auth_value(),
        ))
    }

    fn refresh_status(&mut self) {
        match free_claude_launcher::detect_claude_path() {
            Some(path) => {
                self.status_text =
                    format!("已偵測 Claude Desktop\n{}", compact_path(&path, 60));
                self.status_ok = true;
            }
            None => {
                self.status_text = "尚未找到 Claude.exe，可使用下方自訂路徑".into();
                self.status_ok = false;
            }
        }
    }

    // ── 事件處理 ────────────────────────────────────────────────

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ProviderSelected(provider) => {
                self.provider = Some(provider.clone());
                self.toast = None;
                match provider.as_str() {
                    "OpenRouter" => {
                        self.base_url = "https://openrouter.ai/api".into();
                        self.auth_scheme = Some("bearer".into());
                    }
                    "NVIDIA" => {
                        self.base_url = "https://integrate.api.nvidia.com/v1".into();
                        self.auth_scheme = Some("bearer".into());
                    }
                    "Anthropic" => {
                        self.base_url = "https://api.anthropic.com".into();
                        self.auth_scheme = Some("x-api-key".into());
                    }
                    _ => {}
                }
            }
            Message::BaseUrlChanged(url) => {
                self.base_url = url;
                self.toast = None;
            }
            Message::ApiKeyChanged(key) => {
                self.api_key = key;
                self.toast = None;
            }
            Message::AuthSchemeSelected(scheme) => {
                self.auth_scheme = Some(scheme);
                self.toast = None;
            }
            Message::CustomPathToggled(checked) => {
                self.use_custom_path = checked;
                self.toast = None;
            }
            Message::CustomPathChanged(path) => {
                self.custom_path = path;
                self.toast = None;
            }
            Message::SaveAndLaunch => {
                self.confirming_restore = false;
                match self.save_config() {
                    Ok(()) => {
                        let custom = if self.use_custom_path {
                            Some(self.custom_path.as_str())
                        } else {
                            None
                        };
                        match free_claude_launcher::launch_claude(custom) {
                            Ok(path) => {
                                self.api_key.clear();
                                self.api_key_placeholder =
                                    "已儲存 API Key，留空沿用".into();
                                self.refresh_status();
                                self.toast = Some(Toast {
                                    message: format!(
                                        "✅ Claude Desktop 已啟動\n{path}"
                                    ),
                                    is_success: true,
                                });
                            }
                            Err(e) => {
                                self.toast = Some(Toast {
                                    message: format!("❌ 啟動失敗：{e}"),
                                    is_success: false,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        self.toast = Some(Toast {
                            message: format!("❌ 儲存失敗：{e}"),
                            is_success: false,
                        });
                    }
                }
            }
            Message::SaveOnly => {
                self.confirming_restore = false;
                match self.save_config() {
                    Ok(()) => {
                        self.api_key.clear();
                        self.api_key_placeholder = "已儲存 API Key，留空沿用".into();
                        self.toast = Some(Toast {
                            message: "✅ 設定已寫入 Claude。".into(),
                            is_success: true,
                        });
                    }
                    Err(e) => {
                        self.toast = Some(Toast {
                            message: format!("❌ 儲存失敗：{e}"),
                            is_success: false,
                        });
                    }
                }
            }
            Message::RestoreRequested => {
                self.confirming_restore = true;
                self.toast = None;
            }
            Message::ConfirmRestore => {
                self.confirming_restore = false;
                match json_result(free_claude_launcher::restore_official_config()) {
                    Ok(()) => {
                        self.api_key.clear();
                        self.api_key_placeholder = "輸入 API Key".into();
                        self.toast = Some(Toast {
                            message: "✅ Claude 設定已回到官方預設。".into(),
                            is_success: true,
                        });
                    }
                    Err(e) => {
                        self.toast = Some(Toast {
                            message: format!("❌ 還原失敗：{e}"),
                            is_success: false,
                        });
                    }
                }
            }
            Message::CancelRestore => {
                self.confirming_restore = false;
            }
            Message::DismissToast => {
                self.toast = None;
            }
            Message::None => {}
            Message::WindowOpened(id) => {
                self.window_id = Some(id);
            }
            Message::CloseRequested(id) => {
                self.window_id = Some(id);
                self.is_hidden = true;
                return window::set_mode(id, Mode::Hidden);
            }
            Message::TrayQuit => {
                if let Some(id) = self.window_id {
                    return window::close(id);
                }
            }
            Message::TrayShow => {
                self.is_hidden = false;
                if let Some(id) = self.window_id {
                    return Task::batch([
                        window::set_mode(id, Mode::Windowed),
                        window::gain_focus(id),
                    ]);
                }
            }
            Message::TrayHide => {
                self.is_hidden = true;
                if let Some(id) = self.window_id {
                    return window::set_mode(id, Mode::Hidden);
                }
            }
        }
        Task::none()
    }

    // ── 介面佈局 ────────────────────────────────────────────────

    fn view(&self) -> Element<'_, Message> {
        // ── 標題區 ──
        let header = column![
            text("Free Claude Launcher")
                .size(28)
                .font(Font {
                    weight: Weight::Bold,
                    ..Default::default()
                }),
            text("本機 Proxy：127.0.0.1:3000")
                .size(13)
                .color(CLR_TEXT_DIM),
        ]
        .spacing(4);

        // ── 狀態卡片 ──
        let status_color = if self.status_ok { CLR_SUCCESS } else { CLR_WARNING };
        let status_lines: Vec<&str> = self.status_text.lines().collect();

        let mut status_col = column![text(
            status_lines.first().copied().unwrap_or("").to_string()
        )
        .size(14)
        .color(status_color)
        .font(Font {
            weight: Weight::Semibold,
            ..Default::default()
        })]
        .spacing(3);

        if let Some(path_line) = status_lines.get(1) {
            status_col =
                status_col.push(text(path_line.to_string()).size(12).color(CLR_TEXT_DIM));
        }

        let status_card = container(status_col)
            .style(|_theme| container::Style {
                text_color: None,
                background: Some(Background::Color(CLR_CARD)),
                border: Border {
                    radius: 10.0.into(),
                    width: 1.0,
                    color: CLR_BORDER,
                },
                shadow: Shadow::default(),
                snap: false,
            })
            .padding([14, 18])
            .width(Length::Fill);

        // ── 區段標題 ──
        let section_title = text("連線設定")
            .size(18)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            });

        // ── 表單 ──
        let provider_options: Vec<String> =
            PROVIDERS.iter().map(|s| s.to_string()).collect();
        let auth_options: Vec<String> =
            AUTH_SCHEMES.iter().map(|s| s.to_string()).collect();

        let form = column![
            form_row(
                "API 供應商",
                pick_list(
                    provider_options,
                    self.provider.clone(),
                    Message::ProviderSelected,
                )
                .placeholder("選擇供應商...")
                .width(Length::Fill)
                .into(),
            ),
            form_row(
                "Gateway URL",
                text_input("https://...", &self.base_url)
                    .on_input(Message::BaseUrlChanged)
                    .padding(10)
                    .size(14)
                    .into(),
            ),
            form_row(
                "API Key",
                text_input(&self.api_key_placeholder, &self.api_key)
                    .on_input(Message::ApiKeyChanged)
                    .secure(true)
                    .padding(10)
                    .size(14)
                    .into(),
            ),
            form_row(
                "驗證方式",
                pick_list(
                    auth_options,
                    self.auth_scheme.clone(),
                    Message::AuthSchemeSelected,
                )
                .width(Length::Fill)
                .into(),
            ),
        ]
        .spacing(14);

        // ── 自訂路徑 ──
        let mut custom_input =
            text_input("C:\\Users\\...\\Claude.exe", &self.custom_path)
                .padding(10)
                .size(14);
        if self.use_custom_path {
            custom_input = custom_input.on_input(Message::CustomPathChanged);
        }

        let custom_section = column![
            checkbox(self.use_custom_path)
                .label("使用自訂 Claude.exe 路徑")
                .on_toggle(Message::CustomPathToggled)
                .text_size(14)
                .spacing(8),
            custom_input,
        ]
        .spacing(8);

        // ── 組裝主要內容 ──
        let mut content = column![
            header,
            status_card,
            section_title,
            rule::horizontal(1),
            form,
            custom_section,
        ]
        .spacing(18)
        .max_width(540);

        // ── Toast 通知 ──
        if let Some(ref toast) = self.toast {
            let (bg, border_clr) = if toast.is_success {
                (
                    Color::from_rgba(0.298, 0.831, 0.494, 0.10),
                    CLR_SUCCESS,
                )
            } else {
                (
                    Color::from_rgba(1.0, 0.380, 0.380, 0.10),
                    CLR_DANGER,
                )
            };
            let txt_clr = if toast.is_success {
                CLR_SUCCESS
            } else {
                CLR_DANGER
            };

            let toast_widget = container(
                row![
                    text(toast.message.clone())
                        .size(13)
                        .color(txt_clr)
                        .width(Length::Fill),
                    button(text("✕").size(14).color(CLR_TEXT_DIM))
                        .on_press(Message::DismissToast)
                        .style(ghost_btn_style)
                        .padding([2, 8]),
                ]
                .align_y(Alignment::Center),
            )
            .style(move |_theme| container::Style {
                text_color: None,
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: 8.0.into(),
                    width: 1.0,
                    color: border_clr,
                },
                shadow: Shadow::default(),
                snap: false,
            })
            .padding([10, 14])
            .width(Length::Fill);

            content = content.push(toast_widget);
        }

        // ── 還原確認列 ──
        if self.confirming_restore {
            let confirm_bar = container(
                row![
                    text("⚠ 確定要還原為官方設定？將移除 Gateway 設定。")
                        .size(13)
                        .color(CLR_WARNING)
                        .width(Length::Fill),
                    button(text("確定").size(13).color(Color::WHITE))
                        .on_press(Message::ConfirmRestore)
                        .style(danger_btn_style)
                        .padding([6, 16]),
                    button(text("取消").size(13).color(CLR_TEXT_DIM))
                        .on_press(Message::CancelRestore)
                        .style(secondary_btn_style)
                        .padding([6, 16]),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .style(|_theme| container::Style {
                text_color: None,
                background: Some(Background::Color(Color::from_rgba(
                    1.0, 0.694, 0.298, 0.08,
                ))),
                border: Border {
                    radius: 8.0.into(),
                    width: 1.0,
                    color: CLR_WARNING,
                },
                shadow: Shadow::default(),
                snap: false,
            })
            .padding([10, 14])
            .width(Length::Fill);

            content = content.push(confirm_bar);
        }

        // ── 底部按鈕列 ──
        let buttons = row![
            button(
                text("🚀 儲存並啟動 Claude")
                    .size(14)
                    .font(Font {
                        weight: Weight::Semibold,
                        ..Default::default()
                    })
            )
            .on_press(Message::SaveAndLaunch)
            .style(primary_btn_style)
            .padding([10, 24]),
            button(text("💾 僅儲存").size(14))
                .on_press(Message::SaveOnly)
                .style(secondary_btn_style)
                .padding([10, 20]),
            button(text("↩ 還原官方").size(14).color(CLR_TEXT_DIM))
                .on_press(Message::RestoreRequested)
                .style(outline_btn_style)
                .padding([10, 20]),
        ]
        .spacing(10);

        content = content.push(buttons);

        // ── 外層容器 ──
        container(content).padding(30).center_x(Length::Fill).into()
    }
}

// ════════════════════════════════════════════════════════════════
//  按鈕樣式
// ════════════════════════════════════════════════════════════════

/// 主要操作按鈕：紫藍背景
fn primary_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => CLR_PRIMARY_HOVER,
        button::Status::Pressed => CLR_PRIMARY_PRESS,
        _ => CLR_PRIMARY,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: Color::WHITE,
        border: Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 次要按鈕：深色背景 + 邊框
fn secondary_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => CLR_BTN_SEC_HOVER,
        _ => CLR_BTN_SEC,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: CLR_TEXT,
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: CLR_BORDER,
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 輪廓按鈕：透明背景 + 邊框，hover 變紅
fn outline_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    let (bg, border_clr) = match status {
        button::Status::Hovered => (CLR_BTN_SEC_HOVER, CLR_DANGER),
        _ => (Color::TRANSPARENT, CLR_BORDER),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: CLR_TEXT_DIM,
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: border_clr,
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 危險按鈕：紅色背景
fn danger_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => CLR_DANGER_HOVER,
        _ => CLR_DANGER,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: Color::WHITE,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 幽靈按鈕：無背景（用於關閉 toast 等）
fn ghost_btn_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: CLR_TEXT_DIM,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

// ════════════════════════════════════════════════════════════════
//  輔助函式
// ════════════════════════════════════════════════════════════════

/// 表單列：左側標籤 + 右側控件
fn form_row<'a>(label: &str, widget: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(14)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            })
            .width(110),
        widget,
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// 縮短檔案路徑顯示
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

/// 解析 lib 回傳的 JSON 結果
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

/// 從嵌入的 ico 載入視窗圖示
fn load_icon() -> Option<window::Icon> {
    let ico_data = include_bytes!("../icon.ico");
    let img = image::load_from_memory(ico_data).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

// ════════════════════════════════════════════════════════════════
//  系統匣
// ════════════════════════════════════════════════════════════════

/// 建立系統匣圖示並執行訊息迴圈（在獨立 thread 執行）
fn run_tray_icon(
    quit_requested: Arc<AtomicBool>,
    show_requested: Arc<AtomicBool>,
    hide_requested: Arc<AtomicBool>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_tray_icon_inner(quit_requested, show_requested, hide_requested)
    }));
    if let Err(e) = result {
        eprintln!("[tray] PANIC: {e:?}");
    }
}

fn run_tray_icon_inner(
    quit_requested: Arc<AtomicBool>,
    show_requested: Arc<AtomicBool>,
    hide_requested: Arc<AtomicBool>,
) {
    eprintln!("[tray] run_tray_icon 啟動");
    // 從 icon.ico 載入圖示（縮小為 32x32 適合系統匣）
    let icon_data = include_bytes!("../icon.ico");
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
    let resized = image::imageops::resize(
        &img,
        32,
        32,
        image::imageops::FilterType::Lanczos3,
    );
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

    // 訊息迴圈：泵送 Windows 訊息 + 處理選單事件
    loop {
        // 檢查選單事件
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id.0.as_str() {
                "quit" => {
                    quit_requested.store(true, Ordering::Release);
                    return;
                }
                "show" => {
                    show_requested.store(true, Ordering::Release);
                }
                "hide" => {
                    hide_requested.store(true, Ordering::Release);
                }
                _ => {}
            }
        }

        // 檢查系統匣點擊事件（左鍵切換顯示）
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                show_requested.store(true, Ordering::Release);
            }
        }

        // Windows 訊息泵送（tray-icon 的隱藏視窗需要）
        #[cfg(target_os = "windows")]
        unsafe {
            use winapi::um::winuser::{
                DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
            };
            let mut msg = std::mem::zeroed::<MSG>();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == winapi::um::winuser::WM_QUIT {
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

// ════════════════════════════════════════════════════════════════
//  程式進入點
// ════════════════════════════════════════════════════════════════

fn main() -> iced::Result {
    // 先啟動背景 proxy server
    if let Err(e) = free_claude_launcher::start_server_background() {
        // 嘗試向舊實例發送喚醒請求
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(800))
            .build();
        if let Ok(client) = client {
            if let Ok(resp) = client.get("http://127.0.0.1:3000/__launcher_show").send() {
                if resp.status().is_success() {
                    return Ok(()); // 喚醒成功，新實例直接退出
                }
            }
        }

        #[cfg(target_os = "windows")]
        unsafe {
            use std::os::windows::ffi::OsStrExt;
            let title: Vec<u16> = std::ffi::OsStr::new("啟動失敗")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let msg_str = format!(
                "無法啟動 Proxy 伺服器。這通常是因為程式已在背景運行（請檢查系統匣/系統列圖示）。\n\n錯誤詳情：{}",
                e
            );
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
        return Ok(());
    }

    // 建立系統匣的共享狀態
    let quit_requested = Arc::new(AtomicBool::new(false));
    let show_requested = Arc::new(AtomicBool::new(false));
    let hide_requested = Arc::new(AtomicBool::new(false));

    // 在背景 thread 啟動系統匣圖示
    let tray_quit = quit_requested.clone();
    let tray_show = show_requested.clone();
    let tray_hide = hide_requested.clone();
    std::thread::spawn(move || {
        run_tray_icon(tray_quit, tray_show, tray_hide);
    });

    // 傳遞共享狀態給應用程式
    let app_quit = quit_requested.clone();
    let app_show = show_requested.clone();
    let app_hide = hide_requested.clone();

    iced::application(
        move || LauncherApp::new(app_quit.clone(), app_show.clone(), app_hide.clone()),
        LauncherApp::update,
        LauncherApp::view,
    )
    .subscription(LauncherApp::subscription)
    .title("FreeClaudeLauncher")
    .theme(LauncherApp::theme)
    .window(window::Settings {
        size: iced::Size::new(600.0, 620.0),
        resizable: false,
        icon: load_icon(),
        exit_on_close_request: false,
        ..Default::default()
    })
    .run()
}
