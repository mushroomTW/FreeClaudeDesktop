use std::env;
use std::path::PathBuf;

/// 獲取本機的 Local AppData 目錄路徑，支援 Windows, macOS 與 Linux。
#[cfg(target_os = "windows")]
pub fn local_app_data() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
pub fn local_app_data() -> PathBuf {
    env::var_os("HOME")
        .map(|p| PathBuf::from(p).join("Library").join("Application Support"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn local_app_data() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 偵測作業系統目前是否開啟深色主題 (Dark Mode)
#[cfg(target_os = "windows")]
pub fn is_system_dark_mode() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // 0x08000000 = CREATE_NO_WINDOW (隱藏主控台視窗)
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .creation_flags(0x08000000)
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        // AppsUseLightTheme為 0x0 時表示系統使用深色主題
        if stdout.contains("0x0") {
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
pub fn is_system_dark_mode() -> bool {
    use std::process::Command;
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.trim().eq_ignore_ascii_case("Dark") {
            return true;
        }
    }
    false
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn is_system_dark_mode() -> bool {
    use std::process::Command;
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("dark") {
            return true;
        }
    }
    false
}
