use std::env;
use std::io;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::process::Stdio;

#[cfg(not(target_os = "windows"))]
use std::fs;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const RUN_VALUE: &str = "FreeClaudeDesktop";
#[cfg(target_os = "windows")]
const LEGACY_TASK_NAME: &str = "FreeClaudeDesktop";

/// 執行 `enable` 對應的處理流程。
pub fn enable() -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let command_line = windows_start_command()?;
        run_silently(Command::new("reg.exe").args([
            "ADD",
            RUN_KEY,
            "/v",
            RUN_VALUE,
            "/t",
            "REG_SZ",
            "/d",
            command_line.as_str(),
            "/f",
        ]))?;
        remove_legacy_task();
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let executable = env::current_exe()?;
        let path = launch_agent_path()?;
        fs::create_dir_all(path.parent().expect("LaunchAgent 目錄"))?;
        fs::write(
            &path,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>com.freeclaude.desktop</string><key>ProgramArguments</key><array><string>{}</string><string>start</string></array><key>RunAtLoad</key><true/></dict></plist>",
                executable.display()
            ),
        )?;
        run(Command::new("launchctl").args(["load", "-w", path.to_string_lossy().as_ref()]))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let executable = env::current_exe()?;
        let path = systemd_unit_path()?;
        fs::create_dir_all(path.parent().expect("systemd user 目錄"))?;
        fs::write(
            &path,
            format!(
                "[Unit]\nDescription=FreeClaudeDesktop proxy\n\n[Service]\nType=simple\nExecStart={} start\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
                executable.display()
            ),
        )?;
        run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
        run(Command::new("systemctl").args(["--user", "enable", "--now", "freeclaude.service"]))
    }
}

/// 執行 `disable` 對應的處理流程。
pub fn disable() -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        if registry_autostart_exists()? {
            run_silently(Command::new("reg.exe").args(["DELETE", RUN_KEY, "/v", RUN_VALUE, "/f"]))?;
        }
        remove_legacy_task();
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let path = launch_agent_path()?;
        if path.exists() {
            run(Command::new("launchctl").args(["unload", "-w", path.to_string_lossy().as_ref()]))?;
            fs::remove_file(path)?;
        }
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let path = systemd_unit_path()?;
        let _ = run(Command::new("systemctl").args([
            "--user",
            "disable",
            "--now",
            "freeclaude.service",
        ]));
        if path.exists() {
            fs::remove_file(path)?;
        }
        run(Command::new("systemctl").args(["--user", "daemon-reload"]))
    }
}

/// 判斷是否符合 `is_enabled` 的條件。
pub fn is_enabled() -> io::Result<bool> {
    #[cfg(target_os = "windows")]
    {
        registry_autostart_exists()
    }
    #[cfg(target_os = "macos")]
    {
        Ok(launch_agent_path()?.exists())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Ok(systemd_unit_path()?.exists())
    }
}

/// 執行 `run` 對應的處理流程。
fn run(command: &mut Command) -> io::Result<()> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("autostart 命令失敗：{status}")))
    }
}

#[cfg(target_os = "windows")]
/// 建立目前使用者登入時要執行的 Windows 命令列。
fn windows_start_command() -> io::Result<String> {
    let executable = env::current_exe()?;
    let command_line = format!("\"{}\" start", executable.display());
    if command_line.chars().count() > 260 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows Registry 自動啟動命令列不可超過 260 個字元",
        ));
    }
    Ok(command_line)
}

#[cfg(target_os = "windows")]
/// 執行 Windows 命令並隱藏不影響結果的輸出。
fn run_silently(command: &mut Command) -> io::Result<()> {
    command.stdout(Stdio::null()).stderr(Stdio::null());
    run(command)
}

#[cfg(target_os = "windows")]
/// 判斷目前使用者的 Registry 自動啟動值是否存在。
fn registry_autostart_exists() -> io::Result<bool> {
    Ok(Command::new("reg.exe")
        .args(["QUERY", RUN_KEY, "/v", RUN_VALUE])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

#[cfg(target_os = "windows")]
/// 嘗試移除舊版 Task Scheduler 自動啟動項目。
fn remove_legacy_task() {
    let _ = Command::new("schtasks.exe")
        .args(["/Delete", "/TN", LEGACY_TASK_NAME, "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(target_os = "macos")]
/// 啟動或執行 `launch_agent_path` 流程。
fn launch_agent_path() -> io::Result<PathBuf> {
    Ok(env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "找不到 HOME"))?
        .join("Library/LaunchAgents/com.freeclaude.desktop.plist"))
}

#[cfg(all(unix, not(target_os = "macos")))]
/// 執行 `systemd_unit_path` 對應的處理流程。
fn systemd_unit_path() -> io::Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "找不到使用者設定目錄"))?;
    Ok(base.join("systemd/user/freeclaude.service"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AutostartTestGuard {
        was_enabled: bool,
    }

    impl Drop for AutostartTestGuard {
        /// 執行 `drop` 對應的處理流程。
        fn drop(&mut self) {
            if self.was_enabled {
                let _ = enable();
            } else {
                let _ = disable();
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    /// 驗證 Windows Registry 自動啟動位置符合預期。
    fn registry_autostart_location_is_stable() {
        assert_eq!(
            RUN_KEY,
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"
        );
        assert_eq!(RUN_VALUE, "FreeClaudeDesktop");
    }

    #[test]
    #[ignore = "會修改使用者的自動啟動設定，請在本機手動執行"]
    /// 驗證 `test_autostart_integration` 的行為符合預期。
    fn test_autostart_integration() {
        let was_enabled = is_enabled().unwrap_or(false);
        let _guard = AutostartTestGuard { was_enabled };

        match enable() {
            Ok(_) => {
                assert!(
                    is_enabled().expect("is_enabled() 應該要成功"),
                    "enable() 之後 is_enabled() 應為 true"
                );

                disable().expect("disable() 應該要成功");
                assert!(
                    !is_enabled().expect("is_enabled() 應該要成功"),
                    "disable() 之後 is_enabled() 應為 false"
                );
            }
            Err(e) => panic!("enable() 失敗：{:?}", e),
        }
    }
}
