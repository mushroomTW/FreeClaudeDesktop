use crate::common::local_app_data;
use crate::crypto::unprotect_secret;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub real_base_url: String,
    pub real_api_key: String,
    pub real_auth_scheme: String,
    pub real_model: Option<String>,
    #[serde(default)]
    pub real_model_sonnet: Option<String>,
    #[serde(default)]
    pub real_model_opus: Option<String>,
    #[serde(default)]
    pub real_model_haiku: Option<String>,
    pub real_model_routes: HashMap<String, String>,
    #[serde(default)]
    pub real_model_reasoning_efforts: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub discovered_models: Vec<String>,
    #[serde(default)]
    pub model_reasoning_overrides: HashMap<String, String>,
    #[serde(default)]
    pub model_1m_overrides: HashMap<String, bool>,
    /// 寫入 Claude Desktop gateway config 的本機 proxy token
    #[serde(default = "default_proxy_auth_token")]
    pub proxy_auth_token: String,
    /// 當前啟用的代理埠號
    #[serde(default)]
    pub active_port: Option<u16>,
    /// 上游通訊協定類型：
    /// - "openai_chat": 需要將 Anthropic 請求轉換為 OpenAI Chat Completions 格式（預設）
    /// - "anthropic_messages": 原生 Anthropic Messages API，直接 passthrough
    #[serde(default)]
    pub transport_type: String,
    /// 推理/思考塊重播模式：
    /// - "disabled": 丟棄所有 thinking 內容
    /// - "think_tags": 將 thinking 包裝在 <thinking>...</thinking> 標籤中（預設）
    /// - "reasoning_content": 使用 reasoning_content 欄位（若 provider 支援）
    #[serde(default)]
    pub reasoning_replay_mode: String,
    /// 是否啟用配額檢查模擬（攔截 max_tokens=1 且包含 "quota" 的請求）
    #[serde(default = "default_true")]
    pub enable_quota_check_mock: bool,
    /// 是否啟用快速前綴檢測（解析 shell 命令前綴，避免呼叫 LLM）
    #[serde(default = "default_true")]
    pub enable_prefix_detection: bool,
    /// 是否啟用標題生成跳過（回傳固定標題 "Conversation"）
    #[serde(default = "default_true")]
    pub enable_title_generation_skip: bool,
    /// 是否啟用建議模式跳過（回傳空建議）
    #[serde(default = "default_true")]
    pub enable_suggestion_mode_skip: bool,
    /// 是否啟用檔案路徑提取模擬（從命令輸出中本地提取檔案路徑）
    #[serde(default = "default_true")]
    pub enable_filepath_extraction_mock: bool,
    /// 是否啟用 Web 工具攔截（本地執行 web_search/web_fetch）
    #[serde(default = "default_false")]
    pub enable_web_server_tools: bool,
    /// 是否寫入本機 computer MCP server 設定
    #[serde(default = "default_false")]
    pub enable_computer_mcp_server: bool,
    /// web_fetch 允許的 URL 方案清單（逗號分隔，如 "http,https"）
    #[serde(default = "default_web_fetch_schemes")]
    pub web_fetch_allowed_schemes: String,
    /// 是否允許 web_fetch 存取私有網路目標
    #[serde(default = "default_false")]
    pub web_fetch_allow_private_networks: bool,
    /// 主題模式 ("light", "dark", "system")
    #[serde(default = "default_theme_mode")]
    pub theme_mode: String,
}

pub fn default_theme_mode() -> String {
    "light".to_string()
}

pub fn default_true() -> bool {
    true
}

pub fn default_false() -> bool {
    false
}

pub fn default_proxy_auth_token() -> String {
    crate::constants::PROXY_AUTH_TOKEN.to_string()
}

fn default_web_fetch_schemes() -> String {
    "http,https".to_string()
}

pub fn generate_proxy_auth_token() -> AppResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| AppError::Crypto(error.to_string()))?;

    let mut token = String::with_capacity(4 + bytes.len() * 2);
    token.push_str("fcl_");
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            real_base_url: String::new(),
            real_api_key: String::new(),
            real_auth_scheme: String::new(),
            real_model: None,
            real_model_sonnet: None,
            real_model_opus: None,
            real_model_haiku: None,
            real_model_routes: HashMap::new(),
            real_model_reasoning_efforts: HashMap::new(),
            discovered_models: Vec::new(),
            model_reasoning_overrides: HashMap::new(),
            model_1m_overrides: HashMap::new(),
            proxy_auth_token: default_proxy_auth_token(),
            active_port: None,
            transport_type: String::new(),
            reasoning_replay_mode: String::new(),
            enable_quota_check_mock: true,
            enable_prefix_detection: true,
            enable_title_generation_skip: true,
            enable_suggestion_mode_skip: true,
            enable_filepath_extraction_mock: true,
            enable_web_server_tools: false,
            enable_computer_mcp_server: false,
            web_fetch_allowed_schemes: "http,https".to_string(),
            web_fetch_allow_private_networks: false,
            theme_mode: default_theme_mode(),
        }
    }
}

struct PublicConfig {
    base_url: String,
    auth_scheme: String,
    has_api_key: bool,
}

pub fn default_auth_scheme() -> String {
    "bearer".to_string()
}

pub fn parse_json_text(text: &str) -> serde_json::Result<Value> {
    let clean = text.trim_start_matches('\u{feff}');
    serde_json::from_str(clean)
}

pub fn to_public_config(settings: &Settings) -> Value {
    let has_key = unprotect_secret(&settings.real_api_key)
        .map(|key| !key.is_empty())
        .unwrap_or(!settings.real_api_key.is_empty());

    json!(PublicConfig {
        base_url: settings.real_base_url.clone(),
        auth_scheme: if settings.real_auth_scheme.is_empty() {
            default_auth_scheme()
        } else {
            settings.real_auth_scheme.clone()
        },
        has_api_key: has_key,
    })
}

impl Serialize for PublicConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("PublicConfig", 3)?;
        state.serialize_field("baseUrl", &self.base_url)?;
        state.serialize_field("authScheme", &self.auth_scheme)?;
        state.serialize_field("hasApiKey", &self.has_api_key)?;
        state.end()
    }
}

pub fn settings_file() -> PathBuf {
    local_app_data()
        .join("FreeClaudeLauncher")
        .join("launcher_settings.json")
}

fn legacy_settings_file() -> PathBuf {
    local_app_data()
        .join("Claude-3p")
        .join("launcher_settings.json")
}

fn migrate_legacy_settings() {
    let legacy = legacy_settings_file();
    let new_file = settings_file();
    if legacy.exists() && !new_file.exists() {
        if let Some(parent) = new_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::copy(&legacy, &new_file).is_ok() {
            let _ = fs::remove_file(legacy);
        }
    }
}

pub fn get_launcher_settings() -> Option<Settings> {
    migrate_legacy_settings();
    let path = settings_file();
    if !path.exists() {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let value = parse_json_text(&text).ok()?;
    serde_json::from_value(value).ok()
}

pub fn save_launcher_settings(settings: &Settings) -> AppResult<()> {
    let path = settings_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(settings)?;
    fs::write(&path, serialized)?;

    let legacy = legacy_settings_file();
    if legacy.exists() && legacy != path {
        let _ = fs::remove_file(legacy);
    }
    Ok(())
}
