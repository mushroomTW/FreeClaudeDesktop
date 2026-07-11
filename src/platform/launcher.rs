use serde_json::{json, Value};
use std::env;
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::local_app_data;
use crate::error::{AppError, AppResult};
use crate::platform::atomic_file::{write_transaction, PendingWrite};

pub fn mirror_profile_dir() -> PathBuf {
    local_app_data()
        .join("FreeClaudeLauncher")
        .join("claude_profile")
}

pub fn official_app_data_dir() -> PathBuf {
    app_data_roaming_dir().join("Claude")
}

pub fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    if !src.exists() {
        return Ok(());
    }

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn ensure_mirror_profile_initialized() -> AppResult<()> {
    let mirror = mirror_profile_dir();
    if !mirror.exists() {
        let old_3p = local_app_data().join("Claude-3p");
        if old_3p.exists() {
            copy_dir_all(&old_3p, &mirror)?;
        } else {
            let official = official_app_data_dir();
            if official.exists() {
                copy_dir_all(&official, &mirror)?;
            } else {
                fs::create_dir_all(&mirror)?;
            }
        }
    }
    Ok(())
}

pub fn resync_from_official() -> AppResult<()> {
    let official = official_app_data_dir();
    let mirror = mirror_profile_dir();
    if official.exists() {
        copy_dir_all(&official, &mirror)?;
    }
    let settings = crate::config::load_launcher_settings()?;
    let port = settings
        .as_ref()
        .and_then(|s| s.active_port)
        .unwrap_or(crate::constants::DEFAULT_PORT);
    apply_3p_deployment_mode()?;
    update_config_port(port)?;
    Ok(())
}

pub fn reset_mirror_profile() -> AppResult<()> {
    let mirror = mirror_profile_dir();
    let backup = mirror.with_file_name(format!(
        "{}.reset-backup-{}",
        mirror
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("claude_profile"),
        std::process::id()
    ));
    if mirror.exists() {
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        fs::rename(&mirror, &backup)?;
    }
    let result = ensure_mirror_profile_initialized();
    if result.is_err() && backup.exists() {
        let _ = fs::remove_dir_all(&mirror);
        let _ = fs::rename(&backup, &mirror);
    } else if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    result
}

pub fn user_data_dir() -> PathBuf {
    if let Ok(dir) = env::var("CLAUDE_USER_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    mirror_profile_dir()
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

fn config_library_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![config_lib_dir()];
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
    let legacy = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("launcher_settings.json");
    let s_file = crate::config::settings_file();
    if legacy != s_file {
        match fs::remove_file(legacy) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub use crate::constants::CONFIG_ID;

fn claude_config_model_name(name: &str) -> (&str, bool) {
    name.strip_suffix("[1m]")
        .or_else(|| name.strip_suffix("[1M]"))
        .map(|base| (base, true))
        .unwrap_or((name, false))
}

fn strip_display_1m_suffix(name: &str) -> &str {
    name.strip_suffix(" 1M")
        .or_else(|| name.strip_suffix(" 1m"))
        .or_else(|| name.strip_suffix("-1M"))
        .or_else(|| name.strip_suffix("-1m"))
        .unwrap_or(name)
}

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
                let (config_name, has_1m_suffix) = claude_config_model_name(&m.name);
                let supports_1m = has_1m_suffix || m.supports1m == Some(true);
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
                    "name": config_name,
                    "labelOverride": if supports_1m { strip_display_1m_suffix(label) } else { label },
                    "displayName": if supports_1m { strip_display_1m_suffix(display) } else { display }
                });
                if supports_1m {
                    item["supports1m"] = json!(true);
                }
                item
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
    if let Err(error) = try_update_applied_claude_config(port, inference_models) {
        tracing::error!(%error, "Failed to update applied Claude config");
    }
}

pub fn try_update_applied_claude_config(
    port: u16,
    inference_models: &[crate::models::openai::InferenceModel],
) -> AppResult<()> {
    let token = crate::config::load_launcher_settings()?
        .map(|settings| settings.proxy_auth_token)
        .unwrap_or_else(crate::config::default_proxy_auth_token);
    let content = serde_json::to_string_pretty(&claude_config(port, inference_models, &token))?;
    write_config_to_all_paths(&format!("{CONFIG_ID}.json"), &content)
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

const MANAGED_CLAUDE_ENV_KEYS: [&str; 3] = [
    "ANTHROPIC_BASE_URL",
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_ENABLE_AUTO_MODE",
];
const PREVIOUS_CLAUDE_SETTINGS_KEY: &str = "freeClaudeLauncherPreviousSettings";

fn previous_setting_entry(value: Option<&Value>) -> Value {
    json!({
        "present": value.is_some(),
        "value": value.cloned().unwrap_or(Value::Null)
    })
}

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
    vec![mirror_profile_dir().join("claude_desktop_config.json")]
}

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
                    if c == '*' {
                        if let Some(&'/') = chars.peek() {
                            chars.next();
                            break;
                        }
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

pub fn read_json_config(path: &Path) -> Option<Value> {
    read_json_config_result(path).ok().flatten()
}

fn read_json_config_result(path: &Path) -> AppResult<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let cleaned = clean_json_text(&text);
    let value = serde_json::from_str(&cleaned).map_err(AppError::InvalidConfigJson)?;
    Ok(Some(value))
}

pub fn collect_all_mcp_servers() -> serde_json::Map<String, Value> {
    collect_all_mcp_servers_result().unwrap_or_default()
}

pub fn collect_all_mcp_servers_result() -> AppResult<serde_json::Map<String, Value>> {
    let mut merged = serde_json::Map::new();
    let mut search_paths = mcp_config_paths();
    search_paths.push(official_app_data_dir().join("claude_desktop_config.json"));

    for path in search_paths {
        if let Some(data) = read_json_config_result(&path)? {
            if let Some(servers) = data.get("mcpServers").and_then(Value::as_object) {
                for (k, v) in servers {
                    if !merged.contains_key(k) {
                        merged.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }
    Ok(merged)
}

pub fn merge_mcp_servers(mut data: Value, all_servers: &serde_json::Map<String, Value>) -> Value {
    if !data.is_object() {
        data = json!({});
    }
    if !all_servers.is_empty() {
        if let Some(obj) = data.as_object_mut() {
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
    }
    data
}

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

pub fn restore_1p_deployment_mode() -> AppResult<()> {
    let all_mcp_servers = collect_all_mcp_servers_result()?;
    let mut writes = Vec::new();

    for path in mcp_config_paths() {
        if path.exists() {
            if let Some(data) = read_json_config_result(&path)? {
                let data = merge_mcp_servers(data, &all_mcp_servers);
                let content = serde_json::to_string_pretty(&restore_managed_deployment_mode(data))?;
                writes.push(PendingWrite::new(path, content.into_bytes()));
            }
        }
    }
    write_transaction(writes)?;
    Ok(())
}

#[cfg(test)]
mod tests;
