use serde_json::{json, Value};
use std::env;
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::local_app_data;
use crate::error::{AppError, AppResult};

pub fn mirror_profile_dir() -> PathBuf {
    local_app_data().join("FreeClaudeLauncher").join("claude_profile")
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
        let _ = copy_dir_all(&official, &mirror);
    }
    let settings = crate::config::get_launcher_settings();
    let port = settings.as_ref().and_then(|s| s.active_port).unwrap_or(crate::constants::DEFAULT_PORT);
    let enable_computer_mcp = settings.as_ref().map(|s| s.enable_computer_mcp_server).unwrap_or(false);

    apply_3p_deployment_mode()?;
    apply_computer_mcp_server_config(enable_computer_mcp)?;
    let _ = update_config_port(port);
    Ok(())
}

pub fn reset_mirror_profile() -> AppResult<()> {
    let mirror = mirror_profile_dir();
    if mirror.exists() {
        let _ = fs::remove_dir_all(&mirror);
    }
    ensure_mirror_profile_initialized()
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
        // Windows Store 版 Claude 無法吃 --user-data-dir，ClaudeSource 會固定讀這裡的 3P profile。
        dirs.push(local_app_data().join("Claude-3p").join("configLibrary"));
        dirs.push(app_data_roaming_dir().join("Claude-3p").join("configLibrary"));

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
    let _ = remove_managed_config_from_all_paths();
    let _ = remove_anthropic_base_url_env();
    let _ = apply_computer_mcp_server_config(false);
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
        "modelDiscoveryEnabled": true,
        "autoModeEnabled": true,
        "coworkTabEnabled": true,
        "isClaudeCodeForDesktopEnabled": true,
        "chatTabEnabled": true,
        "extensions": {
            "enabled": true
        }
    });

    if !inference_models.is_empty() {
        let models_val: Vec<Value> = inference_models
            .iter()
            .map(|m| {
                let mut item = json!({
                    "name": m.name,
                    "labelOverride": if m.label_override.is_empty() { &m.display_name } else { &m.label_override },
                    "displayName": if m.display_name.is_empty() { &m.name } else { &m.display_name }
                });
                if m.supports1m == Some(true) {
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

    if data.get("autoModeEnabled").is_none() {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("autoModeEnabled".to_string(), Value::Bool(true));
        }
    }

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
                if env_obj.remove("ENABLE_TOOL_SEARCH").is_some() {
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
            let mut temp_chars = out_chars.clone();
            let mut trailing = false;
            while let Some(next_c) = temp_chars.next() {
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
    if !path.exists() {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let cleaned = clean_json_text(&text);
    serde_json::from_str(&cleaned).ok()
}

pub fn is_managed_computer_mcp_server(name: &str, val: &Value) -> bool {
    if name == COMPUTER_MCP_SERVER_NAME || name == "launcher-computer" {
        return true;
    }
    if let Some(cmd) = val.get("command").and_then(Value::as_str) {
        let lower = cmd.to_lowercase();
        if lower.contains("freeclaude") || lower.contains("launcher") {
            return true;
        }
    }
    if let Some(args) = val.get("args").and_then(Value::as_array) {
        for arg in args {
            if let Some(s) = arg.as_str() {
                let trimmed = s.trim_start_matches('-');
                if trimmed == "mcp" || trimmed == "mcp-computer-server" {
                    return true;
                }
            }
        }
    }
    false
}

pub fn collect_all_mcp_servers() -> serde_json::Map<String, Value> {
    let mut merged = serde_json::Map::new();
    let mut search_paths = mcp_config_paths();
    search_paths.push(official_app_data_dir().join("claude_desktop_config.json"));

    for path in search_paths {
        if path.exists() {
            if let Some(data) = read_json_config(&path) {
                if let Some(servers) = data.get("mcpServers").and_then(Value::as_object) {
                    for (k, v) in servers {
                        if !is_managed_computer_mcp_server(k, v) && !merged.contains_key(k) {
                            merged.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }
    }
    merged
}

pub fn merge_mcp_servers(mut data: Value, all_servers: &serde_json::Map<String, Value>) -> Value {
    if !data.is_object() {
        data = json!({});
    }
    if !all_servers.is_empty() {
        if let Some(obj) = data.as_object_mut() {
            let servers = obj
                .entry("mcpServers")
                .or_insert_with(|| json!({}));
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

const COMPUTER_MCP_SERVER_NAME: &str = "free-claude-computer";

pub fn with_computer_mcp_server(mut data: Value, enabled: bool, command: &Path) -> Value {
    if !data.is_object() {
        data = json!({});
    }

    let obj = data.as_object_mut().unwrap();
    if let Some(servers) = obj.get_mut("mcpServers").and_then(Value::as_object_mut) {
        let legacy_keys: Vec<String> = servers
            .iter()
            .filter(|(k, v)| is_managed_computer_mcp_server(k, v))
            .map(|(k, _)| k.clone())
            .collect();
        for key in legacy_keys {
            servers.remove(&key);
        }
    }

    if enabled {
        let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
        if !servers.is_object() {
            *servers = json!({});
        }
        servers.as_object_mut().unwrap().insert(
            COMPUTER_MCP_SERVER_NAME.to_string(),
            json!({
                "command": command.to_string_lossy(),
                "args": ["--mcp-computer-server"]
            }),
        );
    } else if let Some(servers) = obj.get_mut("mcpServers").and_then(Value::as_object_mut) {
        if servers.is_empty() {
            obj.remove("mcpServers");
        }
    }

    data
}

pub fn apply_computer_mcp_server_config(enabled: bool) -> AppResult<()> {
    let command = if enabled {
        env::current_exe().map_err(|error| AppError::Launcher(error.to_string()))?
    } else {
        PathBuf::new()
    };

    let all_mcp_servers = collect_all_mcp_servers();

    for path in mcp_config_paths() {
        let (data_opt, file_existed) = if path.exists() {
            (read_json_config(&path), true)
        } else {
            if !enabled && all_mcp_servers.is_empty() {
                continue;
            }
            (Some(json!({})), false)
        };

        if file_existed && data_opt.is_none() {
            tracing::warn!("Skipping writing MCP config to {:?} because JSON parsing failed", path);
            continue;
        }

        let data = data_opt.unwrap_or_else(|| json!({}));
        let data = merge_mcp_servers(data, &all_mcp_servers);
        let data = with_computer_mcp_server(data, enabled, &command);

        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(&data)?;
        fs::write(&path, content)?;
    }

    Ok(())
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
    let all_mcp_servers = collect_all_mcp_servers();

    for path in mcp_config_paths() {
        let (data_opt, file_existed) = if path.exists() {
            (read_json_config(&path), true)
        } else {
            let parent_exists = path.parent().map(|p| p.exists()).unwrap_or(false);
            if !parent_exists && all_mcp_servers.is_empty() {
                continue;
            }
            (Some(json!({})), false)
        };

        if file_existed && data_opt.is_none() {
            tracing::warn!("Skipping writing config to {:?} because JSON parsing failed", path);
            continue;
        }

        let data = data_opt.unwrap_or_else(|| json!({}));
        let data = merge_mcp_servers(data, &all_mcp_servers);
        let data = apply_managed_deployment_mode(data);

        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(&data)?;
        let _ = fs::write(&path, content);
    }
    Ok(())
}

pub fn restore_1p_deployment_mode() -> AppResult<()> {
    let all_mcp_servers = collect_all_mcp_servers();

    for path in mcp_config_paths() {
        if path.exists() {
            if let Some(data) = read_json_config(&path) {
                let data = merge_mcp_servers(data, &all_mcp_servers);
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

    #[test]
    fn computer_mcp_config_preserves_other_servers() {
        let command = Path::new("C:\\FreeClaudeLauncher.exe");
        let enabled = with_computer_mcp_server(
            json!({
                "mcpServers": {
                    "other": { "command": "node" }
                }
            }),
            true,
            command,
        );

        assert_eq!(enabled["mcpServers"]["other"]["command"], "node");
        assert_eq!(
            enabled["mcpServers"]["free-claude-computer"]["args"][0],
            "--mcp-computer-server"
        );

        let disabled = with_computer_mcp_server(enabled, false, command);
        assert_eq!(disabled["mcpServers"]["other"]["command"], "node");
        assert!(disabled["mcpServers"].get("free-claude-computer").is_none());
    }

    #[test]
    fn claude_config_includes_supports1m_field() {
        let model = crate::models::openai::InferenceModel {
            name: "claude-sonnet-4-6[0]".to_string(),
            label_override: "deepseek-v4-flash".to_string(),
            provider_model_id: "deepseek-v4-flash".to_string(),
            display_name: "deepseek-v4-flash".to_string(),
            max_input_tokens: Some(1_000_000),
            max_tokens: Some(8192),
            capabilities: serde_json::json!({}),
            supports1m: Some(true),
            transport_type: None,
        };

        let config = claude_config(12345, &[model], "proxy-token");
        let models = config["inferenceModels"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["name"], "claude-sonnet-4-6[0]");
        assert_eq!(models[0]["supports1m"], true);
    }

    #[test]
    fn claude_config_enables_chat_and_extensions_by_default() {
        let config = claude_config(12345, &[], "proxy-token");
        assert_eq!(config["coworkTabEnabled"], true);
        assert_eq!(config["isClaudeCodeForDesktopEnabled"], true);
        assert_eq!(config["chatTabEnabled"], true);
        assert_eq!(config["extensions"]["enabled"], true);
    }

    #[test]
    fn clean_json_text_strips_comments_bom_and_trailing_commas() {
        let raw = "\u{feff}{\n  // line comment\n  \"mcpServers\": {\n    /* block comment */\n    \"custom\": { \"command\": \"node\", },\n  },\n}";
        let cleaned = clean_json_text(raw);
        let parsed: Value = serde_json::from_str(&cleaned).expect("Failed to parse cleaned json");
        assert_eq!(parsed["mcpServers"]["custom"]["command"], "node");
    }

    #[test]
    fn clean_json_text_robust_boundary_scenarios() {
        let raw = "\u{feff}{
            // line comment
            \"url\": \"http://example.com/api\",
            \"comment_block_in_str\": \"/* this is not a comment */\",
            \"escaped_quote_in_str\": \"hello \\\"world\\\"\",
            \"escaped_slash_in_str\": \"hello \\/ world\",
            \"unicode_escapes\": \"\\u0022hello\\u0022\",
            \"unicode_backslash\": \"\\u005c\",
            \"array_with_trailing\": [
                1,
                2,
                3,
            ],
            \"object_with_trailing\": {
                \"a\": 1,
                \"b\": 2,
            },
        }";
        let cleaned = clean_json_text(raw);
        let parsed: Value = serde_json::from_str(&cleaned).expect(&format!("Failed to parse: {}", cleaned));
        assert_eq!(parsed["url"], "http://example.com/api");
        assert_eq!(parsed["comment_block_in_str"], "/* this is not a comment */");
        assert_eq!(parsed["escaped_quote_in_str"], "hello \"world\"");
        assert_eq!(parsed["escaped_slash_in_str"], "hello / world");
        assert_eq!(parsed["unicode_escapes"], "\"hello\"");
        assert_eq!(parsed["unicode_backslash"], "\\");
        assert_eq!(parsed["array_with_trailing"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["object_with_trailing"]["b"], 2);
    }

    #[test]
    fn merge_mcp_servers_preserves_and_merges_custom_servers() {
        let mut servers = serde_json::Map::new();
        servers.insert("user_mcp".to_string(), json!({ "command": "python" }));

        let data = json!({
            "deploymentMode": "3p",
            "mcpServers": {
                "existing_mcp": { "command": "node" }
            }
        });

        let merged = merge_mcp_servers(data, &servers);
        assert_eq!(merged["mcpServers"]["existing_mcp"]["command"], "node");
        assert_eq!(merged["mcpServers"]["user_mcp"]["command"], "python");
    }

    #[test]
    fn removes_legacy_launcher_computer_mcp_entry() {
        let command = Path::new("C:\\FreeClaudeLauncher.exe");
        let data = json!({
            "mcpServers": {
                "launcher-computer": { "command": "C:\\FreeClaudeLauncher.exe", "args": ["mcp"] },
                "custom": { "command": "node" }
            }
        });

        let updated = with_computer_mcp_server(data, true, command);
        assert!(updated["mcpServers"].get("launcher-computer").is_none());
        assert_eq!(updated["mcpServers"]["free-claude-computer"]["args"][0], "--mcp-computer-server");
        assert_eq!(updated["mcpServers"]["custom"]["command"], "node");
    }

    #[test]
    fn mirror_profile_dir_returns_valid_path() {
        let mirror = mirror_profile_dir();
        assert!(mirror.to_string_lossy().contains("FreeClaudeLauncher"));
        assert!(mirror.to_string_lossy().contains("claude_profile"));
    }

    #[test]
    fn official_app_data_dir_returns_valid_path() {
        let official = official_app_data_dir();
        assert!(official.to_string_lossy().contains("Claude"));
    }

    #[test]
    fn copy_dir_all_recursively_copies_files() {
        let temp_src = std::env::temp_dir().join(format!("fcl_test_src_{}", std::process::id()));
        let temp_dst = std::env::temp_dir().join(format!("fcl_test_dst_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_src);
        let _ = fs::remove_dir_all(&temp_dst);

        let sub_dir = temp_src.join("subdir");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(sub_dir.join("test.txt"), "hello").unwrap();

        copy_dir_all(&temp_src, &temp_dst).unwrap();
        assert_eq!(fs::read_to_string(temp_dst.join("subdir").join("test.txt")).unwrap(), "hello");

        let _ = fs::remove_dir_all(&temp_src);
        let _ = fs::remove_dir_all(&temp_dst);
    }
}
