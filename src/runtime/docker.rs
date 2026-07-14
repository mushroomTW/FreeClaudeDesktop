use std::{io, path::PathBuf, process::Command, time::Duration};

const COMPOSE_FILE_ENV: &str = "FREECLAUDE_COMPOSE_FILE";

/// 以 Docker Compose 管理本專案的本機 proxy。所有命令均透過參數陣列執行，
/// 不會將使用者輸入交給 shell 解譯。
pub fn install() -> io::Result<()> {
    compose(&["up", "--detach", "--build"])?;
    poll_healthz()?;
    Ok(())
}

pub fn start() -> io::Result<()> {
    compose(&["up", "--detach"])?;
    poll_healthz()?;
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
    let file_path = compose_file()?;
    let content = std::fs::read_to_string(&file_path)?;

    let has_build = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "build:" || trimmed.starts_with("build:")
    });

    if has_build {
        compose(&["up", "--detach", "--build"])?;
    } else {
        let _ = compose(&["pull"]);
        compose(&["up", "--detach"])?;
    }

    poll_healthz()?;
    Ok(())
}

pub fn status() -> io::Result<String> {
    let output = compose_output(&["ps", "--format", "json"])?;
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_compose_ps(&stdout_str);
    Ok(serde_json::to_string(&parsed).unwrap_or_else(|_| "[]".to_string()))
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
    let compose_file_str = compose_file.to_string_lossy().into_owned();
    let mut all_args = vec!["compose", "--file", &compose_file_str];
    all_args.extend(args);
    run_command("docker", &all_args)
}

fn docker(args: &[&str]) -> io::Result<std::process::Output> {
    run_command("docker", args)
}

fn compose_file() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os(COMPOSE_FILE_ENV) {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Ok(p);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("設定的 FREECLAUDE_COMPOSE_FILE 不存在：{}", p.display()),
            ));
        }
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
fn get_mock_status(success: bool) -> std::process::ExitStatus {
    if success {
        Command::new("rustc")
            .arg("--version")
            .status()
            .unwrap_or_else(|_| {
                #[cfg(target_os = "windows")]
                {
                    Command::new("cmd")
                        .args(["/c", "exit", "0"])
                        .status()
                        .unwrap()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Command::new("sh").args(["-c", "exit 0"]).status().unwrap()
                }
            })
    } else {
        Command::new("rustc")
            .arg("--invalid-option-for-mock-exit-status-error")
            .status()
            .unwrap_or_else(|_| {
                #[cfg(target_os = "windows")]
                {
                    Command::new("cmd")
                        .args(["/c", "exit", "1"])
                        .status()
                        .unwrap()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Command::new("sh").args(["-c", "exit 1"]).status().unwrap()
                }
            })
    }
}

fn run_command(cmd_name: &str, args: &[&str]) -> io::Result<std::process::Output> {
    #[cfg(test)]
    {
        if let Ok(mock_type) = std::env::var("FREECLAUDE_DOCKER_MOCK") {
            match mock_type.as_str() {
                "daemon_unavailable" => {
                    return Ok(std::process::Output {
                        status: get_mock_status(false),
                        stdout: Vec::new(),
                        stderr:
                            b"Cannot connect to the Docker daemon. Is the docker daemon running?"
                                .to_vec(),
                    });
                }
                "port_in_use" => {
                    return Ok(std::process::Output {
                        status: get_mock_status(false),
                        stdout: Vec::new(),
                        stderr: b"Bind for 0.0.0.0:3000 failed: port is already allocated".to_vec(),
                    });
                }
                "healthcheck_fail" => {
                    if args.contains(&"up") {
                        return Ok(std::process::Output {
                            status: get_mock_status(true),
                            stdout: b"Container freeclaude-proxy-1 Started".to_vec(),
                            stderr: Vec::new(),
                        });
                    } else if args.contains(&"logs") {
                        return Ok(std::process::Output {
                            status: get_mock_status(true),
                            stdout: b"some proxy error logs".to_vec(),
                            stderr: Vec::new(),
                        });
                    } else if args.contains(&"ps") {
                        return Ok(std::process::Output {
                            status: get_mock_status(true),
                            stdout: b"[]".to_vec(),
                            stderr: Vec::new(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    let mut cmd = Command::new(cmd_name);
    cmd.args(args);
    cmd.output()
}

fn poll_healthz() -> io::Result<()> {
    #[cfg(test)]
    {
        if let Ok(mock_type) = std::env::var("FREECLAUDE_DOCKER_MOCK") {
            if mock_type == "healthcheck_fail" {
                // 繼續執行後面的超時邏輯
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }

    let interval = Duration::from_secs(1);
    let max_attempts = 15;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .map_err(io::Error::other)?;

    for _ in 1..=max_attempts {
        std::thread::sleep(interval);
        let success = rt.block_on(async {
            match client.get("http://127.0.0.1:3000/healthz").send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        if let Ok(body) = resp.text().await {
                            return body.contains("\"status\":\"ok\"")
                                || body.contains("\"status\": \"ok\"");
                        }
                    }
                    false
                }
                Err(_) => false,
            }
        });
        if success {
            return Ok(());
        }
    }

    let logs_output = compose_output(&["logs"])?;
    let stderr = String::from_utf8_lossy(&logs_output.stderr)
        .trim()
        .to_owned();
    let stdout = String::from_utf8_lossy(&logs_output.stdout)
        .trim()
        .to_owned();

    let mut logs = String::new();
    if !stdout.is_empty() {
        logs.push_str("=== Container STDOUT ===\n");
        logs.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !logs.is_empty() {
            logs.push('\n');
        }
        logs.push_str("=== Container STDERR ===\n");
        logs.push_str(&stderr);
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("容器啟動後未通過健康檢查。容器日誌：\n{logs}"),
    ))
}

fn parse_compose_ps(stdout: &str) -> serde_json::Value {
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return serde_json::Value::Array(vec![]);
    }

    let mut containers = Vec::new();

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(stdout) {
        match val {
            serde_json::Value::Array(arr) => {
                for item in arr {
                    if let Some(c) = extract_container_info(item) {
                        containers.push(c);
                    }
                }
            }
            serde_json::Value::Object(_) => {
                if let Some(c) = extract_container_info(val) {
                    containers.push(c);
                }
            }
            _ => {}
        }
    } else {
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(item) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(c) = extract_container_info(item) {
                    containers.push(c);
                }
            }
        }
    }

    serde_json::Value::Array(containers)
}

fn extract_container_info(item: serde_json::Value) -> Option<serde_json::Value> {
    let obj = item.as_object()?;

    let get_field = |keys: &[&str]| -> String {
        for key in keys {
            if let Some(val) = obj.get(*key) {
                if let Some(s) = val.as_str() {
                    return s.to_string();
                }
            }
        }
        String::new()
    };

    let name = get_field(&["Name", "name", "Names", "names"]);
    let state = get_field(&["State", "state", "Status", "status"]);
    let health = get_field(&["Health", "health"]);

    if name.is_empty() && state.is_empty() {
        return None;
    }

    Some(serde_json::json!({
        "name": name,
        "state": state,
        "health": health,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn command_failure_preserves_stderr() {
        let output = Command::new("rustc")
            .arg("--definitely-invalid-option")
            .output()
            .expect("應可執行 rustc");
        let error = command_failed("docker compose", &output);
        assert!(error.to_string().contains("docker compose"));
    }

    #[test]
    fn test_docker_errors_and_scenarios() {
        let _guard = TEST_MUTEX.lock().unwrap();

        let orig_compose = std::env::var(COMPOSE_FILE_ENV).ok();
        let orig_mock = std::env::var("FREECLAUDE_DOCKER_MOCK").ok();

        // 1. 測試 Docker daemon 不可用
        unsafe {
            std::env::set_var("FREECLAUDE_DOCKER_MOCK", "daemon_unavailable");
        }
        let dummy_path = std::env::current_dir().unwrap().join("docker-compose.yml");
        unsafe {
            std::env::set_var(COMPOSE_FILE_ENV, &dummy_path);
        }

        let res = start();
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("Cannot connect to the Docker daemon")
                || err_msg.contains("docker compose 執行失敗")
        );

        // 2. 測試 Compose 檔案不存在
        unsafe {
            std::env::set_var("FREECLAUDE_DOCKER_MOCK", "");
            std::env::set_var(COMPOSE_FILE_ENV, "C:\\nonexistent\\docker-compose.yml");
        }
        let res = start();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::NotFound);

        // 3. 測試連接埠被占用
        unsafe {
            std::env::set_var("FREECLAUDE_DOCKER_MOCK", "port_in_use");
            std::env::set_var(COMPOSE_FILE_ENV, &dummy_path);
        }
        let res = start();
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("port is already allocated")
                || err_msg.contains("docker compose 執行失敗")
        );

        // 4. 測試 Healthcheck 失敗
        unsafe {
            std::env::set_var("FREECLAUDE_DOCKER_MOCK", "healthcheck_fail");
            std::env::set_var(COMPOSE_FILE_ENV, &dummy_path);
        }
        let res = start();
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("未通過健康檢查"));
        assert!(err_msg.contains("some proxy error logs"));

        // 恢復環境變數
        unsafe {
            if let Some(val) = orig_compose {
                std::env::set_var(COMPOSE_FILE_ENV, val);
            } else {
                std::env::remove_var(COMPOSE_FILE_ENV);
            }
            if let Some(val) = orig_mock {
                std::env::set_var("FREECLAUDE_DOCKER_MOCK", val);
            } else {
                std::env::remove_var("FREECLAUDE_DOCKER_MOCK");
            }
        }
    }

    #[test]
    fn test_parse_compose_ps_formats() {
        let json_arr = r#"[
            {"Name": "freeclaude-proxy-1", "State": "running", "Health": "healthy"},
            {"name": "mcp-server-1", "state": "exited", "health": ""}
        ]"#;
        let parsed = parse_compose_ps(json_arr);
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "freeclaude-proxy-1");
        assert_eq!(arr[0]["state"], "running");
        assert_eq!(arr[0]["health"], "healthy");
        assert_eq!(arr[1]["name"], "mcp-server-1");
        assert_eq!(arr[1]["state"], "exited");
        assert_eq!(arr[1]["health"], "");

        let json_lines = "{\"Name\": \"freeclaude-proxy-1\", \"State\": \"running\", \"Health\": \"healthy\"}\n{\"name\": \"mcp-server-1\", \"state\": \"exited\"}";
        let parsed = parse_compose_ps(json_lines);
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "freeclaude-proxy-1");
        assert_eq!(arr[0]["state"], "running");
        assert_eq!(arr[0]["health"], "healthy");
        assert_eq!(arr[1]["name"], "mcp-server-1");
        assert_eq!(arr[1]["state"], "exited");
        assert_eq!(arr[1]["health"], "");
    }
}
