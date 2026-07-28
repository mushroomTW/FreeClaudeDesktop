#![allow(linker_messages)]

mod cli_args;
mod companion_daemon;
mod runtime;
mod update_check;

use std::time::Duration;
use std::{io, process::Command as ProcessCommand};

use clap::Parser;
use cli_args::*;
use serde_json::Value;

#[tokio::main]
/// 啟動程式並執行主要流程。
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Install(args) => install(args).await,
        Command::Start(args) => start(args.runtime).await,
        Command::Stop(args) => stop(args.runtime),
        Command::Status(args) => print_status(args.runtime).await,
        Command::Configure => open_dashboard(),
        Command::LaunchClaude => launch_claude(),
        Command::Restore => restore_settings(),
        Command::Purge(args) => purge(args),
        Command::Update(args) => update(args).await,
        Command::Uninstall(args) => uninstall(args),
        Command::Autostart { command } => manage_autostart(command),
        Command::CompanionDaemon => companion_daemon().await,
    }
}

/// 執行 `companion_daemon` 對應的處理流程。
async fn companion_daemon() -> Result<(), Box<dyn std::error::Error>> {
    crate::companion_daemon::companion_daemon().await
}

/// 執行 `install` 對應的處理流程。
async fn install(args: InstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    let port = proxy_port()?;
    if matches!(args.runtime, Runtime::Docker) {
        ensure_docker_default_port(port)?;
        crate::runtime::docker::install()?;
        free_claude_core::update_config_port(port)?;
        let _ = crate::runtime::native::start_companion(port);
        if !args.no_autostart {
            crate::runtime::autostart::enable()?;
        }
        println!("Docker runtime 已安裝並啟動。");
        return Ok(());
    }
    match args.runtime {
        Runtime::Native => {
            start_proxy().await?;
            free_claude_core::update_config_port(port)?;
            let _ = crate::runtime::native::start_companion(port);
            if !args.no_autostart {
                crate::runtime::autostart::enable()?;
            }
            println!("Native runtime 安裝完成");
            Ok(())
        }
        Runtime::Docker => Err("Docker install 尚需 Docker Compose v2 與 container 設定掛載；請使用 docker compose up --build".into()),
    }
}

/// 執行 `start` 對應的處理流程。
async fn start(runtime: Runtime) -> Result<(), Box<dyn std::error::Error>> {
    let port = proxy_port()?;
    match runtime {
        Runtime::Native => {
            start_proxy().await?;
            let _ = crate::runtime::native::start_companion(port);
            Ok(())
        }
        Runtime::Docker => {
            ensure_docker_default_port(port)?;
            crate::runtime::docker::start()?;
            let _ = crate::runtime::native::start_companion(port);
            println!("Docker proxy 已啟動。");
            Ok(())
        }
    }
}

/// 執行 `stop` 對應的處理流程。
fn stop(runtime: Runtime) -> Result<(), Box<dyn std::error::Error>> {
    let _ = crate::runtime::native::stop_companion();
    match runtime {
        Runtime::Native => stop_proxy(),
        Runtime::Docker => {
            crate::runtime::docker::stop()?;
            println!("Docker proxy 已停止。");
            Ok(())
        }
    }
}

/// 執行 `uninstall` 對應的處理流程。
async fn update(args: UpdateArgs) -> Result<(), Box<dyn std::error::Error>> {
    let check = update_check::check_for_update().await?;
    println!("{}", serde_json::to_string_pretty(&check)?);
    if args.check || !check.update_available {
        return Ok(());
    }

    match args.runtime {
        Runtime::Docker => {
            crate::runtime::docker::update()?;
            println!("Docker proxy 已更新，請確認服務重新啟動完成。");
            Ok(())
        }
        Runtime::Native => Err(
            "Native runtime 不支援直接覆寫執行檔；請由 release 頁面安裝新版本，或使用 `freeclaude update --check` 僅檢查更新。"
                .into(),
        ),
    }
}

fn uninstall(args: UninstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    let _ = crate::runtime::native::stop_companion();
    match args.runtime {
        Runtime::Native => {
            if let Err(error) = crate::runtime::native::stop_proxy()
                && error.kind() != io::ErrorKind::NotFound
            {
                return Err(error.into());
            }
            let _ = crate::runtime::autostart::disable();
        }
        Runtime::Docker => {
            crate::runtime::docker::uninstall(args.purge_image)?;
        }
    }
    let _ = crate::runtime::autostart::disable();
    free_claude_core::restore_official_config()?;
    free_claude_core::purge_application_data()?;

    if args.purge_image && matches!(args.runtime, Runtime::Native) {
        let status = ProcessCommand::new("docker")
            .args(["image", "rm", "freeclaude-proxy:local"])
            .status()?;
        if !status.success() {
            return Err("無法移除 Docker image".into());
        }
    }
    println!("FreeClaudeDesktop 已解除安裝並還原 Claude 設定");
    Ok(())
}

/// 執行 `manage_autostart` 對應的處理流程。
fn manage_autostart(command: AutostartCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        AutostartCommand::Enable => {
            crate::runtime::autostart::enable()?;
            println!("自動啟動已啟用");
        }
        AutostartCommand::Disable => {
            crate::runtime::autostart::disable()?;
            println!("自動啟動已停用");
        }
        AutostartCommand::Status => println!(
            "自動啟動：{}",
            if crate::runtime::autostart::is_enabled()? {
                "已啟用"
            } else {
                "未啟用"
            }
        ),
    }
    Ok(())
}

/// 啟動或執行 `open_dashboard` 流程。
fn open_dashboard() -> Result<(), Box<dyn std::error::Error>> {
    let port = free_claude_core::get_launcher_settings()
        .and_then(|settings| settings.desktop.active_port)
        .unwrap_or(3000);
    let url = format!("http://127.0.0.1:{port}/dashboard");

    println!("正在開啟 Web 控制台：{url}");

    #[cfg(target_os = "windows")]
    let status = ProcessCommand::new("cmd")
        .args(["/C", "start", "", &url])
        .status()?;
    #[cfg(target_os = "macos")]
    let status = ProcessCommand::new("open").arg(&url).status()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = ProcessCommand::new("xdg-open").arg(&url).status()?;

    if !status.success() {
        return Err(io::Error::other("無法開啟 Web 控制台").into());
    }
    Ok(())
}

/// 啟動或執行 `launch_claude` 流程。
fn launch_claude() -> Result<(), Box<dyn std::error::Error>> {
    let path = free_claude_core::launch_claude(None)?;
    println!("Claude 已啟動：{}", path.display());
    Ok(())
}

/// 清理或還原 `restore_settings` 所管理的資料。
fn restore_settings() -> Result<(), Box<dyn std::error::Error>> {
    free_claude_core::restore_official_config()?;
    println!("Claude 官方設定已還原");
    Ok(())
}

/// 執行 `purge` 對應的處理流程。
fn purge(args: PurgeArgs) -> Result<(), Box<dyn std::error::Error>> {
    if !args.yes {
        return Err("purge 會停止服務、還原 Claude 設定並刪除所有 FreeClaudeDesktop 資料；請加入 --yes 確認".into());
    }
    let _ = crate::runtime::native::stop_companion();
    if let Err(error) = crate::runtime::native::stop_proxy()
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    let _ = crate::runtime::autostart::disable();
    free_claude_core::purge_application_data()?;
    println!("FreeClaudeDesktop 的本機資料已完整清除");
    Ok(())
}

/// 執行 `proxy_port` 對應的處理流程。
fn proxy_port() -> Result<u16, Box<dyn std::error::Error>> {
    Ok(std::env::var("FREECLAUDE_PROXY_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()?)
}

/// Docker Compose 的容器與健康檢查目前固定使用 3000，避免寫入 Claude
/// 設定的連接埠與實際對外映射不一致。
fn ensure_docker_default_port(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    if port != 3000 {
        return Err("Docker runtime 目前只支援連接埠 3000；請移除 FREECLAUDE_PROXY_PORT，或改用 native runtime".into());
    }
    Ok(())
}

/// 啟動或執行 `start_proxy` 流程。
async fn start_proxy() -> Result<(), Box<dyn std::error::Error>> {
    let port = proxy_port()?;
    let healthz_url = format!("http://127.0.0.1:{port}/healthz");
    if proxy_is_healthy(&healthz_url).await {
        println!("Proxy 已在運作：{healthz_url}");
        return Ok(());
    }
    let pid = crate::runtime::native::start_proxy(port)?;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if proxy_is_healthy(&healthz_url).await {
            println!("Proxy 已啟動：PID {pid}，連接埠 {port}");
            return Ok(());
        }
    }

    let _ = crate::runtime::native::stop_proxy();
    Err("Proxy 未在 5 秒內通過健康檢查".into())
}

/// 停止或停用 `stop_proxy` 流程。
fn stop_proxy() -> Result<(), Box<dyn std::error::Error>> {
    crate::runtime::native::stop_proxy()?;
    println!("Proxy 已停止");
    Ok(())
}

/// 執行 `print_status` 對應的處理流程。
async fn print_status(runtime: Runtime) -> Result<(), Box<dyn std::error::Error>> {
    match runtime {
        Runtime::Native => print_proxy_status().await,
        Runtime::Docker => {
            let containers = crate::runtime::docker::status()?;
            println!(
                "{}",
                serde_json::json!({ "runtime": "docker", "containers": containers })
            );
            Ok(())
        }
    }
}

/// 執行 `print_proxy_status` 對應的處理流程。
async fn print_proxy_status() -> Result<(), Box<dyn std::error::Error>> {
    let proxy_url = std::env::var("FREECLAUDE_PROXY_URL").unwrap_or_else(|_| {
        let port = proxy_port().unwrap_or(3000);
        format!("http://127.0.0.1:{port}")
    });
    let healthz_url = format!("{}/healthz", proxy_url.trim_end_matches('/'));
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?
        .get(&healthz_url)
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await?;

    if !status.is_success() || body.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(format!("Proxy 健康檢查失敗：HTTP {status}，回應：{body}").into());
    }

    let pid = std::fs::read_to_string(crate::runtime::native::pid_file())
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok());
    let autostart = crate::runtime::autostart::is_enabled().unwrap_or(false);
    println!(
        "{}",
        serde_json::json!({
            "proxy": { "status": "ok", "healthz": healthz_url, "pid": pid },
            "autostart": autostart,
            "companion": { "endpoint": "/companion" },
        })
    );
    Ok(())
}

/// 執行 `proxy_is_healthy` 對應的處理流程。
async fn proxy_is_healthy(healthz_url: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    else {
        return false;
    };
    let Ok(response) = client.get(healthz_url).send().await else {
        return false;
    };
    response.status().is_success()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 驗證 `parses_documented_command_tree` 的行為符合預期。
    fn parses_documented_command_tree() {
        let cli = Cli::try_parse_from(["freeclaude", "install", "--runtime", "docker"])
            .expect("install 應可解析");
        assert!(matches!(
            cli.command,
            Command::Install(InstallArgs {
                runtime: Runtime::Docker,
                no_autostart: false
            })
        ));

        Cli::try_parse_from(["freeclaude", "update", "--check"]).expect("update --check 應可解析");
        Cli::try_parse_from(["freeclaude", "uninstall", "--purge-image", "--yes"])
            .expect("uninstall 選項應可解析");
        Cli::try_parse_from(["freeclaude", "autostart", "enable"])
            .expect("autostart 子命令應可解析");
        Cli::try_parse_from(["freeclaude", "purge", "--yes"]).expect("purge 選項應可解析");
    }

    #[test]
    /// 驗證 `uninstall_does_not_require_confirmation` 的行為符合預期。
    fn uninstall_does_not_require_confirmation() {
        Cli::try_parse_from(["freeclaude", "uninstall"]).expect("uninstall 不應要求 --yes");
    }

    #[test]
    /// 驗證 `install_defaults_to_native_runtime` 的行為符合預期。
    fn install_defaults_to_native_runtime() {
        let args = InstallArgs {
            runtime: Runtime::Native,
            no_autostart: false,
        };
        assert!(matches!(args.runtime, Runtime::Native));
    }

    #[test]
    /// 驗證 `detects_newer_three_part_versions` 的行為符合預期。
    fn detects_newer_three_part_versions() {
        assert!(update_check::version_is_newer("0.2.0", "0.1.9"));
        assert!(!update_check::version_is_newer("0.1.1", "0.1.1"));
        assert!(!update_check::version_is_newer("invalid", "0.1.1"));
    }

    #[test]
    /// 驗證 `docker_runtime_rejects_non_default_proxy_port` 的行為符合預期。
    fn docker_runtime_rejects_non_default_proxy_port() {
        assert!(ensure_docker_default_port(3000).is_ok());
        assert!(ensure_docker_default_port(3001).is_err());
    }
}
