use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use free_claude_core::platform::common::local_app_data;

pub fn pid_file() -> PathBuf {
    local_app_data().join("FreeClaudeDesktop").join("proxy.pid")
}

pub fn proxy_binary_path() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or_else(|| io::Error::other("無法判斷 CLI 所在目錄"))?;
    let name = if cfg!(target_os = "windows") {
        "freeclaude-proxy.exe"
    } else {
        "freeclaude-proxy"
    };
    Ok(directory.join(name))
}

pub fn start_proxy(port: u16) -> io::Result<u32> {
    if let Some(parent) = pid_file().parent() {
        fs::create_dir_all(parent)?;
    }
    let binary = proxy_binary_path()?;
    if !binary.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("找不到 proxy binary：{}", binary.display()),
        ));
    }

    let child = Command::new(binary)
        .env("FREECLAUDE_PROXY_PORT", port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    fs::write(pid_file(), child.id().to_string())?;
    Ok(child.id())
}

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

pub fn companion_pid_file() -> PathBuf {
    local_app_data().join("FreeClaudeDesktop").join("companion.pid")
}

pub fn start_companion(port: u16) -> io::Result<u32> {
    if let Some(parent) = companion_pid_file().parent() {
        fs::create_dir_all(parent)?;
    }
    let binary = std::env::current_exe()?;
    let child = Command::new(binary)
        .arg("companion-daemon")
        .env("FREECLAUDE_PROXY_PORT", port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    fs::write(companion_pid_file(), child.id().to_string())?;
    Ok(child.id())
}

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
        return Err(io::Error::other(format!("停止 companion process 失敗：{pid}")));
    }
    fs::remove_file(path)?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_is_scoped_to_application_directory() {
        assert!(pid_file().ends_with(PathBuf::from("FreeClaudeDesktop").join("proxy.pid")));
    }
}
