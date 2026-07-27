use serde_json::{Value, json};
use std::env;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::platform::atomic_file::{PendingWrite, write_transaction};

/// 執行 `claude_home_dir` 對應的處理流程。
pub fn claude_home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// 執行 `claude_settings_json_path` 對應的處理流程。
pub fn claude_settings_json_path() -> PathBuf {
    claude_home_dir().join(".claude").join("settings.json")
}

const MANAGED_CLAUDE_ENV_KEYS: [&str; 3] = [
    "ANTHROPIC_BASE_URL",
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_ENABLE_AUTO_MODE",
];
pub(crate) const PREVIOUS_CLAUDE_SETTINGS_KEY: &str = "freeClaudeDesktopPreviousSettings";

/// 執行 `previous_setting_entry` 對應的處理流程。
fn previous_setting_entry(value: Option<&Value>) -> Value {
    json!({
        "present": value.is_some(),
        "value": value.cloned().unwrap_or(Value::Null)
    })
}

/// 清理或還原 `restore_previous_setting` 所管理的資料。
fn restore_previous_setting(obj: &mut serde_json::Map<String, Value>, key: &str, previous: &Value) {
    if previous
        .get("present")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        obj.insert(
            key.to_string(),
            previous.get("value").cloned().unwrap_or(Value::Null),
        );
    } else {
        obj.remove(key);
    }
}

/// 轉換或更新 `apply_anthropic_base_url_env` 所處理的內容。
pub fn apply_anthropic_base_url_env(port: u16) -> AppResult<()> {
    let path = claude_settings_json_path();
    let mut data: Value = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).map_err(AppError::InvalidConfigJson)?
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        json!({})
    };

    if let Some(obj) = data.as_object_mut() {
        if !obj.contains_key(PREVIOUS_CLAUDE_SETTINGS_KEY) {
            let env_previous = obj
                .get("env")
                .and_then(Value::as_object)
                .map(|env| {
                    MANAGED_CLAUDE_ENV_KEYS
                        .iter()
                        .map(|key| (key.to_string(), previous_setting_entry(env.get(*key))))
                        .collect::<serde_json::Map<_, _>>()
                })
                .unwrap_or_else(|| {
                    MANAGED_CLAUDE_ENV_KEYS
                        .iter()
                        .map(|key| (key.to_string(), previous_setting_entry(None)))
                        .collect()
                });
            obj.insert(
                PREVIOUS_CLAUDE_SETTINGS_KEY.to_string(),
                json!({
                    "autoModeEnabled": previous_setting_entry(obj.get("autoModeEnabled")),
                    "envPresent": obj.get("env").is_some(),
                    "env": env_previous
                }),
            );
        }

        if obj.get("autoModeEnabled").is_none() {
            obj.insert("autoModeEnabled".to_string(), Value::Bool(true));
        }
        if obj.get("env").is_none() {
            obj.insert("env".to_string(), json!({}));
        }
    }

    if let Some(env_obj) = data.get_mut("env").and_then(Value::as_object_mut) {
        env_obj.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(format!("http://127.0.0.1:{}", port)),
        );
        env_obj.insert(
            "ENABLE_TOOL_SEARCH".to_string(),
            Value::String("true".to_string()),
        );
        env_obj.insert(
            "CLAUDE_CODE_ENABLE_AUTO_MODE".to_string(),
            Value::String("1".to_string()),
        );
    }

    let content = serde_json::to_string_pretty(&data)?;
    write_transaction(vec![PendingWrite::new(path, content.into_bytes())])?;
    Ok(())
}

/// 清理或還原 `remove_anthropic_base_url_env` 所管理的資料。
pub fn remove_anthropic_base_url_env() -> AppResult<()> {
    let path = claude_settings_json_path();
    if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        let mut data = serde_json::from_str::<Value>(&text).map_err(AppError::InvalidConfigJson)?;
        let mut changed = false;
        if let Some(obj) = data.as_object_mut() {
            if let Some(previous) = obj.remove(PREVIOUS_CLAUDE_SETTINGS_KEY) {
                changed = true;
                if let Some(auto_mode) = previous.get("autoModeEnabled") {
                    restore_previous_setting(obj, "autoModeEnabled", auto_mode);
                }

                let env_present = previous
                    .get("envPresent")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                if obj.get("env").is_none() {
                    obj.insert("env".to_string(), json!({}));
                }
                if let Some(env_obj) = obj.get_mut("env").and_then(Value::as_object_mut) {
                    if let Some(previous_env) = previous.get("env").and_then(Value::as_object) {
                        for key in MANAGED_CLAUDE_ENV_KEYS {
                            if let Some(previous_value) = previous_env.get(key) {
                                restore_previous_setting(env_obj, key, previous_value);
                            } else {
                                env_obj.remove(key);
                            }
                        }
                    }
                    if env_obj.is_empty() && !env_present {
                        obj.remove("env");
                    }
                }
            } else if let Some(env_obj) = obj.get_mut("env").and_then(Value::as_object_mut) {
                for key in MANAGED_CLAUDE_ENV_KEYS {
                    if env_obj.remove(key).is_some() {
                        changed = true;
                    }
                }
            }
        }
        if changed {
            let content = serde_json::to_string_pretty(&data)?;
            write_transaction(vec![PendingWrite::new(path, content.into_bytes())])?;
        }
    }
    Ok(())
}
