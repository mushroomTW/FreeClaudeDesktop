use iced::{
    window::{self, Id},
    Subscription, Task, Theme,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Mutex;

// ════════════════════════════════════════════════════════════════
//  應用程式狀態
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

impl ThemeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "dark" => Self::Dark,
            "system" => Self::System,
            _ => Self::Light,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub is_success: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    General,
    Models,
    Extensions,
    Optimizations,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
    ThemeModeSelected(ThemeMode),
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
    ComputerMcpServerToggled(bool),
    WebFetchPrivateNetworkToggled(bool),
    WebFetchAllowedSchemesChanged(String),
    ReasoningReplayModeSelected(String),
    TransportTypeSelected(String),
    ModelReasoningLevelSelected(String, String),
    Model1mToggled(String, bool),
    RealModelSelected(Option<String>),
    RealModelSonnetSelected(Option<String>),
    RealModelOpusSelected(Option<String>),
    RealModelHaikuSelected(Option<String>),
    RefreshModels,
    ResyncFromOfficial,
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
    pub enable_computer_mcp_server: bool,
    pub web_fetch_allow_private_networks: bool,
    pub web_fetch_allowed_schemes: String,
    pub reasoning_replay_mode: String,
    pub transport_type: String,
    pub theme_mode: ThemeMode,
    pub discovered_models: Vec<String>,
    pub model_options: Vec<String>,
    pub model_reasoning_overrides: HashMap<String, String>,
    pub model_1m_overrides: HashMap<String, bool>,
    pub real_model: Option<String>,
    pub real_model_sonnet: Option<String>,
    pub real_model_opus: Option<String>,
    pub real_model_haiku: Option<String>,
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
            enable_title_generation_skip: true,
            enable_suggestion_mode_skip: true,
            enable_filepath_extraction_mock: true,
            enable_web_server_tools: false,
            enable_computer_mcp_server: false,
            web_fetch_allow_private_networks: false,
            web_fetch_allowed_schemes: "http,https".to_string(),
            reasoning_replay_mode: "separate".to_string(),
            transport_type: "openai_chat".to_string(),
            theme_mode: ThemeMode::Light,
            discovered_models: Vec::new(),
            model_options: vec!["(自動/動態別名)".to_string()],
            model_reasoning_overrides: HashMap::new(),
            model_1m_overrides: HashMap::new(),
            real_model: None,
            real_model_sonnet: None,
            real_model_opus: None,
            real_model_haiku: None,
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
                app.provider = Some("自訂".into());
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

            // 還原 Per-feature optimization settings
            app.enable_quota_check_mock = settings.enable_quota_check_mock;
            app.enable_prefix_detection = settings.enable_prefix_detection;
            app.enable_title_generation_skip = settings.enable_title_generation_skip;
            app.enable_suggestion_mode_skip = settings.enable_suggestion_mode_skip;
            app.enable_filepath_extraction_mock = settings.enable_filepath_extraction_mock;
            app.enable_web_server_tools = settings.enable_web_server_tools;
            app.enable_computer_mcp_server = settings.enable_computer_mcp_server;
            app.web_fetch_allow_private_networks = settings.web_fetch_allow_private_networks;
            app.web_fetch_allowed_schemes = settings.web_fetch_allowed_schemes;
            app.reasoning_replay_mode = settings.reasoning_replay_mode;
            app.transport_type = settings.transport_type;
            app.theme_mode = ThemeMode::parse(&settings.theme_mode);
            app.discovered_models = settings.discovered_models;
            app.update_model_options();
            app.model_reasoning_overrides = settings.model_reasoning_overrides;
            app.model_1m_overrides = settings.model_1m_overrides;
            app.real_model = settings.real_model;
            app.real_model_sonnet = settings.real_model_sonnet;
            app.real_model_opus = settings.real_model_opus;
            app.real_model_haiku = settings.real_model_haiku;

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
        let is_dark = match self.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => crate::platform::is_system_dark_mode(),
        };
        if is_dark {
            Theme::Dark
        } else {
            Theme::Light
        }
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
            self.enable_computer_mcp_server,
            self.web_fetch_allow_private_networks,
            &self.reasoning_replay_mode,
            &self.transport_type,
            &self.web_fetch_allowed_schemes,
            self.theme_mode.as_str(),
            &self.model_reasoning_overrides,
            &self.model_1m_overrides,
            self.real_model.clone(),
            self.real_model_sonnet.clone(),
            self.real_model_opus.clone(),
            self.real_model_haiku.clone(),
        )
        .map_err(|e| e.to_string())
    }

    fn reload_model_settings(&mut self) {
        if let Some(settings) = crate::get_launcher_settings() {
            self.discovered_models = settings.discovered_models;
            self.update_model_options();
            self.model_reasoning_overrides = settings.model_reasoning_overrides;
            self.model_1m_overrides = settings.model_1m_overrides;
            self.real_model = settings.real_model;
            self.real_model_sonnet = settings.real_model_sonnet;
            self.real_model_opus = settings.real_model_opus;
            self.real_model_haiku = settings.real_model_haiku;
        }
    }

    pub fn update_model_options(&mut self) {
        let mut opts = vec!["(自動/動態別名)".to_string()];
        opts.extend(self.discovered_models.clone());
        self.model_options = opts;
    }

    fn save_theme_mode(&self) {
        if let Some(mut settings) = crate::get_launcher_settings() {
            settings.theme_mode = self.theme_mode.as_str().to_string();
            let _ = crate::save_launcher_settings(&settings);
        }
    }

    pub fn refresh_status(&mut self) {
        match crate::detect_claude_path() {
            Some(path) => {
                self.status_text = format!("已偵測 Claude Desktop\n{}", path.display());
                self.status_ok = true;
            }
            None => {
                self.status_text = "尚未找到 Claude.exe，可使用下方自訂路徑".into();
                self.status_ok = false;
            }
        }
    }
}

mod update;
mod utils;

pub use utils::{compact_path, json_result};
