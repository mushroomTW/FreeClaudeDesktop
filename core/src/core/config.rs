use crate::common::local_app_data;
use crate::crypto::unprotect_secret;
use crate::error::{AppError, AppResult};
use crate::platform::atomic_file::{PendingWrite, write_transaction};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub gateway: GatewaySettings,
    pub models: ModelSettings,
    pub optimizations: OptimizationSettings,
    pub desktop: DesktopSettings,
    pub ui: UiSettings,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewaySettings {
    pub real_base_url: String,
    pub real_api_key: String,
    pub real_auth_scheme: String,
    pub transport_type: String,
    pub proxy_auth_token: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSettings {
    pub real_model: Option<String>,
    pub real_model_sonnet: Option<String>,
    pub real_model_opus: Option<String>,
    pub real_model_haiku: Option<String>,
    pub real_model_routes: HashMap<String, String>,
    pub real_model_reasoning_efforts: HashMap<String, Vec<String>>,
    pub discovered_models: Vec<String>,
    pub model_reasoning_overrides: HashMap<String, String>,
    pub model_1m_overrides: HashMap<String, bool>,
    pub model_1m_prefer_overrides: HashMap<String, bool>,
    pub model_visibility_overrides: HashMap<String, bool>,
    pub reasoning_replay_mode: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptimizationSettings {
    pub enable_quota_check_mock: bool,
    pub enable_prefix_detection: bool,
    pub enable_title_generation_skip: bool,
    pub enable_suggestion_mode_skip: bool,
    pub enable_filepath_extraction_mock: bool,
    pub enable_web_server_tools: bool,
    pub web_fetch_allowed_schemes: String,
    pub web_fetch_allow_private_networks: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopSettings {
    pub custom_claude_path: Option<String>,
    pub active_port: Option<u16>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiSettings {
    pub theme_mode: String,
    pub language: String,
}

/// 執行 `default_theme_mode` 對應的處理流程。
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
    /// 執行 `as_str` 對應的處理流程。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhTw => "zh-tw",
        }
    }

    /// 執行 `parse` 對應的處理流程。
    pub fn parse(s: &str) -> Self {
        match s {
            "zh-tw" => Self::ZhTw,
            _ => Self::En,
        }
    }

    /// 執行 `tr` 對應的處理流程。
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
                "save_launch" => "Launch ↵",
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
                "thinking_mode" => "思考模式",
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
                "save_launch" => "啟動 ↵",
                "save_only" => "僅儲存",
                "sync_from_official" => "從原版同步",
                "reset_mirror_profile" => "重置鏡像 Profile",
                "detected_claude" => "已偵測 Claude Desktop",
                "not_found_claude" => "尚未找到 Claude.exe，可使用下方自訂路徑",
                "job_cancelled" => "工作已取消",
                "sync_success" => "已從原版 Claude 重新同步設定至鏡像目錄。",
                "sync_failed" => "同步失敗",
                "reset_success" => "鏡像 Profile 目錄已重置。原版目錄完全不受影響。",
                "theme_save_failed" => "儲存佈景主題失敗",
                "load_settings_failed" => "載入設定失敗",
                "fetch_models_success" => "已更新模型列表：{} 個模型",
                "settings_written" => "設定已寫入 Claude。",
                "saving" => "儲存中...",
                "refreshing" => "抓取模型中...",
                "launching" => "啟動中...",
                "resyncing" => "同步中...",
                "restoring" => "重置中...",
                "save_failed" => "設定儲存失敗",
                "launch_failed" => "啟動失敗",
                "reset_failed" => "重置失敗",
                _ => key,
            },
        }
    }
}

/// 執行 `default_language` 對應的處理流程。
pub fn default_language() -> String {
    "en".to_string()
}

/// 執行 `default_true` 對應的處理流程。
pub fn default_true() -> bool {
    true
}

/// 執行 `default_false` 對應的處理流程。
pub fn default_false() -> bool {
    false
}

/// 執行 `default_proxy_auth_token` 對應的處理流程。
pub fn default_proxy_auth_token() -> String {
    crate::constants::PROXY_AUTH_TOKEN.to_string()
}

/// 執行 `default_web_fetch_schemes` 對應的處理流程。
fn default_web_fetch_schemes() -> String {
    "http,https".to_string()
}

/// 建立 `generate_proxy_auth_token` 所需的結果。
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

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            real_base_url: String::new(),
            real_api_key: String::new(),
            real_auth_scheme: String::new(),
            transport_type: String::new(),
            proxy_auth_token: default_proxy_auth_token(),
        }
    }
}

impl Default for OptimizationSettings {
    fn default() -> Self {
        Self {
            enable_quota_check_mock: true,
            enable_prefix_detection: true,
            enable_title_generation_skip: true,
            enable_suggestion_mode_skip: true,
            enable_filepath_extraction_mock: true,
            enable_web_server_tools: false,
            web_fetch_allowed_schemes: default_web_fetch_schemes(),
            web_fetch_allow_private_networks: false,
        }
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme_mode: default_theme_mode(),
            language: default_language(),
        }
    }
}

/// 執行 `default_auth_scheme` 對應的處理流程。
pub fn default_auth_scheme() -> String {
    "bearer".to_string()
}

/// 解析 `parse_json_text` 所需的資料。
pub fn parse_json_text(text: &str) -> serde_json::Result<Value> {
    let clean = text.trim_start_matches('\u{feff}');
    serde_json::from_str(clean)
}

/// 轉換 `to_public_config` 對應的資料格式。
pub fn to_public_config(settings: &Settings) -> Value {
    let has_key = unprotect_secret(&settings.gateway.real_api_key)
        .map(|key| !key.is_empty())
        .unwrap_or(!settings.gateway.real_api_key.is_empty());

    json!({
        "baseUrl": settings.gateway.real_base_url,
        "authScheme": if settings.gateway.real_auth_scheme.is_empty() {
            default_auth_scheme()
        } else {
            settings.gateway.real_auth_scheme.clone()
        },
        "hasApiKey": has_key,
        "transportType": settings.gateway.transport_type,
        "realModel": settings.models.real_model,
        "realModelSonnet": settings.models.real_model_sonnet,
        "realModelOpus": settings.models.real_model_opus,
        "realModelHaiku": settings.models.real_model_haiku,
        "realModelRoutes": settings.models.real_model_routes,
        "realModelReasoningEfforts": settings.models.real_model_reasoning_efforts,
        "discoveredModels": settings.models.discovered_models,
        "modelReasoningOverrides": settings.models.model_reasoning_overrides,
        "model1mOverrides": settings.models.model_1m_overrides,
        "model1mPreferOverrides": settings.models.model_1m_prefer_overrides,
        "modelVisibilityOverrides": settings.models.model_visibility_overrides,
        "reasoningReplayMode": settings.models.reasoning_replay_mode,
        "enableQuotaCheckMock": settings.optimizations.enable_quota_check_mock,
        "enablePrefixDetection": settings.optimizations.enable_prefix_detection,
        "enableTitleGenerationSkip": settings.optimizations.enable_title_generation_skip,
        "enableSuggestionModeSkip": settings.optimizations.enable_suggestion_mode_skip,
        "enableFilepathExtractionMock": settings.optimizations.enable_filepath_extraction_mock,
        "enableWebServerTools": settings.optimizations.enable_web_server_tools,
        "webFetchAllowedSchemes": settings.optimizations.web_fetch_allowed_schemes,
        "webFetchAllowPrivateNetworks": settings.optimizations.web_fetch_allow_private_networks,
        "customClaudePath": settings.desktop.custom_claude_path,
        "activePort": settings.desktop.active_port,
        "themeMode": settings.ui.theme_mode,
        "language": settings.ui.language,
    })
}

/// 執行 `settings_file` 對應的處理流程。
pub fn settings_file() -> PathBuf {
    local_app_data()
        .join("FreeClaudeDesktop")
        .join("launcher_settings.json")
}

/// 讀取 `load_launcher_settings` 所需的資料。
pub fn load_launcher_settings() -> AppResult<Option<Settings>> {
    let path = settings_file();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let value = parse_json_text(&text).map_err(AppError::InvalidConfigJson)?;
    Ok(Some(serde_json::from_value(value)?))
}

/// 讀取 `get_launcher_settings` 所需的資料。
pub fn get_launcher_settings() -> Option<Settings> {
    load_launcher_settings().ok().flatten()
}

/// 儲存 `save_launcher_settings` 所處理的資料。
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
    /// 驗證 `removed_feature_field_is_not_serialized` 的行為符合預期。
    fn removed_feature_field_is_rejected() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["enableComputerMcpServer"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<Settings>(value).is_err());
    }

    #[test]
    /// 舊版扁平設定格式不得被當成有效的新設定。
    fn flat_settings_format_is_rejected() {
        let value = serde_json::json!({
            "realBaseUrl": "https://example.com",
            "realApiKey": "fallback:test",
            "realAuthScheme": "bearer",
            "proxyAuthToken": "token"
        });
        assert!(serde_json::from_value::<Settings>(value).is_err());
    }
}
