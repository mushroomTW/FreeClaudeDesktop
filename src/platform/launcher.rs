use serde_json::{json, Value};
use std::env;
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::local_app_data;
use crate::error::{AppError, AppResult};

pub fn config_lib_dir() -> PathBuf {
    local_app_data().join("Claude-3p").join("configLibrary")
}

pub fn meta_file() -> PathBuf {
    config_lib_dir().join("_meta.json")
}

#[cfg(target_os = "windows")]
pub fn known_claude_paths() -> Vec<PathBuf> {
    let local = local_app_data();
    vec![
        local
            .join("Programs")
            .join("claude-desktop")
            .join("Claude.exe"),
        local.join("Programs").join("Claude").join("Claude.exe"),
        PathBuf::from(env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string()))
            .join("Claude")
            .join("Claude.exe"),
        PathBuf::from(
            env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string()),
        )
        .join("Claude")
        .join("Claude.exe"),
    ]
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
    let standard_dir = config_lib_dir();
    fs::create_dir_all(&standard_dir)?;
    fs::write(standard_dir.join(file_name), content)?;

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
                let _ = fs::create_dir_all(&dir);
                let _ = fs::write(dir.join(file_name), content);
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
    let _ = fs::remove_dir_all(config_lib_dir());
    let _ = remove_anthropic_base_url_env();
    let _ = restore_1p_deployment_mode();

    #[cfg(target_os = "windows")]
    {
        if let Ok(entries) = fs::read_dir(
            env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("AppData")
                .join("Local")
                .join("Packages"),
        ) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("claude")
                {
                    let _ = fs::remove_dir_all(
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
    _inference_models: &[crate::models::openai::InferenceModel],
) -> Value {
    serde_json::json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": format!("http://127.0.0.1:{}", port),
        "inferenceGatewayApiKey": crate::constants::PROXY_AUTH_TOKEN,
        "inferenceGatewayAuthScheme": "bearer",
        "modelDiscoveryEnabled": true
    })
}

pub fn update_applied_claude_config(
    port: u16,
    inference_models: &[crate::models::openai::InferenceModel],
) {
    let content = serde_json::to_string_pretty(&claude_config(port, inference_models)).unwrap();
    let _ = write_config_to_all_paths(&format!("{CONFIG_ID}.json"), &content);
}

pub fn update_config_port(port: u16) -> AppResult<()> {
    apply_anthropic_base_url_env(port)
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

pub fn apply_3p_deployment_mode() -> AppResult<()> {
    for path in mcp_config_paths() {
        let mut data: Value = if path.exists() {
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

        if let Some(obj) = data.as_object_mut() {
            obj.insert(
                "deploymentMode".to_string(),
                Value::String("3p".to_string()),
            );
        }

        let content = serde_json::to_string_pretty(&data)?;
        let _ = fs::write(&path, content);
    }
    Ok(())
}

pub fn restore_1p_deployment_mode() -> AppResult<()> {
    for path in mcp_config_paths() {
        if path.exists() {
            let text = fs::read_to_string(&path)?;
            if let Ok(mut data) = serde_json::from_str::<Value>(&text) {
                let mut changed = false;
                if let Some(obj) = data.as_object_mut() {
                    obj.insert(
                        "deploymentMode".to_string(),
                        Value::String("1p".to_string()),
                    );
                    changed = true;
                }

                if changed {
                    let content = serde_json::to_string_pretty(&data)?;
                    let _ = fs::write(&path, content);
                }
            }
        }
    }
    Ok(())
}
