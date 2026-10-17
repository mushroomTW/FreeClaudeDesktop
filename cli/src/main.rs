#![allow(linker_messages)]

mod cli_args;
mod companion_daemon;
mod runtime;
mod runtime_daemon;
mod update_check;

use std::io;
use std::time::Duration;

use clap::Parser;
use cli_args::{
    AutostartCommand, Cli, Command, HiddenRuntimeArgs, InstallArgs, PurgeArgs, UninstallArgs,
    UpdateArgs,
};
use serde_json::Value;

/// 啟動程式並執行主要流程。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    free_claude_core::runtime::build_runtime()?.block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
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
        Command::RuntimeDaemon => runtime_daemon::run().await,
    }
}

/// 檢查已移除的 --runtime 參數，給予友善遷移錯誤而非 `unexpected argument`。
fn check_deprecated_runtime(runtime: Option<&String>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(v) = runtime {
        return Err(format!(
            "The Docker runtime was removed in v1.x, so --runtime {v} is no longer supported. Use the native runtime instead (native is the default, so --runtime is unnecessary). To clean up old containers, run `docker compose down` manually. See the README for details."
        )
        .into());
    }
    Ok(())
}

/// 對已移除的 Docker 相關環境變數給予警告，避免靜默忽略。
fn warn_deprecated_env_vars() {
    if std::env::var_os("FREECLAUDE_COMPOSE_FILE").is_some() {
        eprintln!(
            "Warning: FREECLAUDE_COMPOSE_FILE has been removed; the Docker runtime is no longer supported and this variable is ignored."
        );
    }
    if std::env::var_os("FREECLAUDE_DOCKER_MEMORY_LIMIT").is_some() {
        eprintln!(
            "Warning: FREECLAUDE_DOCKER_MEMORY_LIMIT has been removed; the Docker runtime is no longer supported."
        );
    }
    if std::env::var_os("FREECLAUDE_DOCKER_MOCK").is_some() {
        eprintln!("Warning: FREECLAUDE_DOCKER_MOCK has been removed.");
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
            "Note: found a legacy compose.yaml / FREECLAUDE_COMPOSE_FILE, but the Docker runtime has been removed. If port 3000 is in use, an old container may still be running; run `docker compose down` manually."
        );
    }
}

/// 執行 `install` 對應的處理流程。
async fn install(args: InstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    warn_deprecated_env_vars();
    let port = proxy_port()?;
    ensure_launcher_settings(port)?;
    start_proxy().await?;
    free_claude_core::update_config_port(port)?;
    if !args.no_autostart {
        crate::runtime::autostart::enable()?;
    }
    println!("Native runtime installed");
    Ok(())
}

/// 執行 `start` 對應的處理流程。
async fn start(args: HiddenRuntimeArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    warn_deprecated_env_vars();
    let port = proxy_port()?;
    ensure_launcher_settings(port)?;
    start_proxy().await?;
    Ok(())
}

/// 初始化受保護 loopback Proxy 必要的設定與 token。
fn ensure_launcher_settings(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let mut settings = free_claude_core::get_launcher_settings().unwrap_or_default();
    if settings.gateway.proxy_auth_token.trim().is_empty()
        || settings.gateway.proxy_auth_token == free_claude_core::constants::PROXY_AUTH_TOKEN
    {
        settings.gateway.proxy_auth_token = free_claude_core::generate_proxy_auth_token()?;
    }
    settings.desktop.active_port = Some(port);
    free_claude_core::save_launcher_settings(&settings)?;
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
        "The native runtime cannot overwrite the executable in place; install the new version from the release page, or use `freeclaude update --check` to only check for updates."
            .into(),
    )
}

fn uninstall(args: UninstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_deprecated_runtime(args.runtime.as_ref())?;
    if args.purge_image {
        return Err(
            "--purge-image was removed together with the Docker runtime; the `freeclaude-proxy:local` image no longer exists, so there is nothing to clean up."
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

    println!("FreeClaudeDesktop uninstalled and Claude settings restored");
    Ok(())
}

/// 執行 `manage_autostart` 對應的處理流程。
fn manage_autostart(command: AutostartCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        AutostartCommand::Enable => {
            crate::runtime::autostart::enable()?;
            println!("Autostart enabled");
        }
        AutostartCommand::Disable => {
            crate::runtime::autostart::disable()?;
            println!("Autostart disabled");
        }
        AutostartCommand::Status => println!(
            "Autostart: {}",
            if crate::runtime::autostart::is_enabled()? {
                "enabled"
            } else {
                "disabled"
            }
        ),
    }
    Ok(())
}

/// 啟動或執行 `open_dashboard` 流程。
fn open_dashboard() -> Result<(), Box<dyn std::error::Error>> {
    let settings = free_claude_core::get_launcher_settings()
        .ok_or("Gateway is not configured; cannot open the protected dashboard")?;
    let port = settings.desktop.active_port.unwrap_or(3000);
    let token = settings.gateway.proxy_auth_token.trim();
    if token.is_empty() {
        return Err("Proxy auth token must not be empty".into());
    }
    let encoded_token: String = url::form_urlencoded::byte_serialize(token.as_bytes()).collect();
    let url = format!("http://127.0.0.1:{port}/dashboard#{encoded_token}");

    println!("Opening the web dashboard: http://127.0.0.1:{port}/dashboard");

    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .status()?;
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&url).status()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(&url).status()?;

    if !status.success() {
        return Err(io::Error::other("Failed to open the web dashboard").into());
    }
    Ok(())
}

/// 啟動或執行 `launch_claude` 流程。
fn launch_claude() -> Result<(), Box<dyn std::error::Error>> {
    let path = free_claude_core::launch_claude(None)?;
    println!("Claude launched: {}", path.display());
    Ok(())
}

/// 清理或還原 `restore_settings` 所管理的資料。
fn restore_settings() -> Result<(), Box<dyn std::error::Error>> {
    free_claude_core::restore_official_config()?;
    println!("Official Claude settings restored");
    Ok(())
}

/// 執行 `purge` 對應的處理流程。
fn purge(args: PurgeArgs) -> Result<(), Box<dyn std::error::Error>> {
    if !args.yes {
        return Err("purge stops services, restores Claude settings, and deletes all FreeClaudeDesktop data; add --yes to confirm".into());
    }
    let _ = crate::runtime::native::stop_companion();
    if let Err(error) = crate::runtime::native::stop_proxy()
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    let _ = crate::runtime::autostart::disable();
    free_claude_core::purge_application_data()?;
    println!("All local FreeClaudeDesktop data has been erased");
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
    if crate::runtime::native::companion_pid_file().is_file() {
        return Err(
            "Detected an older Companion process; run `freeclaude stop` before starting.".into(),
        );
    }
    if proxy_is_healthy(&healthz_url).await {
        println!("Proxy is already running: {healthz_url}");
        return Ok(());
    }
    let pid = crate::runtime::native::start_proxy(port)?;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if proxy_is_healthy(&healthz_url).await {
            println!("Proxy started: PID {pid}, port {port}");
            return Ok(());
        }
    }

    let _ = crate::runtime::native::stop_proxy();
    Err("Proxy did not pass the health check within 5 seconds".into())
}

/// 停止或停用 `stop_proxy` 流程。
fn stop_proxy() -> Result<(), Box<dyn std::error::Error>> {
    crate::runtime::native::stop_proxy()?;
    println!("Proxy stopped");
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
        return Err(format!("Proxy health check failed: HTTP {status}, response: {body}").into());
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
        let cli = Cli::try_parse_from(["freeclaude", "install"]).expect("install 應可解析");
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
            assert!(
                err.to_string()
                    .contains("The Docker runtime was removed in v1.x")
            );
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
