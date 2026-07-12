use iced::{
    Subscription, Task, Theme,
    window::{self, Id},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Saving,
    Refreshing,
    Launching,
    Resyncing,
    Restoring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    SaveOnly,
    SaveAndLaunch,
    RefreshModels,
}

#[derive(Clone)]
pub struct LoadedSettings(pub crate::Settings);

impl std::fmt::Debug for LoadedSettings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LoadedSettings(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum JobState {
    #[default]
    Idle,
    Running(JobKind),
    Failed(String),
}

#[derive(Debug, Default)]
pub struct JobTracker {
    next_id: u64,
    state: JobState,
    abort: Option<futures::future::AbortHandle>,
}

impl JobTracker {
    fn begin(&mut self, kind: JobKind) -> (u64, futures::future::AbortRegistration) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        self.next_id = self.next_id.wrapping_add(1);
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        self.abort = Some(abort);
        self.state = JobState::Running(kind);
        (self.next_id, registration)
    }

    fn accept(&mut self, id: u64) -> bool {
        if id != self.next_id {
            return false;
        }
        self.abort = None;
        self.state = JobState::Idle;
        true
    }

    fn fail(&mut self, id: u64, error: String) -> bool {
        if id != self.next_id {
            return false;
        }
        self.abort = None;
        self.state = JobState::Failed(error);
        true
    }

    fn cancel(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        self.state = JobState::Idle;
    }

    fn state(&self) -> &JobState {
        &self.state
    }
}

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
    WebFetchPrivateNetworkToggled(bool),
    WebFetchAllowedSchemesChanged(String),
    ReasoningReplayModeSelected(String),
    TransportTypeSelected(String),
    ModelReasoningLevelSelected(String, String),
    Model1mToggled(String, bool),
    ModelVisibilityToggled(String, bool),
    RealModelSelected(Option<String>),
    RealModelSonnetSelected(Option<String>),
    RealModelOpusSelected(Option<String>),
    RealModelHaikuSelected(Option<String>),
    RefreshModels,
    ResyncFromOfficial,
    ConfigFinished(
        u64,
        ConfigAction,
        Result<crate::config_service::SaveConfigOutput, String>,
    ),
    LaunchFinished(u64, Result<std::path::PathBuf, String>),
    ResyncFinished(u64, Result<(), String>),
    RestoreFinished(u64, Result<(), String>),
    StatusLoaded(Option<std::path::PathBuf>),
    SettingsLoaded(Result<Option<Box<LoadedSettings>>, String>),
    ThemeSaved(Result<(), String>),
    LanguageSelected(crate::config::Language),
    LanguageSaved(Result<(), String>),
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
    pub web_fetch_allowed_schemes: String,
    pub reasoning_replay_mode: String,
    pub transport_type: String,
    pub theme_mode: ThemeMode,
    pub language: crate::config::Language,
    pub discovered_models: Vec<String>,
    pub model_options: Vec<String>,
    pub provider_options: Vec<String>,
    pub model_reasoning_overrides: HashMap<String, String>,
    pub model_1m_overrides: HashMap<String, bool>,
    pub model_visibility_overrides: HashMap<String, bool>,
    pub real_model: Option<String>,
    pub real_model_sonnet: Option<String>,
    pub real_model_opus: Option<String>,
    pub real_model_haiku: Option<String>,
    pub jobs: JobTracker,
}

impl LauncherApp {
    pub fn new(
        port: u16,
        tray_rx: Arc<Mutex<UnboundedReceiver<Message>>>,
    ) -> (Self, Task<Message>) {
        let app = Self {
            provider: Some("OpenRouter".into()),
            base_url: "https://openrouter.ai/api".into(),
            api_key: String::new(),
            api_key_placeholder: "輸入 API Key".into(),
            auth_scheme: Some("bearer".into()),
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
            web_fetch_allow_private_networks: false,
            web_fetch_allowed_schemes: "http,https".to_string(),
            reasoning_replay_mode: "separate".to_string(),
            transport_type: "openai_chat".to_string(),
            theme_mode: ThemeMode::Light,
            language: crate::config::Language::En,
            discovered_models: Vec::new(),
            model_options: vec![crate::config::Language::En.tr("auto_alias").to_string()],
            provider_options: vec![
                "OpenRouter".to_string(),
                "NVIDIA".to_string(),
                crate::config::Language::En.tr("custom").to_string(),
            ],
            model_reasoning_overrides: HashMap::new(),
            model_1m_overrides: HashMap::new(),
            model_visibility_overrides: HashMap::new(),
            real_model: None,
            real_model_sonnet: None,
            real_model_opus: None,
            real_model_haiku: None,
            current_tab: Tab::General,
            jobs: JobTracker::default(),
        };

        let status_task = Task::perform(
            async {
                tokio::task::spawn_blocking(crate::detect_claude_path)
                    .await
                    .ok()
                    .flatten()
            },
            Message::StatusLoaded,
        );
        let settings_task = Task::perform(
            async {
                tokio::task::spawn_blocking(crate::config::load_launcher_settings)
                    .await
                    .map_err(|error| error.to_string())?
                    .map(|settings| settings.map(|settings| Box::new(LoadedSettings(settings))))
                    .map_err(|error| error.to_string())
            },
            Message::SettingsLoaded,
        );

        (app, Task::batch([status_task, settings_task]))
    }

    pub fn theme(&self) -> Theme {
        let is_dark = match self.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => crate::platform::is_system_dark_mode(),
        };
        if is_dark { Theme::Dark } else { Theme::Light }
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

    pub fn is_busy(&self) -> bool {
        matches!(self.jobs.state(), JobState::Running(_))
    }

    fn config_input(&self) -> crate::config_service::SaveConfigInput {
        crate::config_service::SaveConfigInput {
            port: self.current_port,
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            auth_scheme: self.auth_value().to_string(),
            enable_quota_check_mock: self.enable_quota_check_mock,
            enable_prefix_detection: self.enable_prefix_detection,
            enable_title_generation_skip: self.enable_title_generation_skip,
            enable_suggestion_mode_skip: self.enable_suggestion_mode_skip,
            enable_filepath_extraction_mock: self.enable_filepath_extraction_mock,
            enable_web_server_tools: self.enable_web_server_tools,
            web_fetch_allow_private_networks: self.web_fetch_allow_private_networks,
            reasoning_replay_mode: self.reasoning_replay_mode.clone(),
            transport_type: self.transport_type.clone(),
            web_fetch_allowed_schemes: self.web_fetch_allowed_schemes.clone(),
            theme_mode: self.theme_mode.as_str().to_string(),
            language: self.language.as_str().to_string(),
            model_reasoning_overrides: self.model_reasoning_overrides.clone(),
            model_1m_overrides: self.model_1m_overrides.clone(),
            model_visibility_overrides: self.model_visibility_overrides.clone(),
            real_model: self.real_model.clone(),
            real_model_sonnet: self.real_model_sonnet.clone(),
            real_model_opus: self.real_model_opus.clone(),
            real_model_haiku: self.real_model_haiku.clone(),
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
            false,
            self.web_fetch_allow_private_networks,
            &self.reasoning_replay_mode,
            &self.transport_type,
            &self.web_fetch_allowed_schemes,
            self.theme_mode.as_str(),
            self.language.as_str(),
            &self.model_reasoning_overrides,
            &self.model_1m_overrides,
            &self.model_visibility_overrides,
            self.real_model.clone(),
            self.real_model_sonnet.clone(),
            self.real_model_opus.clone(),
            self.real_model_haiku.clone(),
        )
        .map_err(|e| e.to_string())
    }

    pub fn update_model_options(&mut self) {
        let mut opts = vec![self.language.tr("auto_alias").to_string()];
        opts.extend(self.discovered_models.clone());
        self.model_options = opts;
    }

    pub fn update_provider_options(&mut self) {
        self.provider_options = vec![
            "OpenRouter".to_string(),
            "NVIDIA".to_string(),
            self.language.tr("custom").to_string(),
        ];
    }

    fn apply_status(&mut self, path: Option<std::path::PathBuf>) {
        match path {
            Some(path) => {
                let status_prefix = self.language.tr("detected_claude");
                self.status_text = format!("{}\n{}", status_prefix, path.display());
                self.status_ok = true;
                let known = crate::launcher::known_claude_paths();
                let path_text = path.to_string_lossy();
                if !known.contains(&path) && !path_text.to_lowercase().contains("windowsapps") {
                    self.use_custom_path = true;
                    self.custom_path = path_text.into_owned();
                }
            }
            None => {
                self.status_text = self.language.tr("not_found_claude").into();
                self.status_ok = false;
            }
        }
    }

    fn apply_settings(&mut self, settings: crate::Settings) {
        self.language = crate::config::Language::parse(&settings.language);
        if settings.real_base_url.contains("openrouter.ai") {
            self.provider = Some("OpenRouter".into());
            self.auth_scheme = Some("bearer".into());
        } else if settings.real_base_url.contains("api.nvidia.com") {
            self.provider = Some("NVIDIA".into());
            self.auth_scheme = Some("bearer".into());
        } else {
            self.provider = Some(self.language.tr("custom").into());
            self.auth_scheme = Some(settings.real_auth_scheme.clone());
        }
        self.base_url = settings.real_base_url;
        if !settings.real_api_key.is_empty() {
            self.api_key_placeholder = self.language.tr("key_saved_tip").into();
        }
        self.enable_quota_check_mock = settings.enable_quota_check_mock;
        self.enable_prefix_detection = settings.enable_prefix_detection;
        self.enable_title_generation_skip = settings.enable_title_generation_skip;
        self.enable_suggestion_mode_skip = settings.enable_suggestion_mode_skip;
        self.enable_filepath_extraction_mock = settings.enable_filepath_extraction_mock;
        self.enable_web_server_tools = settings.enable_web_server_tools;
        self.web_fetch_allow_private_networks = settings.web_fetch_allow_private_networks;
        self.web_fetch_allowed_schemes = settings.web_fetch_allowed_schemes;
        self.reasoning_replay_mode = settings.reasoning_replay_mode;
        self.transport_type = settings.transport_type;
        self.theme_mode = ThemeMode::parse(&settings.theme_mode);
        self.discovered_models = settings.discovered_models;
        self.update_model_options();
        self.update_provider_options();
        self.model_reasoning_overrides = settings.model_reasoning_overrides;
        self.model_1m_overrides = settings.model_1m_overrides;
        self.model_visibility_overrides = settings.model_visibility_overrides;
        self.real_model = settings.real_model;
        self.real_model_sonnet = settings.real_model_sonnet;
        self.real_model_opus = settings.real_model_opus;
        self.real_model_haiku = settings.real_model_haiku;
    }
}

#[cfg(test)]
mod i18n_tests;
mod update;
mod utils;

pub use utils::{compact_path, json_result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_job_result_is_rejected() {
        let mut tracker = JobTracker::default();
        let (old, _) = tracker.begin(JobKind::Saving);
        let (current, _) = tracker.begin(JobKind::Refreshing);

        assert!(!tracker.accept(old));
        assert!(tracker.accept(current));
        assert_eq!(tracker.state(), &JobState::Idle);
    }

    #[tokio::test]
    async fn starting_new_job_aborts_previous_future() {
        let mut tracker = JobTracker::default();
        let (_, old_registration) = tracker.begin(JobKind::Saving);
        let _ = tracker.begin(JobKind::Refreshing);

        let result =
            futures::future::Abortable::new(futures::future::pending::<()>(), old_registration)
                .await;
        assert!(result.is_err());
    }
}
