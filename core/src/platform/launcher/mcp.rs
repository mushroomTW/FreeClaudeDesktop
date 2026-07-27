use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::platform::atomic_file::{PendingWrite, write_transaction};

use super::{mirror_profile_dir, official_app_data_dir};

/// 執行 `app_data_roaming_dir` 對應的處理流程。
pub fn app_data_roaming_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME")
            .map(|p| PathBuf::from(p).join("Library").join("Application Support"))
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        env::var_os("HOME")
            .map(|p| PathBuf::from(p).join(".config"))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// 執行 `mcp_config_paths` 對應的處理流程。
pub fn mcp_config_paths() -> Vec<PathBuf> {
    vec![mirror_profile_dir().join("claude_desktop_config.json")]
}

/// 正規化 `clean_json_text` 所處理的資料。
pub fn clean_json_text(input: &str) -> String {
    let text = input.strip_prefix("\u{feff}").unwrap_or(input);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut is_escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if is_escaped {
                is_escaped = false;
            } else if ch == '\\' {
                is_escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }

        if ch == '/' {
            if let Some(&'/') = chars.peek() {
                chars.next();
                while let Some(&next_ch) = chars.peek() {
                    if next_ch == '\n' || next_ch == '\r' {
                        break;
                    }
                    chars.next();
                }
                continue;
            } else if let Some(&'*') = chars.peek() {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '*'
                        && let Some(&'/') = chars.peek()
                    {
                        chars.next();
                        break;
                    }
                }
                continue;
            }
        }

        out.push(ch);
    }

    let mut cleaned = String::with_capacity(out.len());
    let mut out_chars = out.chars().peekable();
    in_string = false;
    is_escaped = false;

    while let Some(ch) = out_chars.next() {
        if in_string {
            cleaned.push(ch);
            if is_escaped {
                is_escaped = false;
            } else if ch == '\\' {
                is_escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            cleaned.push(ch);
            continue;
        }

        if ch == ',' {
            let temp_chars = out_chars.clone();
            let mut trailing = false;
            for next_c in temp_chars {
                if next_c.is_whitespace() {
                    continue;
                }
                if next_c == '}' || next_c == ']' {
                    trailing = true;
                }
                break;
            }
            if trailing {
                continue;
            }
        }

        cleaned.push(ch);
    }

    cleaned
}

/// 讀取 `read_json_config` 所需的資料。
pub fn read_json_config(path: &Path) -> Option<Value> {
    read_json_config_result(path).ok().flatten()
}

/// 讀取 `read_json_config_result` 所需的資料。
fn read_json_config_result(path: &Path) -> AppResult<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let cleaned = clean_json_text(&text);
    let value = serde_json::from_str(&cleaned).map_err(AppError::InvalidConfigJson)?;
    Ok(Some(value))
}

/// 執行 `collect_all_mcp_servers` 對應的處理流程。
pub fn collect_all_mcp_servers() -> serde_json::Map<String, Value> {
    collect_all_mcp_servers_result().unwrap_or_default()
}

/// 執行 `collect_all_mcp_servers_result` 對應的處理流程。
pub fn collect_all_mcp_servers_result() -> AppResult<serde_json::Map<String, Value>> {
    let mut merged = serde_json::Map::new();
    let mut search_paths = mcp_config_paths();
    search_paths.push(official_app_data_dir().join("claude_desktop_config.json"));

    for path in search_paths {
        if let Some(data) = read_json_config_result(&path)?
            && let Some(servers) = data.get("mcpServers").and_then(Value::as_object)
        {
            for (k, v) in servers {
                if !merged.contains_key(k) {
                    merged.insert(k.clone(), v.clone());
                }
            }
        }
    }
    Ok(merged)
}

/// 建立 `merge_mcp_servers` 所需的結果。
pub fn merge_mcp_servers(mut data: Value, all_servers: &serde_json::Map<String, Value>) -> Value {
    if !data.is_object() {
        data = json!({});
    }
    if !all_servers.is_empty()
        && let Some(obj) = data.as_object_mut()
    {
        let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
        if !servers.is_object() {
            *servers = json!({});
        }
        if let Some(servers_obj) = servers.as_object_mut() {
            for (k, v) in all_servers {
                if !servers_obj.contains_key(k) {
                    servers_obj.insert(k.clone(), v.clone());
                }
            }
        }
    }
    data
}

/// 正規化 `strip_removed_computer_mcp` 所處理的資料。
pub(crate) fn strip_removed_computer_mcp(mut data: Value) -> Value {
    let Some(root) = data.as_object_mut() else {
        return data;
    };
    if let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.remove("free-claude-computer");
        servers.remove("launcher-computer");
        if servers.is_empty() {
            root.remove("mcpServers");
        }
    }
    data
}

const PREVIOUS_DEPLOYMENT_MODE_KEY: &str = "freeClaudeDesktopPreviousDeploymentMode";

/// 轉換或更新 `apply_managed_deployment_mode` 所處理的內容。
pub fn apply_managed_deployment_mode(mut data: Value) -> Value {
    if !data.is_object() {
        data = json!({});
    }
    if let Some(obj) = data.as_object_mut() {
        if !obj.contains_key(PREVIOUS_DEPLOYMENT_MODE_KEY) {
            let previous = obj.get("deploymentMode").cloned().unwrap_or(Value::Null);
            obj.insert(PREVIOUS_DEPLOYMENT_MODE_KEY.to_string(), previous);
        }
        obj.insert(
            "deploymentMode".to_string(),
            Value::String("3p".to_string()),
        );
    }
    data
}

/// 清理或還原 `restore_managed_deployment_mode` 所管理的資料。
pub fn restore_managed_deployment_mode(mut data: Value) -> Value {
    if let Some(obj) = data.as_object_mut() {
        if let Some(previous) = obj.remove(PREVIOUS_DEPLOYMENT_MODE_KEY) {
            if previous.is_null() {
                obj.remove("deploymentMode");
            } else {
                obj.insert("deploymentMode".to_string(), previous);
            }
        } else if obj.get("deploymentMode").and_then(Value::as_str) == Some("3p") {
            obj.insert(
                "deploymentMode".to_string(),
                Value::String("1p".to_string()),
            );
        }
    }
    data
}

/// 轉換或更新 `apply_3p_deployment_mode` 所處理的內容。
pub fn apply_3p_deployment_mode() -> AppResult<()> {
    let all_mcp_servers = collect_all_mcp_servers_result()?;
    let mut writes = Vec::new();

    for path in mcp_config_paths() {
        let data_opt = if path.exists() {
            read_json_config_result(&path)?
        } else {
            let parent_exists = path.parent().map(|p| p.exists()).unwrap_or(false);
            if !parent_exists && all_mcp_servers.is_empty() {
                continue;
            }
            Some(json!({}))
        };

        let data = data_opt.unwrap_or_else(|| json!({}));
        let data = merge_mcp_servers(data, &all_mcp_servers);
        let data = strip_removed_computer_mcp(apply_managed_deployment_mode(data));

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&data)?;
        writes.push(PendingWrite::new(path, content.into_bytes()));
    }
    write_transaction(writes)?;
    Ok(())
}

/// 清理或還原 `restore_1p_deployment_mode` 所管理的資料。
pub fn restore_1p_deployment_mode() -> AppResult<()> {
    let all_mcp_servers = collect_all_mcp_servers_result()?;
    let mut writes = Vec::new();

    for path in mcp_config_paths() {
        if path.exists()
            && let Some(data) = read_json_config_result(&path)?
        {
            let data = merge_mcp_servers(data, &all_mcp_servers);
            let content = serde_json::to_string_pretty(&restore_managed_deployment_mode(data))?;
            writes.push(PendingWrite::new(path, content.into_bytes()));
        }
    }
    write_transaction(writes)?;
    Ok(())
}
