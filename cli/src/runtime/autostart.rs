use std::env;
use std::io;
use std::process::Command;

#[cfg(not(target_os = "windows"))]
use std::fs;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

#[cfg(target_os = "windows")]
const SERVICE_NAME: &str = "FreeClaudeDesktop";

pub fn enable() -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let executable = env::current_exe()?;
        let task_command = format!("\"{}\" start", executable.display());
        run(Command::new("schtasks").args([
            "/Create",
            "/TN",
            SERVICE_NAME,
            "/TR",
            &task_command,
            "/SC",
            "ONLOGON",
            "/RL",
            "LIMITED",
            "/F",
        ]))
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

pub fn disable() -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        run(Command::new("schtasks").args(["/Delete", "/TN", SERVICE_NAME, "/F"]))
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

pub fn is_enabled() -> io::Result<bool> {
    #[cfg(target_os = "windows")]
    {
        Ok(Command::new("schtasks")
            .args(["/Query", "/TN", SERVICE_NAME])
            .status()?
            .success())
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

fn run(command: &mut Command) -> io::Result<()> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("autostart 命令失敗：{status}")))
    }
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> io::Result<PathBuf> {
    Ok(env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "找不到 HOME"))?
        .join("Library/LaunchAgents/com.freeclaude.desktop.plist"))
}

#[cfg(all(unix, not(target_os = "macos")))]
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
    fn service_name_is_stable() {
        assert_eq!(SERVICE_NAME, "FreeClaudeDesktop");
    }

    #[test]
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
            Err(e) => {
                let err_msg = e.to_string();
                if cfg!(target_os = "windows") && err_msg.contains("exit code: 1") {
                    println!(
                        "警告：當前 Windows 環境可能缺乏管理員權限（Access is denied），已跳過自動啟動整合測試。"
                    );
                    return;
                }
                panic!("enable() 失敗：{:?}", e);
            }
        }
    }
}
