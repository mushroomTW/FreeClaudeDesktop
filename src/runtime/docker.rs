use std::{io, path::PathBuf, process::Command};

const COMPOSE_FILE_ENV: &str = "FREECLAUDE_COMPOSE_FILE";

/// 以 Docker Compose 管理本專案的本機 proxy。所有命令均透過參數陣列執行，
/// 不會將使用者輸入交給 shell 解譯。
pub fn install() -> io::Result<()> {
    compose(&["up", "--detach", "--build"])?;
    Ok(())
}

pub fn start() -> io::Result<()> {
    compose(&["up", "--detach"])?;
    Ok(())
}

pub fn stop() -> io::Result<()> {
    compose(&["stop"])?;
    Ok(())
}

pub fn uninstall(purge_image: bool) -> io::Result<()> {
    compose(&["down"])?;
    if purge_image {
        docker(&["image", "rm", "freeclaude-proxy:local"])?;
    }
    Ok(())
}

pub fn update() -> io::Result<()> {
    // 此映像檔由本機 Dockerfile 建置；重新建置可套用目前工作目錄中的版本。
    compose(&["up", "--detach", "--build"])?;
    Ok(())
}

pub fn status() -> io::Result<String> {
    let output = compose_output(&["ps", "--format", "json"])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn compose(args: &[&str]) -> io::Result<std::process::Output> {
    let output = compose_output(args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(command_failed("docker compose", &output))
    }
}

fn compose_output(args: &[&str]) -> io::Result<std::process::Output> {
    let compose_file = compose_file()?;
    let mut command = Command::new("docker");
    command.args(["compose", "--file"]);
    command.arg(compose_file);
    command.args(args);
    command.output()
}

fn docker(args: &[&str]) -> io::Result<std::process::Output> {
    let output = Command::new("docker").args(args).output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(command_failed("docker", &output))
    }
}

fn compose_file() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os(COMPOSE_FILE_ENV) {
        return Ok(PathBuf::from(path));
    }
    let candidate = std::env::current_dir()?.join("docker-compose.yml");
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "找不到 docker-compose.yml；請在專案目錄執行，或設定 FREECLAUDE_COMPOSE_FILE。",
        ))
    }
}

fn command_failed(command: &str, output: &std::process::Output) -> io::Error {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let detail = if detail.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        detail
    };
    io::Error::other(format!("{command} 執行失敗：{detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_failure_preserves_stderr() {
        let output = Command::new("rustc")
            .arg("--definitely-invalid-option")
            .output()
            .expect("應可執行 rustc");
        let error = command_failed("docker compose", &output);
        assert!(error.to_string().contains("docker compose"));
    }
}
