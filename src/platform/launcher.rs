use serde_json::{json, Value};
use std::env;
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::local_app_data;
use crate::error::{AppError, AppResult};

pub fn user_data_dir() -> PathBuf {
    if let Ok(dir) = env::var("CLAUDE_USER_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    local_app_data().join("Claude-3p")
}

pub fn config_lib_dir() -> PathBuf {
    user_data_dir().join("configLibrary")
}

pub fn meta_file() -> PathBuf {
    config_lib_dir().join("_meta.json")
}

#[cfg(target_os = "windows")]
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

    // 探測 WindowsApps 下的包目錄 (如 C:\Program Files\WindowsApps\Claude_*\app\Claude.exe)
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
pub fn known_claude_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/bin/claude-desktop"),
        PathBuf::from("/usr/local/bin/claude-desktop"),
        PathBuf::from("/usr/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
}

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

pub fn write_config_to_all_paths(file_name: &str, content: &str) -> AppResult<()> {
    for dir in config_library_dirs() {
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(file_name), content)?;
    }
    Ok(())
}

fn config_library_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![config_lib_dir()];
    #[cfg(target_os = "windows")]
    {
        let packages_dir = env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("AppData")
            .join("Local")
            .join("Packages");
        if let Ok(entries) = fs::read_dir(packages_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if !name.contains("claude") {
                    continue;
                }
                let dir = entry
                    .path()
                    .join("LocalCache")
                    .join("Local")
                    .join("Claude-3p")
                    .join("configLibrary");
                dirs.push(dir);
            }
        }
    }
    dirs
}

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
        entries.push(json!({ "id": CONFIG_ID, "name": "FreeClaudeLauncher" }));
    } else {
        obj.insert(
            "entries".to_string(),
            json!([{ "id": CONFIG_ID, "name": "FreeClaudeLauncher" }]),
        );
    }
    meta
}

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

pub fn write_managed_meta_to_all_paths() -> AppResult<()> {
    for dir in config_library_dirs() {
        fs::create_dir_all(&dir)?;
        let path = dir.join("_meta.json");
        let meta = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or_else(|| json!({}));
        let content = serde_json::to_string_pretty(&upsert_managed_meta_entry(meta))?;
        fs::write(path, content)?;
    }
    Ok(())
}

fn remove_managed_config_from_all_paths() -> AppResult<()> {
    for dir in config_library_dirs() {
        let _ = fs::remove_file(dir.join(format!("{CONFIG_ID}.json")));
        let meta_path = dir.join("_meta.json");
        if meta_path.exists() {
            let text = fs::read_to_string(&meta_path)?;
            if let Ok(meta) = serde_json::from_str::<Value>(&text) {
                let content = serde_json::to_string_pretty(&remove_managed_meta_entry(meta))?;
                fs::write(meta_path, content)?;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
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
fn get_claude_appx_package_family_name() -> Option<String> {
    powershell_output(
        "Get-AppxPackage -Name *Claude* | Select-Object -ExpandProperty PackageFamilyName",
    )
}

#[cfg(target_os = "windows")]
fn get_claude_appx_application_id() -> String {
    powershell_output("$app = Get-AppxPackage -Name *Claude*; if ($app) { $manifestPath = Join-Path $app.InstallLocation 'AppxManifest.xml'; if (Test-Path $manifestPath) { [xml]$xml = Get-Content $manifestPath; $xml.Package.Applications.Application.Id } }")
        .unwrap_or_else(|| "Claude".to_string())
}

#[cfg(target_os = "windows")]
pub fn detect_claude_path() -> Option<PathBuf> {
    for path in known_claude_paths() {
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(install_location) = powershell_output(
        "Get-AppxPackage -Name *Claude* | Select-Object -ExpandProperty InstallLocation",
    ) {
        for suffix in ["app\\Claude.exe", "Claude.exe"] {
            let path = PathBuf::from(&install_location).join(suffix);
            if path.exists() {
                return Some(path);
            }
        }
    }
    powershell_output("Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $_.Path } | Select-Object -First 1 -ExpandProperty Path")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

#[cfg(not(target_os = "windows"))]
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
pub fn launch_claude(custom_path: Option<&Path>) -> AppResult<PathBuf> {
    kill_claude_processes();
    let target = match custom_path {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => detect_claude_path()
            .ok_or_else(|| AppError::Launcher("找不到 Claude.exe".to_string()))?,
    };

    let target = validate_launch_path(&target)?;
    if !target.exists() {
        return Err(AppError::Launcher("找不到 Claude.exe".to_string()));
    }

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
            Command::new(&target).spawn()
        }
    } else {
        Command::new(&target).spawn()
    };

    launched
        .map(|_| target)
        .map_err(|error| AppError::Launcher(error.to_string()))
}

#[cfg(not(target_os = "windows"))]
pub fn launch_claude(custom_path: Option<&Path>) -> AppResult<PathBuf> {
    kill_claude_processes();
    let target = match custom_path {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => detect_claude_path()
            .ok_or_else(|| AppError::Launcher("找不到 Claude 執行檔".to_string()))?,
    };

    let target = validate_launch_path(&target)?;
    if !target.exists() {
        return Err(AppError::Launcher("找不到 Claude 執行檔".to_string()));
    }

    Command::new(&target)
        .spawn()
        .map(|_| target)
        .map_err(|error| AppError::Launcher(error.to_string()))
}

pub fn restore_official_config() -> AppResult<()> {
    kill_claude_processes();
    let _ = remove_managed_config_from_all_paths();
    let _ = remove_anthropic_base_url_env();
    let _ = restore_1p_deployment_mode();

    let _ = fs::remove_file(crate::config::settings_file());
    let legacy = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("launcher_settings.json");
    let s_file = crate::config::settings_file();
    if legacy != s_file {
        let _ = fs::remove_file(legacy);
    }
    Ok(())
}

pub use crate::constants::CONFIG_ID;

pub fn claude_config(
    port: u16,
    inference_models: &[crate::models::openai::InferenceModel],
    proxy_auth_token: &str,
) -> Value {
    let auth_scheme = crate::config::get_launcher_settings()
        .map(|s| s.real_auth_scheme)
        .unwrap_or_else(|| "bearer".to_string());

    let mut config = serde_json::json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": format!("http://127.0.0.1:{}", port),
        "inferenceGatewayApiKey": proxy_auth_token,
        "inferenceGatewayAuthScheme": auth_scheme,
        "modelDiscoveryEnabled": true
    });

    if !inference_models.is_empty() {
        let models_val: Vec<Value> = inference_models
            .iter()
            .map(|m| {
                json!({
                    "name": m.name,
                    "displayName": if m.display_name.is_empty() { &m.name } else { &m.display_name }
                })
            })
            .collect();
        config["inferenceModels"] = Value::Array(models_val);
    }

    config
}

pub fn update_applied_claude_config(
    port: u16,
    inference_models: &[crate::models::openai::InferenceModel],
) {
    let token = crate::config::get_launcher_settings()
        .map(|settings| settings.proxy_auth_token)
        .unwrap_or_else(crate::config::default_proxy_auth_token);
    let content =
        serde_json::to_string_pretty(&claude_config(port, inference_models, &token)).unwrap();
    let _ = write_config_to_all_paths(&format!("{CONFIG_ID}.json"), &content);
}

pub fn update_config_port(port: u16) -> AppResult<()> {
    apply_anthropic_base_url_env(port)?;
    update_gateway_port_in_all_paths(port)
}

fn with_gateway_port(mut config: Value, port: u16) -> Value {
    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "inferenceGatewayBaseUrl".to_string(),
            Value::String(format!("http://127.0.0.1:{}", port)),
        );
    }
    config
}

fn update_gateway_port_in_all_paths(port: u16) -> AppResult<()> {
    for dir in config_library_dirs() {
        let path = dir.join(format!("{CONFIG_ID}.json"));
        if !path.exists() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let config = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({}));
        let content = serde_json::to_string_pretty(&with_gateway_port(config, port))?;
        fs::write(path, content)?;
    }
    Ok(())
}

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

pub fn claude_settings_json_path() -> PathBuf {
    claude_home_dir().join(".claude").join("settings.json")
}

pub fn apply_anthropic_base_url_env(port: u16) -> AppResult<()> {
    let path = claude_settings_json_path();
    let mut data: Value = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).unwrap_or(json!({}))
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        json!({})
    };

    if data.get("env").is_none() {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("env".to_string(), json!({}));
        }
    }

    if let Some(env_obj) = data.get_mut("env").and_then(Value::as_object_mut) {
        env_obj.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(format!("http://127.0.0.1:{}", port)),
        );
    }

    let content = serde_json::to_string_pretty(&data)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn remove_anthropic_base_url_env() -> AppResult<()> {
    let path = claude_settings_json_path();
    if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        if let Ok(mut data) = serde_json::from_str::<Value>(&text) {
            let mut changed = false;
            if let Some(env_obj) = data.get_mut("env").and_then(Value::as_object_mut) {
                if env_obj.remove("ANTHROPIC_BASE_URL").is_some() {
                    changed = true;
                }
            }
            if changed {
                let content = serde_json::to_string_pretty(&data)?;
                std::fs::write(&path, content)?;
            }
        }
    }
    Ok(())
}

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

pub fn mcp_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. 標準 Local AppData 路徑下的 Claude-3p
    let std_dir = local_app_data().join("Claude-3p");
    paths.push(std_dir.join("claude_desktop_config.json"));

    // 2. MSIX 封裝的 LocalCache 路徑下
    #[cfg(target_os = "windows")]
    {
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
                    let dir = entry
                        .path()
                        .join("LocalCache")
                        .join("Local")
                        .join("Claude-3p");
                    paths.push(dir.join("claude_desktop_config.json"));
                }
            }
        }
    }

    // 3. 標準 Roaming 路徑下的 Claude (作為 fallback/相容)
    paths.push(
        app_data_roaming_dir()
            .join("Claude")
            .join("claude_desktop_config.json"),
    );

    paths
}

const PREVIOUS_DEPLOYMENT_MODE_KEY: &str = "freeClaudeLauncherPreviousDeploymentMode";

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

pub fn apply_3p_deployment_mode() -> AppResult<()> {
    for path in mcp_config_paths() {
        let data: Value = if path.exists() {
            let text = fs::read_to_string(&path)?;
            serde_json::from_str(&text).unwrap_or(json!({}))
        } else {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    continue;
                }
            }
            json!({})
        };

        let data = apply_managed_deployment_mode(data);
        let content = serde_json::to_string_pretty(&data)?;
        let _ = fs::write(&path, content);
    }
    Ok(())
}

pub fn restore_1p_deployment_mode() -> AppResult<()> {
    for path in mcp_config_paths() {
        if path.exists() {
            let text = fs::read_to_string(&path)?;
            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                let content = serde_json::to_string_pretty(&restore_managed_deployment_mode(data))?;
                let _ = fs::write(&path, content);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_upsert_preserves_existing_entries() {
        let meta = json!({
            "appliedId": "other-id",
            "entries": [
                { "id": "other-id", "name": "Other Config" }
            ]
        });

        let updated = upsert_managed_meta_entry(meta);
        let entries = updated["entries"].as_array().unwrap();

        assert_eq!(updated["appliedId"], CONFIG_ID);
        assert!(entries.iter().any(|entry| entry["id"] == "other-id"));
        assert!(entries.iter().any(|entry| entry["id"] == CONFIG_ID));
    }

    #[test]
    fn meta_remove_only_removes_managed_entry() {
        let meta = json!({
            "appliedId": CONFIG_ID,
            "entries": [
                { "id": "other-id", "name": "Other Config" },
                { "id": CONFIG_ID, "name": "FreeClaudeLauncher" }
            ]
        });

        let updated = remove_managed_meta_entry(meta);
        let entries = updated["entries"].as_array().unwrap();

        assert_eq!(updated["appliedId"], "other-id");
        assert!(entries.iter().any(|entry| entry["id"] == "other-id"));
        assert!(!entries.iter().any(|entry| entry["id"] == CONFIG_ID));
    }

    #[test]
    fn deployment_mode_restore_keeps_previous_value() {
        let original = json!({ "deploymentMode": "custom" });

        let applied = apply_managed_deployment_mode(original);
        assert_eq!(applied["deploymentMode"], "3p");
        assert_eq!(
            applied["freeClaudeLauncherPreviousDeploymentMode"],
            "custom"
        );

        let restored = restore_managed_deployment_mode(applied);
        assert_eq!(restored["deploymentMode"], "custom");
        assert!(restored
            .get("freeClaudeLauncherPreviousDeploymentMode")
            .is_none());
    }

    #[test]
    fn gateway_port_rewrite_updates_config_value() {
        let updated = with_gateway_port(
            json!({
                "inferenceProvider": "gateway",
                "inferenceGatewayBaseUrl": "http://127.0.0.1:3000",
                "other": true
            }),
            4567,
        );

        assert_eq!(updated["inferenceGatewayBaseUrl"], "http://127.0.0.1:4567");
        assert_eq!(updated["other"], true);
    }
}
