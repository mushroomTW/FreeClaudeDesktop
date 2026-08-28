#![allow(linker_messages)]

mod cli_args;
mod companion_daemon;
mod runtime;
mod update_check;

use std::time::Duration;
use std::io;

use clap::Parser;
use cli_args::{
    AutostartCommand, Cli, Command, HiddenRuntimeArgs, InstallArgs, PurgeArgs, UninstallArgs,
    UpdateArgs,
};
use serde_json::Value;

#[tokio::main]
/// 啟動程式並執行主要流程。
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Install(args) => install(args).await,
        Command::Start(args) => start(args).await,
        Command::Stop(args) => stop(args),
        Command::Status(args) => print_status(args).await,
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

/// 檢查已移除的 --runtime 參數，給予友善遷移錯誤而非 `unexpected argument`。
fn check_deprecated_runtime(runtime: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(v) = runtime {
        return Err(format!(
            "Docker runtime 已於 v1.x 移除，不再支援 --runtime {v}。請改用 native runtime（預設即 native，無需指定 --runtime）。若需清理舊容器，請手動執行 `docker compose down`。詳見 README。"
        )
        .into());
    }
    Ok(())
}

/// 對已移除的 Docker 相關環境變數給予警告，避免靜默忽略。
fn warn_deprecated_env_vars() {
    if std::env::var_os("FREECLAUDE_COMPOSE_FILE").is_some() {
        eprintln!("警告: FREECLAUDE_COMPOSE_FILE 已移除，Docker runtime 不再支援，此變數已被忽略。");
    }
    if std::env::var_os("FREECLAUDE_DOCKER_MEMORY_LIMIT").is_some() {
        eprintln!("警告: FREECLAUDE_DOCKER_MEMORY_LIMIT 已移除，Docker runtime 不再支援。");
    }
    if std::env::var_os("FREECLAUDE_DOCKER_MOCK").is_some() {
        eprintln!("警告: FREECLAUDE_DOCKER_MOCK 已移除。");
    }
}

/// 若偵測到舊的 compose 檔案，提示孤兒容器需手動清理。
fn warn_orphan_compose_file() {
    let compose_exists = std::env::var_os("FREECLAUDE_COMPOSE_FILE")
        .map(|p| std::path::Path::new(&p).is_file())
        .unwrap_or(false)
        || std::path::Path::new("compose.yaml").is_file()
        || std::path::Path::new("docker-compose.yml").is_file();
    if compose_exists {
        eprintln!(
            "提示: 偵測到舊的 compose.yaml / FREECLAUDE_COMPOSE_FILE，Docker runtime 已移除。若 3000 埠被佔用，舊容器可能仍在運行，請手動執行 `docker compose down`。"
        );
    }
}

/// 執行 `companion_daemon` 對應的處理流程。
async fn companion_daemon() -> Result<(), Box<dyn std::error::Error>> {
    crate::companion_daemon::companion_daemon().await
}

/// 執行 `install` 對應的處理流程。
async fn install(args: InstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    warn_deprecated_env_vars();
    let port = proxy_port()?;
    start_proxy().await?;
    free_claude_core::update_config_port(port)?;
    let _ = crate::runtime::native::start_companion(port);
    if !args.no_autostart {
        crate::runtime::autostart::enable()?;
    }
    println!("Native runtime 安裝完成");
    Ok(())
}

/// 執行 `start` 對應的處理流程。
async fn start(args: HiddenRuntimeArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    warn_deprecated_env_vars();
    let port = proxy_port()?;
    start_proxy().await?;
    let _ = crate::runtime::native::start_companion(port);
    Ok(())
}

/// 執行 `stop` 對應的處理流程。
fn stop(args: HiddenRuntimeArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    warn_deprecated_env_vars();
    warn_orphan_compose_file();
    let _ = crate::runtime::native::stop_companion();
    // 嘗試提示孤兒容器清理；不依賴 docker.rs，直接檢查埠佔用提示已足夠。
    stop_proxy()
}

/// 執行 `update` 對應的處理流程。
async fn update(args: UpdateArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    warn_deprecated_env_vars();
    let check = update_check::check_for_update().await?;
    println!("{}", serde_json::to_string_pretty(&check)?);
    if args.check || !check.update_available {
        return Ok(());
    }

    Err(
        "Native runtime 不支援直接覆寫執行檔；請由 release 頁面安裝新版本，或使用 `freeclaude update --check` 僅檢查更新。"
            .into(),
    )
}

fn uninstall(args: UninstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    if args.purge_image {
        return Err(
            "--purge-image 已隨 Docker runtime 移除，Docker image `freeclaude-proxy:local` 不再存在，無需清理。"
                .into(),
        );
    }
    // --yes 保留為隱藏相容參數，uninstall 本身不需確認，忽略即可。
    warn_deprecated_env_vars();
    warn_orphan_compose_file();
    let _ = crate::runtime::native::stop_companion();
    if let Err(error) = crate::runtime::native::stop_proxy()
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    let _ = crate::runtime::autostart::disable();
    free_claude_core::restore_official_config()?;
    free_claude_core::purge_application_data()?;

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
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .status()?;
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&url).status()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(&url).status()?;

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
    warn_deprecated_env_vars();
    Ok(std::env::var("FREECLAUDE_PROXY_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()?)
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
async fn print_status(args: HiddenRuntimeArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    warn_deprecated_env_vars();
    print_proxy_status().await
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
        let cli = Cli::try_parse_from(["freeclaude", "install"])
            .expect("install 應可解析");
        assert!(matches!(
            cli.command,
            Command::Install(InstallArgs {
                no_autostart: false,
                runtime: None
            })
        ));

        Cli::try_parse_from(["freeclaude", "install", "--no-autostart"])
            .expect("install --no-autostart 應可解析");

        Cli::try_parse_from(["freeclaude", "update", "--check"]).expect("update --check 應可解析");
        Cli::try_parse_from(["freeclaude", "uninstall"]).expect("uninstall 應可解析");
        Cli::try_parse_from(["freeclaude", "autostart", "enable"])
            .expect("autostart 子命令應可解析");
        Cli::try_parse_from(["freeclaude", "purge", "--yes"]).expect("purge 選項應可解析");
        Cli::try_parse_from(["freeclaude", "start"]).expect("start 應可解析");
        Cli::try_parse_from(["freeclaude", "stop"]).expect("stop 應可解析");
        Cli::try_parse_from(["freeclaude", "status"]).expect("status 應可解析");
    }

    #[test]
    /// 驗證已移除的 --runtime 參數會被解析並在執行時給予友善錯誤，而非 `unexpected argument`。
    fn deprecated_runtime_is_parsed_and_rejected() {
        let cli = Cli::try_parse_from(["freeclaude", "install", "--runtime", "docker"])
            .expect("--runtime 應被隱藏參數解析，而非報錯");
        if let Command::Install(args) = cli.command {
            assert_eq!(args.runtime.as_deref(), Some("docker"));
            let err = check_deprecated_runtime(args.runtime.as_ref()).unwrap_err();
            assert!(err.to_string().contains("Docker runtime 已於 v1.x 移除"));
        } else {
            panic!("應為 Install");
        }

        let cli = Cli::try_parse_from(["freeclaude", "start", "--runtime", "docker"])
            .expect("start --runtime 應被解析");
        if let Command::Start(args) = cli.command {
            assert!(check_deprecated_runtime(args.runtime.as_ref()).is_err());
        } else {
            panic!("應為 Start");
        }

        let cli = Cli::try_parse_from(["freeclaude", "uninstall", "--purge-image"])
            .expect("--purge-image 應被解析");
        if let Command::Uninstall(args) = cli.command {
            assert!(args.purge_image);
        } else {
            panic!("應為 Uninstall");
        }
    }

    #[test]
    /// 驗證 `uninstall_does_not_require_confirmation` 的行為符合預期。
    fn uninstall_does_not_require_confirmation() {
        Cli::try_parse_from(["freeclaude", "uninstall"]).expect("uninstall 不應要求 --yes");
        // 舊腳本的 --yes 仍相容（隱藏參數）
        Cli::try_parse_from(["freeclaude", "uninstall", "--yes"]).expect("--yes 應相容");
    }

    #[test]
    /// 驗證 `install_defaults_to_native_runtime` 的行為符合預期。
    fn install_defaults_to_native_runtime() {
        let args = InstallArgs {
            no_autostart: false,
            runtime: None,
        };
        assert!(!args.no_autostart);
        assert!(args.runtime.is_none());
    }

    #[test]
    /// 驗證 `detects_newer_three_part_versions` 的行為符合預期。
    fn detects_newer_three_part_versions() {
        assert!(update_check::version_is_newer("0.2.0", "0.1.9"));
        assert!(!update_check::version_is_newer("0.1.1", "0.1.1"));
        assert!(!update_check::version_is_newer("invalid", "0.1.1"));
    }
}
