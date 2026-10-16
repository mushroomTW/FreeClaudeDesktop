use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use free_claude_core::platform::common::local_app_data;

/// 執行 `pid_file` 對應的處理流程。
pub fn pid_file() -> PathBuf {
    local_app_data().join("FreeClaudeDesktop").join("proxy.pid")
}

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// 啟動或執行 `start_proxy` 流程。
pub fn start_proxy(port: u16) -> io::Result<u32> {
    if let Some(parent) = pid_file().parent() {
        fs::create_dir_all(parent)?;
    }
    let executable = std::env::current_exe()?;
    let mut cmd = Command::new(executable);
    cmd.arg("runtime-daemon")
        .env("FREECLAUDE_PROXY_PORT", port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x00000008 | 0x00000200);

    let child = cmd.spawn()?;
    fs::write(pid_file(), child.id().to_string())?;
    Ok(child.id())
}

/// 停止或停用 `stop_proxy` 流程。
pub fn stop_proxy() -> io::Result<()> {
    let path = pid_file();
    let pid = fs::read_to_string(&path)?
        .trim()
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Proxy PID 檔內容無效"))?;

    #[cfg(target_os = "windows")]
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    #[cfg(not(target_os = "windows"))]
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()?;

    if !status.success() {
        return Err(io::Error::other(format!("停止 proxy process 失敗：{pid}")));
    }
    fs::remove_file(path)?;
    Ok(())
}

/// 執行 `companion_pid_file` 對應的處理流程。
pub fn companion_pid_file() -> PathBuf {
    local_app_data()
        .join("FreeClaudeDesktop")
        .join("companion.pid")
}

/// 停止或停用 `stop_companion` 流程。
pub fn stop_companion() -> io::Result<()> {
    let path = companion_pid_file();
    if !path.is_file() {
        return Ok(());
    }
    let pid = fs::read_to_string(&path)?
        .trim()
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Companion PID 檔內容無效"))?;

    #[cfg(target_os = "windows")]
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    #[cfg(not(target_os = "windows"))]
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()?;

    if !status.success() {
        return Err(io::Error::other(format!(
            "停止 companion process 失敗：{pid}"
        )));
    }
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 驗證 `pid_file_is_scoped_to_application_directory` 的行為符合預期。
    fn pid_file_is_scoped_to_application_directory() {
        assert!(pid_file().ends_with(PathBuf::from("FreeClaudeDesktop").join("proxy.pid")));
    }
}
