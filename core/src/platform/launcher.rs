use serde_json::{Value, json};
use std::env;
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::local_app_data;
use crate::error::{AppError, AppResult};
use crate::platform::atomic_file::{PendingWrite, write_transaction};

mod profile;

pub use profile::{
    copy_dir_all, ensure_mirror_profile_initialized, mirror_profile_dir, official_app_data_dir,
    reset_mirror_profile, resync_from_official,
};

/// 執行 `mirror_profile_dir` 對應的處理流程。
/// 執行 `official_app_data_dir` 對應的處理流程。
/// 建立 `copy_dir_all` 所需的結果。
/// 執行 `ensure_mirror_profile_initialized` 對應的處理流程。
/// 轉換或更新 `resync_from_official` 所處理的內容。
/// 清理或還原 `reset_mirror_profile` 所管理的資料。
/// 執行 `user_data_dir` 對應的處理流程。
pub fn user_data_dir() -> PathBuf {
    if let Ok(dir) = env::var("CLAUDE_USER_DATA_DIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    mirror_profile_dir()
}

/// 執行 `config_lib_dir` 對應的處理流程。
pub fn config_lib_dir() -> PathBuf {
    user_data_dir().join("configLibrary")
}

/// 執行 `meta_file` 對應的處理流程。
pub fn meta_file() -> PathBuf {
    config_lib_dir().join("_meta.json")
}

#[cfg(target_os = "windows")]
/// 執行 `known_claude_paths` 對應的處理流程。
pub fn known_claude_paths() -> Vec<PathBuf> {
    let local = local_app_data();
    let program_files =
        PathBuf::from(env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string()));
    let mut paths = vec![
        local
            .join("Programs")
            .join("claude-desktop")
            .join("Claude.exe"),
        local.join("Programs").join("Claude").join("Claude.exe"),
        program_files.join("Claude").join("Claude.exe"),
        PathBuf::from(
            env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string()),
        )
        .join("Claude")
        .join("Claude.exe"),
    ];

    // 探測 WindowsApps 下的包目錄，例如 C:\Program Files\WindowsApps\Claude_*\app\Claude.exe。
    let windows_apps = program_files.join("WindowsApps");
    if let Ok(entries) = fs::read_dir(&windows_apps) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.contains("claude") {
                let exe1 = entry.path().join("app").join("Claude.exe");
                let exe2 = entry.path().join("Claude.exe");
                if exe1.exists() {
                    paths.push(exe1);
                }
                if exe2.exists() {
                    paths.push(exe2);
                }
            }
        }
    }

    paths
}

#[cfg(target_os = "macos")]
/// 執行 `known_claude_paths` 對應的處理流程。
pub fn known_claude_paths() -> Vec<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"));
    vec![
        PathBuf::from("/Applications/Claude.app/Contents/MacOS/Claude"),
        home.join("Applications")
            .join("Claude.app")
            .join("Contents")
            .join("MacOS")
            .join("Claude"),
    ]
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
/// 執行 `known_claude_paths` 對應的處理流程。
pub fn known_claude_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/bin/claude-desktop"),
        PathBuf::from("/usr/local/bin/claude-desktop"),
        PathBuf::from("/usr/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
}

/// 驗證 `validate_launch_path` 所需的條件。
pub fn validate_launch_path(target_path: &Path) -> AppResult<PathBuf> {
    if target_path.as_os_str().is_empty() {
        return Err(AppError::Launcher(
            "Claude executable path is required".to_string(),
        ));
    }
    if !target_path.is_absolute() {
        return Err(AppError::Launcher(
            "Claude executable path must be absolute".to_string(),
        ));
    }
    #[cfg(target_os = "windows")]
    {
        if target_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("exe"))
            != Some(true)
        {
            return Err(AppError::Launcher(
                "Claude executable path must end with .exe".to_string(),
            ));
        }
    }
    Ok(target_path.to_path_buf())
}

/// 儲存 `write_config_to_all_paths` 所處理的資料。
pub fn write_config_to_all_paths(file_name: &str, content: &str) -> AppResult<()> {
    let mut writes = Vec::new();
    for dir in config_library_dirs() {
        fs::create_dir_all(&dir)?;
        writes.push(PendingWrite::new(
            dir.join(file_name),
            content.as_bytes().to_vec(),
        ));
    }
    write_transaction(writes)?;
    Ok(())
}

/// 執行 `config_library_dirs` 對應的處理流程。
fn config_library_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    let mut dirs = vec![config_lib_dir()];
    #[cfg(not(target_os = "windows"))]
    let dirs = vec![config_lib_dir()];
    #[cfg(target_os = "windows")]
    {
        dirs.push(local_app_data().join("Claude-3p").join("configLibrary"));
        // Windows Store 版 Claude 無法吃 --user-data-dir；ClaudeSource 會固定讀這裡的 3P profile。
        dirs.push(
            app_data_roaming_dir()
                .join("Claude-3p")
                .join("configLibrary"),
        );

        let packages_dir = env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("AppData")
            .join("Local")
            .join("Packages");
        if let Ok(entries) = fs::read_dir(packages_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.contains("claude") {
                    dirs.push(
                        entry
                            .path()
                            .join("LocalCache")
                            .join("Local")
                            .join("Claude-3p")
                            .join("configLibrary"),
                    );
                }
            }
        }
    }
    dirs
}

/// 執行 `upsert_managed_meta_entry` 對應的處理流程。
pub fn upsert_managed_meta_entry(mut meta: Value) -> Value {
    if !meta.is_object() {
        meta = json!({});
    }
    let obj = meta.as_object_mut().unwrap();
    obj.insert(
        "appliedId".to_string(),
        Value::String(CONFIG_ID.to_string()),
    );
    let entries = obj
        .entry("entries")
        .or_insert_with(|| json!([]))
        .as_array_mut();

    if let Some(entries) = entries {
        entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(CONFIG_ID));
        entries.push(json!({ "id": CONFIG_ID, "name": "FreeClaudeDesktop" }));
    } else {
        obj.insert(
            "entries".to_string(),
            json!([{ "id": CONFIG_ID, "name": "FreeClaudeDesktop" }]),
        );
    }
    meta
}

/// 清理或還原 `remove_managed_meta_entry` 所管理的資料。
pub fn remove_managed_meta_entry(mut meta: Value) -> Value {
    if let Some(obj) = meta.as_object_mut() {
        let was_applied = obj.get("appliedId").and_then(Value::as_str) == Some(CONFIG_ID);
        let mut next_applied_id = None;
        if let Some(entries) = obj.get_mut("entries").and_then(Value::as_array_mut) {
            entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(CONFIG_ID));
            next_applied_id = entries
                .first()
                .and_then(|entry| entry.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if was_applied {
            if let Some(next_id) = next_applied_id {
                obj.insert("appliedId".to_string(), Value::String(next_id));
            } else {
                obj.remove("appliedId");
            }
        }
    }
    meta
}

/// 儲存 `write_managed_meta_to_all_paths` 所處理的資料。
pub fn write_managed_meta_to_all_paths() -> AppResult<()> {
    let mut writes = Vec::new();
    for dir in config_library_dirs() {
        fs::create_dir_all(&dir)?;
        let path = dir.join("_meta.json");
        let meta = if path.exists() {
            let text = fs::read_to_string(&path)?;
            serde_json::from_str::<Value>(&text).map_err(AppError::InvalidConfigJson)?
        } else {
            json!({})
        };
        let content = serde_json::to_string_pretty(&upsert_managed_meta_entry(meta))?;
        writes.push(PendingWrite::new(path, content.into_bytes()));
    }
    write_transaction(writes)?;
    Ok(())
}

/// 清理或還原 `remove_managed_config_from_all_paths` 所管理的資料。
fn remove_managed_config_from_all_paths() -> AppResult<()> {
    let mut writes = Vec::new();
    let mut configs = Vec::new();
    for dir in config_library_dirs() {
        configs.push(dir.join(format!("{CONFIG_ID}.json")));
        let meta_path = dir.join("_meta.json");
        if meta_path.exists() {
            let text = fs::read_to_string(&meta_path)?;
            let meta = serde_json::from_str::<Value>(&text).map_err(AppError::InvalidConfigJson)?;
            let content = serde_json::to_string_pretty(&remove_managed_meta_entry(meta))?;
            writes.push(PendingWrite::new(meta_path, content.into_bytes()));
        }
    }
    write_transaction(writes)?;
    for path in configs {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
/// 執行 `powershell_output` 對應的處理流程。
fn powershell_output(script: &str) -> Option<String> {
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", script]);
    cmd.creation_flags(crate::constants::CREATE_NO_WINDOW);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(target_os = "windows")]
/// 讀取 `get_claude_appx_package_family_name` 所需的資料。
fn get_claude_appx_package_family_name() -> Option<String> {
    powershell_output(
        "Get-AppxPackage -Name *Claude* | Select-Object -ExpandProperty PackageFamilyName",
    )
}

#[cfg(target_os = "windows")]
/// 讀取 `get_claude_appx_application_id` 所需的資料。
fn get_claude_appx_application_id() -> String {
    powershell_output("$app = Get-AppxPackage -Name *Claude*; if ($app) { $manifestPath = Join-Path $app.InstallLocation 'AppxManifest.xml'; if (Test-Path $manifestPath) { [xml]$xml = Get-Content $manifestPath; $xml.Package.Applications.Application.Id } }")
        .unwrap_or_else(|| "Claude".to_string())
}

#[cfg(target_os = "windows")]
/// 解析並選出 `detect_claude_path` 的結果。
pub fn detect_claude_path() -> Option<PathBuf> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<PathBuf>>> =
        std::sync::OnceLock::new();
    let cache_mutex = CACHE.get_or_init(|| std::sync::Mutex::new(None));

    // 優先使用快取
    if let Ok(guard) = cache_mutex.lock()
        && let Some(ref path) = *guard
        && path.exists()
    {
        return Some(path.clone());
    }

    let mut detected = None;
    for path in known_claude_paths() {
        if path.exists() {
            detected = Some(path);
            break;
        }
    }
    if detected.is_none()
        && let Some(install_location) = powershell_output(
            "Get-AppxPackage -Name *Claude* | Select-Object -ExpandProperty InstallLocation",
        )
    {
        for suffix in ["app\\Claude.exe", "Claude.exe"] {
            let path = PathBuf::from(&install_location).join(suffix);
            if path.exists() {
                detected = Some(path);
                break;
            }
        }
    }
    if detected.is_none() {
        detected = powershell_output("Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $_.Path } | Select-Object -First 1 -ExpandProperty Path")
            .map(PathBuf::from)
            .filter(|path| path.exists());
    }

    // 更新快取
    if let Ok(mut guard) = cache_mutex.lock() {
        *guard = detected.clone();
    }

    detected
}

#[cfg(not(target_os = "windows"))]
/// 解析並選出 `detect_claude_path` 的結果。
pub fn detect_claude_path() -> Option<PathBuf> {
    for path in known_claude_paths() {
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            let p1 = dir.join("claude-desktop");
            if p1.exists() {
                return Some(p1);
            }
            let p2 = dir.join("claude");
            if p2.exists() {
                return Some(p2);
            }
        }
    }
    None
}

/// 執行 `kill_claude_processes` 對應的處理流程。
pub fn kill_claude_processes() {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "Claude.exe"])
            .creation_flags(crate::constants::CREATE_NO_WINDOW)
            .status();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("pkill").arg("-x").arg("Claude").status();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

#[cfg(target_os = "windows")]
/// 啟動或執行 `launch_claude` 流程。
pub fn launch_claude(custom_path: Option<&Path>) -> AppResult<PathBuf> {
    kill_claude_processes();
    ensure_mirror_profile_initialized()?;
    let target = match custom_path {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => detect_claude_path()
            .ok_or_else(|| AppError::Launcher("找不到 Claude.exe".to_string()))?,
    };

    let target = validate_launch_path(&target)?;
    if !target.exists() {
        return Err(AppError::Launcher("找不到 Claude.exe".to_string()));
    }

    let user_data_arg = format!("--user-data-dir={}", mirror_profile_dir().display());

    let launched = if let Some(family) = get_claude_appx_package_family_name() {
        let target_str = target.to_string_lossy();
        if target_str.contains("WindowsApps") || target_str.contains(&family) {
            let aumid = format!(
                "shell:AppsFolder\\{}!{}",
                family,
                get_claude_appx_application_id()
            );
            Command::new("explorer.exe").arg(aumid).spawn()
        } else {
            Command::new(&target).arg(&user_data_arg).spawn()
        }
    } else {
        Command::new(&target).arg(&user_data_arg).spawn()
    };

    launched
        .map(|_| target)
        .map_err(|error| AppError::Launcher(error.to_string()))
}

#[cfg(not(target_os = "windows"))]
/// 啟動或執行 `launch_claude` 流程。
pub fn launch_claude(custom_path: Option<&Path>) -> AppResult<PathBuf> {
    kill_claude_processes();
    ensure_mirror_profile_initialized()?;
    let target = match custom_path {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => detect_claude_path()
            .ok_or_else(|| AppError::Launcher("找不到 Claude 執行檔".to_string()))?,
    };

    let target = validate_launch_path(&target)?;
    if !target.exists() {
        return Err(AppError::Launcher("找不到 Claude 執行檔".to_string()));
    }

    let user_data_arg = format!("--user-data-dir={}", mirror_profile_dir().display());

    Command::new(&target)
        .arg(&user_data_arg)
        .spawn()
        .map(|_| target)
        .map_err(|error| AppError::Launcher(error.to_string()))
}

/// 清理或還原 `restore_official_config` 所管理的資料。
pub fn restore_official_config() -> AppResult<()> {
    kill_claude_processes();
    remove_managed_config_from_all_paths()?;
    remove_anthropic_base_url_env()?;
    restore_1p_deployment_mode()?;

    match fs::remove_file(crate::config::settings_file()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// 還原 Claude 官方設定並清除本程式所擁有的所有本機資料。
///
/// 此函式僅移除 `local_app_data()/FreeClaudeDesktop`，不會觸碰官方
/// Claude Desktop profile、其他服務或使用者的其他套件資料。
pub fn purge_application_data() -> AppResult<()> {
    restore_official_config()?;
    crate::crypto::delete_stored_secret()?;

    let data_dir = local_app_data().join("FreeClaudeDesktop");
    match fs::remove_dir_all(data_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Launcher(error.to_string())),
    }
}

pub use crate::constants::CONFIG_ID;

/// 執行 `claude_config` 對應的處理流程。
pub fn claude_config(
    port: u16,
    inference_models: &[crate::models::openai::InferenceModel],
    proxy_auth_token: &str,
) -> Value {
    let auth_scheme = crate::config::get_launcher_settings()
        .map(|s| s.gateway.real_auth_scheme)
        .unwrap_or_else(|| "bearer".to_string());

    let mut config = serde_json::json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": format!("http://127.0.0.1:{}", port),
        "inferenceGatewayApiKey": proxy_auth_token,
        "inferenceGatewayAuthScheme": auth_scheme,
        "modelDiscoveryEnabled": true,
        "autoModeEnabled": true,
        "coworkTabEnabled": true,
        "isClaudeCodeForDesktopEnabled": true,
        "chatTabEnabled": true,
        "isDesktopExtensionEnabled": true,
        "extensions": {
            "enabled": true
        }
    });

    if !inference_models.is_empty() {
        let models_val: Vec<Value> = inference_models
            .iter()
            .map(|m| {
                let label = if m.label_override.is_empty() {
                    &m.display_name
                } else {
                    &m.label_override
                };
                let display = if m.display_name.is_empty() {
                    &m.name
                } else {
                    &m.display_name
                };
                let mut item = json!({
                    "name": &m.name,
                    "labelOverride": label,
                    "displayName": display
                });
                if m.supports1m == Some(true) {
                    item["supports1m"] = serde_json::json!(true);
                }
                if m.supports1m == Some(true) && m.prefer1m == Some(true) {
                    item["prefer1m"] = serde_json::json!(true);
                }
                item
            })
            .collect();
        config["inferenceModels"] = Value::Array(models_val);
    }

    config
}

/// 轉換或更新 `update_applied_claude_config` 所處理的內容。
pub fn update_applied_claude_config(
    port: u16,
    inference_models: &[crate::models::openai::InferenceModel],
) {
    if let Err(error) = try_update_applied_claude_config(port, inference_models) {
        tracing::error!(%error, "Failed to update applied Claude config");
    }
}

/// 執行 `try_update_applied_claude_config` 對應的處理流程。
pub fn try_update_applied_claude_config(
    port: u16,
    inference_models: &[crate::models::openai::InferenceModel],
) -> AppResult<()> {
    let token = crate::config::load_launcher_settings()?
        .map(|settings| settings.gateway.proxy_auth_token)
        .unwrap_or_else(crate::config::default_proxy_auth_token);
    let content = serde_json::to_string_pretty(&claude_config(port, inference_models, &token))?;
    write_config_to_all_paths(&format!("{CONFIG_ID}.json"), &content)
}

/// 轉換或更新 `update_config_port` 所處理的內容。
pub fn update_config_port(port: u16) -> AppResult<()> {
    apply_anthropic_base_url_env(port)?;
    update_gateway_port_in_all_paths(port)
}

/// 執行 `with_gateway_port` 對應的處理流程。
fn with_gateway_port(mut config: Value, port: u16) -> Value {
    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "inferenceGatewayBaseUrl".to_string(),
            Value::String(format!("http://127.0.0.1:{}", port)),
        );
    }
    config
}

/// 轉換或更新 `update_gateway_port_in_all_paths` 所處理的內容。
fn update_gateway_port_in_all_paths(port: u16) -> AppResult<()> {
    let mut writes = Vec::new();
    for dir in config_library_dirs() {
        let path = dir.join(format!("{CONFIG_ID}.json"));
        if !path.exists() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let config = serde_json::from_str::<Value>(&text).map_err(AppError::InvalidConfigJson)?;
        let content = serde_json::to_string_pretty(&with_gateway_port(config, port))?;
        writes.push(PendingWrite::new(path, content.into_bytes()));
    }
    write_transaction(writes)?;
    Ok(())
}

mod environment;

#[cfg(test)]
pub(crate) use environment::PREVIOUS_CLAUDE_SETTINGS_KEY;
pub use environment::{
    apply_anthropic_base_url_env, claude_home_dir, claude_settings_json_path,
    remove_anthropic_base_url_env,
};

mod mcp;

#[cfg(test)]
pub(crate) use mcp::strip_removed_computer_mcp;
pub use mcp::{
    app_data_roaming_dir, apply_3p_deployment_mode, apply_managed_deployment_mode, clean_json_text,
    collect_all_mcp_servers, collect_all_mcp_servers_result, mcp_config_paths, merge_mcp_servers,
    read_json_config, restore_1p_deployment_mode, restore_managed_deployment_mode,
};

#[cfg(test)]
mod tests;
