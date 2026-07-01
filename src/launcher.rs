use serde_json::Value;
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
    Ok(target_path.to_path_buf())
}

pub fn write_config_to_all_paths(file_name: &str, content: &str) -> AppResult<()> {
    let standard_dir = config_lib_dir();
    fs::create_dir_all(&standard_dir)?;
    fs::write(standard_dir.join(file_name), content)?;

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
    Ok(())
}

fn powershell_output(script: &str) -> Option<String> {
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", script]);
    #[cfg(windows)]
    cmd.creation_flags(crate::constants::CREATE_NO_WINDOW);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn get_claude_appx_package_family_name() -> Option<String> {
    powershell_output(
        "Get-AppxPackage -Name *Claude* | Select-Object -ExpandProperty PackageFamilyName",
    )
}

fn get_claude_appx_application_id() -> String {
    powershell_output("$app = Get-AppxPackage -Name *Claude*; if ($app) { $manifestPath = Join-Path $app.InstallLocation 'AppxManifest.xml'; if (Test-Path $manifestPath) { [xml]$xml = Get-Content $manifestPath; $xml.Package.Applications.Application.Id } }")
        .unwrap_or_else(|| "Claude".to_string())
}

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

pub fn launch_claude(custom_path: Option<&Path>) -> AppResult<PathBuf> {
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

pub fn restore_official_config() -> AppResult<()> {
    let _ = fs::remove_dir_all(config_lib_dir());
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

pub fn claude_config(port: u16, _inference_models: &[crate::models::openai::InferenceModel]) -> Value {
    // Claude Desktop 3P 的 `inferenceGatewayModels`（格式 v2 schema）只接 5 個欄位
    // （name / labelOverride / supports1m / anthropicFamilyTier / isFamilyDefault），
    // 不含 max_input_tokens / max_tokens，所以若用物件覆寫會失去 context window
    // 的顯示。為了讓 desktop 顯示**真實 context window**，我們完全不寫
    // `inferenceGatewayModels` 這把鑰匙，改讓 desktop 主動 GET proxy 的
    // `/v1/models` discovery，那個 endpoint 會回 Anthropic 原生格式並帶
    // `max_input_tokens` / `max_tokens`（見 response_converter.rs）。
    serde_json::json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": format!("http://127.0.0.1:{}", port),
        "inferenceGatewayApiKey": crate::constants::PROXY_AUTH_TOKEN,
        "inferenceGatewayAuthScheme": "bearer",
        "modelDiscoveryEnabled": true
    })
}

pub fn update_applied_claude_config(port: u16, inference_models: &[crate::models::openai::InferenceModel]) {
    let content = serde_json::to_string_pretty(&claude_config(port, inference_models)).unwrap();
    let _ = write_config_to_all_paths(&format!("{CONFIG_ID}.json"), &content);
}

pub fn update_config_port(port: u16) -> AppResult<()> {
    let file_name = format!("{CONFIG_ID}.json");
    let path = config_lib_dir().join(&file_name);
    if path.exists() {
        let text = fs::read_to_string(&path)?;
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) {
            json["inferenceGatewayBaseUrl"] = serde_json::json!(format!("http://127.0.0.1:{port}"));
            let content = serde_json::to_string_pretty(&json)?;
            write_config_to_all_paths(&file_name, &content)?;
        }
    }
    Ok(())
}
