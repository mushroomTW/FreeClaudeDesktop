use crate::common::local_app_data;
use crate::crypto::unprotect_secret;
use crate::error::{AppError, AppResult};
use crate::platform::atomic_file::{PendingWrite, write_transaction};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    /// 是否在 Claude Desktop 顯示各上游模型；未設定時預設顯示。
    #[serde(default)]
    pub model_visibility_overrides: HashMap<String, bool>,
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
    /// web_fetch 允許的 URL 方案清單（逗號分隔，如 "http,https"）
    #[serde(default = "default_web_fetch_schemes")]
    pub web_fetch_allowed_schemes: String,
    /// 是否允許 web_fetch 存取私有網路目標
    #[serde(default = "default_false")]
    pub web_fetch_allow_private_networks: bool,
    /// 主題模式 ("light", "dark", "system")
    #[serde(default = "default_theme_mode")]
    pub theme_mode: String,
    /// 語言設定 ("en", "zh-tw")
    #[serde(default = "default_language")]
    pub language: String,
}

pub fn default_theme_mode() -> String {
    "light".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-tw")]
    ZhTw,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhTw => "zh-tw",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "zh-tw" => Self::ZhTw,
            _ => Self::En,
        }
    }

    pub fn tr(&self, key: &'static str) -> &'static str {
        match self {
            Self::En => match key {
                "title" => "FreeClaudeDesktop",
                "connection_settings" => "Connection Settings",
                "api_provider" => "API Provider",
                "select_provider" => "Select provider...",
                "api_url" => "API URL",
                "api_key" => "API Key",
                "auth_scheme" => "Auth Scheme",
                "use_custom_path" => "Use custom Claude.exe path",
                "models_thinking" => "Models & Thinking",
                "fetch_model_list" => "Fetch Models",
                "model_reasoning_limit" => "Model Reasoning Limit",
                "model_override_tip" => "These settings override Claude Desktop model routing.",
                "no_models_fetched" => "No models fetched yet. Save settings to see models.",
                "show" => "Show",
                "context_1m" => "1M Context",
                "opus_model" => "Opus Model",
                "auto_alias" => "(Auto/Dynamic)",
                "sonnet_model" => "Sonnet Model",
                "haiku_model" => "Haiku Model",
                "fallback_model" => "Fallback Model",
                "transport_protocol" => "Transport Protocol",
                "thinking_mode" => "Thinking Mode",
                "extensions_skills" => "Extensions & Skills",
                "web_tool_intercept" => "Web Tool Intercept (web_search / web_fetch)",
                "allow_private_network" => "Allow private networks for web_fetch",
                "allowed_url_schemes" => "Allowed URL Schemes",
                "optimizations" => "Optimizations",
                "quota_check_mock" => "Quota Check Mock",
                "prefix_detection" => "Command Prefix Quick Detection",
                "title_generation_skip" => "Skip Title Generation",
                "suggestion_mode_skip" => "Skip Suggestion Mode",
                "filepath_extraction_mock" => "Filepath Extraction Mock",
                "settings_menu" => "Settings",
                "local_proxy" => "Local Proxy: 127.0.0.1:",
                "detecting" => "Detecting...",
                "reset_confirm_msg" => {
                    "Are you sure you want to reset the mirror Profile? Official profile will not be affected."
                }
                "confirm_reset" => "Confirm Reset",
                "cancel" => "Cancel",
                "save_launch" => "Save & Launch ↵",
                "save_only" => "Save Only",
                "sync_from_official" => "Sync from Official",
                "reset_mirror_profile" => "Reset Mirror",
                "detected_claude" => "Claude Desktop Detected",
                "not_found_claude" => "Claude.exe not found. You can set custom path below.",
                "job_cancelled" => "Job cancelled",
                "sync_success" => "Settings synced from official Claude.",
                "sync_failed" => "Sync failed",
                "reset_success" => {
                    "Mirror Profile directory has been reset. Official directory is unaffected."
                }
                "theme_save_failed" => "Failed to save theme",
                "load_settings_failed" => "Failed to load settings",
                "fetch_models_success" => "Model list updated: {} models",
                "settings_written" => "Settings written to Claude.",
                "saving" => "Saving...",
                "refreshing" => "Refreshing...",
                "launching" => "Launching...",
                "resyncing" => "Syncing...",
                "restoring" => "Resetting...",
                "save_failed" => "Save failed",
                "launch_failed" => "Launch failed",
                "reset_failed" => "Reset failed",
                _ => key,
            },
            Self::ZhTw => match key {
                "connection_settings" => "連線設定",
                "api_provider" => "API 供應商",
                "select_provider" => "選擇供應商...",
                "api_url" => "API URL",
                "api_key" => "API Key",
                "auth_scheme" => "驗證方式",
                "use_custom_path" => "使用自訂 Claude.exe 路徑",
                "models_thinking" => "模型與思考",
                "fetch_model_list" => "抓模型列表",
                "model_reasoning_limit" => "模型思考上限",
                "model_override_tip" => "這裡的設定會覆寫本專案的 Claude Desktop 模型路由。",
                "no_models_fetched" => "尚未抓到模型；儲存設定後會列出可設定的模型。",
                "show" => "顯示",
                "context_1m" => "1M 上下文",
                "opus_model" => "Opus 模型",
                "auto_alias" => "(自動/動態別名)",
                "sonnet_model" => "Sonnet 模型",
                "haiku_model" => "Haiku 模型",
                "fallback_model" => "預設保底模型",
                "transport_protocol" => "傳輸協定",
                "thinking_mode" => "Thinking 模式",
                "extensions_skills" => "擴充與技能",
                "web_tool_intercept" => "Web 工具攔截 (本地執行 web_search / web_fetch)",
                "allow_private_network" => "允許 web_fetch 存取私有網路目標",
                "allowed_url_schemes" => "允許的 URL 方案",
                "optimizations" => "效能優化",
                "quota_check_mock" => "配額檢查攔截",
                "prefix_detection" => "命令前綴快速檢測",
                "title_generation_skip" => "標題生成跳過",
                "suggestion_mode_skip" => "建議模式跳過",
                "filepath_extraction_mock" => "檔案路徑提取模擬",
                "settings_menu" => "設定",
                "local_proxy" => "本機 Proxy：127.0.0.1：",
                "detecting" => "正在偵測...",
                "title" => "FreeClaudeDesktop",
                "reset_confirm_msg" => "⚠ 確定要重置鏡像 Profile 目錄？原版目錄完全不受影響。",
                "confirm_reset" => "確定重置",
                "cancel" => "取消",
                "save_launch" => "儲存並啟動 ↵",
                "save_only" => "僅儲存",
                "sync_from_official" => "從原版同步",
                "reset_mirror_profile" => "重置鏡像 Profile",
                "detected_claude" => "已偵測 Claude Desktop",
                "not_found_claude" => "尚未找到 Claude.exe，可使用下方自訂路徑",
                "job_cancelled" => "工作已取消",
                "sync_success" => "已從原版 Claude 重新同步設定至鏡像目錄。",
                "sync_failed" => "同步",
                "reset_success" => "鏡像 Profile 目錄已重置。原版目錄完全不受影響。",
                "theme_save_failed" => "儲存佈景主題失敗",
                "load_settings_failed" => "載入設定失敗",
                "fetch_models_success" => "已更新模型列表：{} 個模型",
                "settings_written" => "設定已寫入 Claude。",
                "saving" => "儲存",
                "refreshing" => "抓取模型",
                "launching" => "啟動",
                "resyncing" => "同步",
                "restoring" => "重置",
                "save_failed" => "設定儲存",
                "launch_failed" => "啟動",
                "reset_failed" => "重置",
                _ => key,
            },
        }
    }
}

pub fn default_language() -> String {
    "en".to_string()
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
            model_visibility_overrides: HashMap::new(),
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
            web_fetch_allowed_schemes: "http,https".to_string(),
            web_fetch_allow_private_networks: false,
            theme_mode: default_theme_mode(),
            language: default_language(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
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

pub fn settings_file() -> PathBuf {
    local_app_data()
        .join("FreeClaudeDesktop")
        .join("launcher_settings.json")
}

pub fn load_launcher_settings() -> AppResult<Option<Settings>> {
    let path = settings_file();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let value = parse_json_text(&text).map_err(AppError::InvalidConfigJson)?;
    Ok(Some(serde_json::from_value(value)?))
}

pub fn get_launcher_settings() -> Option<Settings> {
    load_launcher_settings().ok().flatten()
}

pub fn save_launcher_settings(settings: &Settings) -> AppResult<()> {
    let path = settings_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(settings)?;
    write_transaction(vec![PendingWrite::new(
        path.clone(),
        serialized.into_bytes(),
    )])?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_feature_field_is_not_serialized() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["enableComputerMcpServer"] = serde_json::Value::Bool(true);
        let settings: Settings = serde_json::from_value(value).unwrap();
        let saved = serde_json::to_value(settings).unwrap();
        assert!(saved.get("enableComputerMcpServer").is_none());
    }
}
