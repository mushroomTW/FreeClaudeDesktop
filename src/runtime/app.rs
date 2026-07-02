use iced::{
    window::{self, Id, Mode},
    Subscription, Task, Theme,
    Theme::Dark,
};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Mutex;

// ════════════════════════════════════════════════════════════════
//  應用程式狀態
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub is_success: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    General,
    Advanced,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
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
    WindowOpened(Id),
    CloseRequested(Id),
    TrayQuit,
    TrayShow,
    TrayHide,
    // Per-feature optimization toggles
    QuotaCheckMockToggled(bool),
    PrefixDetectionToggled(bool),
    TitleGenerationSkipToggled(bool),
    SuggestionModeSkipToggled(bool),
    FilepathExtractionMockToggled(bool),
    WebServerToolsToggled(bool),
    WebFetchPrivateNetworkToggled(bool),
    SafetyClassifierHandlingToggled(bool),
    ReasoningReplayModeSelected(String),
    TransportTypeSelected(String),
}

pub struct LauncherApp {
    pub provider: Option<String>,
    pub base_url: String,
    pub api_key: String,
    pub api_key_placeholder: String,
    pub auth_scheme: Option<String>,
    pub use_custom_path: bool,
    pub custom_path: String,
    pub status_text: String,
    pub status_ok: bool,
    pub toast: Option<Toast>,
    pub confirming_restore: bool,
    pub is_hidden: bool,
    pub window_id: Option<Id>,
    pub tray_rx: Arc<Mutex<UnboundedReceiver<Message>>>,
    pub current_port: u16,
    pub current_tab: Tab,
    // Per-feature optimization toggles
    pub enable_quota_check_mock: bool,
    pub enable_prefix_detection: bool,
    pub enable_title_generation_skip: bool,
    pub enable_suggestion_mode_skip: bool,
    pub enable_filepath_extraction_mock: bool,
    pub enable_web_server_tools: bool,
    pub web_fetch_allow_private_networks: bool,
    pub enable_safety_classifier_handling: bool,
    pub reasoning_replay_mode: String,
    pub transport_type: String,
}

impl LauncherApp {
    pub fn new(
        port: u16,
        tray_rx: Arc<Mutex<UnboundedReceiver<Message>>>,
    ) -> (Self, Task<Message>) {
        let mut app = Self {
            provider: None,
            base_url: String::new(),
            api_key: String::new(),
            api_key_placeholder: "輸入 API Key".into(),
            auth_scheme: None,
            use_custom_path: false,
            custom_path: String::new(),
            status_text: "正在偵測...".into(),
            status_ok: false,
            toast: None,
            confirming_restore: false,
            is_hidden: false,
            window_id: None,
            tray_rx,
            current_port: port,
            // Per-feature optimization toggles (defaults)
            enable_quota_check_mock: true,
            enable_prefix_detection: true,
            enable_title_generation_skip: false,
            enable_suggestion_mode_skip: true,
            enable_filepath_extraction_mock: true,
            enable_web_server_tools: false,
            web_fetch_allow_private_networks: false,
            enable_safety_classifier_handling: true,
            reasoning_replay_mode: "separate".to_string(),
            transport_type: "openai_chat".to_string(),
            current_tab: Tab::General,
        };

        // 讀取本地配置以還原狀態
        if let Some(settings) = crate::get_launcher_settings() {
            // 還原 provider
            if settings.real_base_url.contains("openrouter.ai") {
                app.provider = Some("OpenRouter".into());
                app.auth_scheme = Some("bearer".into());
            } else if settings.real_base_url.contains("api.nvidia.com") {
                app.provider = Some("NVIDIA".into());
                app.auth_scheme = Some("bearer".into());
            } else if settings.real_base_url.contains("api.anthropic.com") {
                app.provider = Some("Anthropic".into());
                app.auth_scheme = Some("x-api-key".into());
            } else {
                app.provider = Some("自訂".into());
                app.auth_scheme = Some(settings.real_auth_scheme.clone());
            }
            app.base_url = settings.real_base_url;

            // API key placeholder 提示
            if !settings.real_api_key.is_empty() {
                app.api_key_placeholder = "已儲存 API Key，留空沿用".into();
            }

            // �跙 Per-feature optimization settings
            app.enable_quota_check_mock = settings.enable_quota_check_mock;
            app.enable_prefix_detection = settings.enable_prefix_detection;
            app.enable_title_generation_skip = settings.enable_title_generation_skip;
            app.enable_suggestion_mode_skip = settings.enable_suggestion_mode_skip;
            app.enable_filepath_extraction_mock = settings.enable_filepath_extraction_mock;
            app.enable_web_server_tools = settings.enable_web_server_tools;
            app.web_fetch_allow_private_networks = settings.web_fetch_allow_private_networks;
            app.enable_safety_classifier_handling = settings.enable_safety_classifier_handling;
            app.reasoning_replay_mode = settings.reasoning_replay_mode;
            app.transport_type = settings.transport_type;

            // 自訂路徑檢查
            if let Some(target) = crate::detect_claude_path() {
                let default_paths = crate::launcher::known_claude_paths();
                let target_str = target.to_string_lossy().to_lowercase();
                let is_official =
                    default_paths.contains(&target) || target_str.contains("windowsapps");
                if !is_official {
                    app.use_custom_path = true;
                    app.custom_path = target.to_string_lossy().to_string();
                }
            }
        } else {
            // 預設 OpenRouter
            app.provider = Some("OpenRouter".into());
            app.base_url = "https://openrouter.ai/api".into();
            app.auth_scheme = Some("bearer".into());
        }

        app.refresh_status();

        (app, Task::none())
    }

    pub fn theme(&self) -> Theme {
        Dark
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let tray_rx = self.tray_rx.clone();

        Subscription::batch(vec![
            window::open_events().map(Message::WindowOpened),
            window::close_requests().map(Message::CloseRequested),
            Self::tray_channel_subscription(tray_rx),
        ])
    }

    fn tray_channel_subscription(
        rx: Arc<Mutex<UnboundedReceiver<Message>>>,
    ) -> Subscription<Message> {
        use std::hash::{Hash, Hasher};

        struct TrayState {
            rx: Arc<Mutex<UnboundedReceiver<Message>>>,
        }

        impl Hash for TrayState {
            fn hash<H: Hasher>(&self, state: &mut H) {
                Arc::as_ptr(&self.rx).hash(state);
            }
        }

        Subscription::run_with(TrayState { rx }, |state| {
            let rx_clone = state.rx.clone();
            futures::stream::unfold(rx_clone, |rx| async move {
                let msg = {
                    let mut guard = rx.lock().await;
                    guard.recv().await
                };
                msg.map(|m| (m, rx))
            })
        })
    }

    pub fn auth_value(&self) -> &'static str {
        if self.auth_scheme.as_deref() == Some("x-api-key") {
            "x-api-key"
        } else {
            "bearer"
        }
    }

    pub fn save_config(&self) -> Result<(), String> {
        crate::save_config(
            self.current_port,
            &self.base_url,
            &self.api_key,
            self.auth_value(),
            self.enable_quota_check_mock,
            self.enable_prefix_detection,
            self.enable_title_generation_skip,
            self.enable_suggestion_mode_skip,
            self.enable_filepath_extraction_mock,
            self.enable_web_server_tools,
            self.web_fetch_allow_private_networks,
            self.enable_safety_classifier_handling,
            &self.reasoning_replay_mode,
            &self.transport_type,
        )
        .map_err(|e| e.to_string())
    }

    pub fn refresh_status(&mut self) {
        match crate::detect_claude_path() {
            Some(path) => {
                self.status_text = format!("已偵測 Claude Desktop\n{}", compact_path(&path, 60));
                self.status_ok = true;
            }
            None => {
                self.status_text = "尚未找到 Claude.exe，可使用下方自訂路徑".into();
                self.status_ok = false;
            }
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ProviderSelected(provider) => {
                self.provider = Some(provider.clone());
                self.toast = None;
                match provider.as_str() {
                    "OpenRouter" => {
                        self.base_url = "https://openrouter.ai/api".into();
                        self.auth_scheme = Some("bearer".into());
                        self.transport_type = "openai_chat".to_string();
                    }
                    "NVIDIA" => {
                        self.base_url = "https://integrate.api.nvidia.com/v1".into();
                        self.auth_scheme = Some("bearer".into());
                        self.transport_type = "openai_chat".to_string();
                    }
                    "Anthropic" => {
                        self.base_url = "https://api.anthropic.com".into();
                        self.auth_scheme = Some("x-api-key".into());
                        self.transport_type = "anthropic_messages".to_string();
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
                            Some(Path::new(&self.custom_path))
                        } else {
                            None
                        };
                        match crate::launch_claude(custom) {
                            Ok(path) => {
                                self.api_key.clear();
                                self.api_key_placeholder = "已儲存 API Key，留空沿用".into();
                                self.refresh_status();
                                self.toast = Some(Toast {
                                    message: format!(
                                        "✅ Claude Desktop 已啟動\n{}",
                                        path.display()
                                    ),
                                    is_success: true,
                                });
                                if let Some(id) = self.window_id {
                                    self.is_hidden = true;
                                    return window::set_mode(id, Mode::Hidden);
                                }
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
                match crate::restore_official_config() {
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
            Message::TabSelected(tab) => self.current_tab = tab,
            // Per-feature optimization toggles
            Message::QuotaCheckMockToggled(v) => self.enable_quota_check_mock = v,
            Message::PrefixDetectionToggled(v) => self.enable_prefix_detection = v,
            Message::TitleGenerationSkipToggled(v) => self.enable_title_generation_skip = v,
            Message::SuggestionModeSkipToggled(v) => self.enable_suggestion_mode_skip = v,
            Message::FilepathExtractionMockToggled(v) => self.enable_filepath_extraction_mock = v,
            Message::WebServerToolsToggled(v) => self.enable_web_server_tools = v,
            Message::WebFetchPrivateNetworkToggled(v) => self.web_fetch_allow_private_networks = v,
            Message::SafetyClassifierHandlingToggled(v) => {
                self.enable_safety_classifier_handling = v
            }
            Message::ReasoningReplayModeSelected(v) => self.reasoning_replay_mode = v,
            Message::TransportTypeSelected(v) => self.transport_type = v,
        }
        Task::none()
    }
}

pub fn compact_path(path: &Path, max_chars: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.len() <= max_chars {
        return path_str.into_owned();
    }
    let tail = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("Claude.exe");
    format!("...\\{tail}")
}

pub fn json_result(value: Value) -> Result<(), String> {
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
