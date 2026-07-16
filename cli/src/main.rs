mod companion_daemon;
mod runtime;

use std::time::Duration;
use std::{io, process::Command as ProcessCommand};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::Value;

#[derive(Debug, Parser)]
#[command(name = "freeclaude", about = "FreeClaudeDesktop 本機代理管理工具")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Install(InstallArgs),
    Start(RuntimeArgs),
    Stop(RuntimeArgs),
    Status(RuntimeArgs),
    Configure,
    #[command(name = "launch-claude")]
    LaunchClaude,
    Restore,
    Purge(PurgeArgs),
    Update(UpdateArgs),
    Uninstall(UninstallArgs),
    Autostart {
        #[command(subcommand)]
        command: AutostartCommand,
    },
    #[command(hide = true)]
    CompanionDaemon,
}

#[derive(Debug, Args)]
struct InstallArgs {
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    runtime: Runtime,
    #[arg(long)]
    no_autostart: bool,
}

#[derive(Debug, Args)]
struct RuntimeArgs {
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    runtime: Runtime,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Runtime {
    Native,
    Docker,
}

#[derive(Debug, Args)]
struct UpdateArgs {
    #[arg(long)]
    check: bool,
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    runtime: Runtime,
}

#[derive(Debug, Args)]
struct UninstallArgs {
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    runtime: Runtime,
    #[arg(long)]
    purge_image: bool,
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
struct PurgeArgs {
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Subcommand)]
enum AutostartCommand {
    Enable,
    Disable,
    Status,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Install(args) => install(args).await,
        Command::Start(args) => start(args.runtime).await,
        Command::Stop(args) => stop(args.runtime),
        Command::Status(args) => print_status(args.runtime).await,
        Command::Configure => open_admin(),
        Command::LaunchClaude => launch_claude(),
        Command::Restore => restore_settings(),
        Command::Purge(args) => purge(args),
        Command::Update(args) => update(args).await,
        Command::Uninstall(args) => uninstall(args),
        Command::Autostart { command } => manage_autostart(command),
        Command::CompanionDaemon => companion_daemon().await,
    }
}

async fn companion_daemon() -> Result<(), Box<dyn std::error::Error>> {
    crate::companion_daemon::companion_daemon().await
}

async fn install(args: InstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    let port = proxy_port()?;
    if matches!(args.runtime, Runtime::Docker) {
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

async fn start(runtime: Runtime) -> Result<(), Box<dyn std::error::Error>> {
    let port = proxy_port()?;
    match runtime {
        Runtime::Native => {
            start_proxy().await?;
            let _ = crate::runtime::native::start_companion(port);
            Ok(())
        }
        Runtime::Docker => {
            crate::runtime::docker::start()?;
            let _ = crate::runtime::native::start_companion(port);
            println!("Docker proxy 已啟動。");
            Ok(())
        }
    }
}

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

async fn update(args: UpdateArgs) -> Result<(), Box<dyn std::error::Error>> {
    let check = check_for_update().await?;
    println!("{}", serde_json::to_string_pretty(&check)?);
    if args.check || !check.update_available {
        return Ok(());
    }
    match args.runtime {
        Runtime::Docker => {
            crate::runtime::docker::update()?;
            println!("Docker proxy 已使用目前本機來源重新建置。");
            Ok(())
        }
        Runtime::Native => Err("為避免覆蓋執行中的原生執行檔，請下載對應 release 資產後重新執行 `freeclaude update --check` 驗證版本。".into()),
    }
}

#[derive(Debug, serde::Serialize)]
struct UpdateCheck {
    current_version: &'static str,
    latest_version: String,
    update_available: bool,
    release_url: String,
}

async fn check_for_update() -> Result<UpdateCheck, Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
    }

    let release = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("freeclaude-cli")
        .build()?
        .get("https://api.github.com/repos/mushroomTW/FreeClaudeDesktop/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json::<Release>()
        .await?;
    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    Ok(UpdateCheck {
        current_version: env!("CARGO_PKG_VERSION"),
        update_available: version_is_newer(&latest_version, env!("CARGO_PKG_VERSION")),
        latest_version,
        release_url: release.html_url,
    })
}

fn version_is_newer(candidate: &str, current: &str) -> bool {
    fn parts(value: &str) -> Option<[u64; 3]> {
        let mut parts = value.split('.').map(str::parse::<u64>);
        Some([
            parts.next()?.ok()?,
            parts.next()?.ok()?,
            parts.next()?.ok()?,
        ])
    }
    match (parts(candidate), parts(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn uninstall(args: UninstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    if !args.yes {
        return Err("uninstall 會停止服務並還原 Claude 設定；請加入 --yes 確認".into());
    }
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
    free_claude_core::restore_official_config()?;

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

fn open_admin() -> Result<(), Box<dyn std::error::Error>> {
    let port = free_claude_core::get_launcher_settings()
        .and_then(|settings| settings.active_port)
        .unwrap_or(3000);
    let url = format!("http://127.0.0.1:{port}/admin");

    println!("正在開啟 Web Admin：{url}");

    #[cfg(target_os = "windows")]
    let status = ProcessCommand::new("cmd")
        .args(["/C", "start", "", &url])
        .status()?;
    #[cfg(target_os = "macos")]
    let status = ProcessCommand::new("open").arg(&url).status()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = ProcessCommand::new("xdg-open").arg(&url).status()?;

    if !status.success() {
        return Err(io::Error::other("無法開啟 Web Admin").into());
    }
    Ok(())
}

fn launch_claude() -> Result<(), Box<dyn std::error::Error>> {
    let path = free_claude_core::launch_claude(None)?;
    println!("Claude 已啟動：{}", path.display());
    Ok(())
}

fn restore_settings() -> Result<(), Box<dyn std::error::Error>> {
    free_claude_core::restore_official_config()?;
    println!("Claude 官方設定已還原");
    Ok(())
}

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

fn proxy_port() -> Result<u16, Box<dyn std::error::Error>> {
    Ok(std::env::var("FREECLAUDE_PROXY_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()?)
}

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

fn stop_proxy() -> Result<(), Box<dyn std::error::Error>> {
    crate::runtime::native::stop_proxy()?;
    println!("Proxy 已停止");
    Ok(())
}

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
    fn uninstall_requires_explicit_confirmation() {
        let args = UninstallArgs {
            runtime: Runtime::Native,
            purge_image: false,
            yes: false,
        };
        assert!(!args.yes);
    }

    #[test]
    fn install_defaults_to_native_runtime() {
        let args = InstallArgs {
            runtime: Runtime::Native,
            no_autostart: false,
        };
        assert!(matches!(args.runtime, Runtime::Native));
    }

    #[test]
    fn detects_newer_three_part_versions() {
        assert!(version_is_newer("0.2.0", "0.1.9"));
        assert!(!version_is_newer("0.1.1", "0.1.1"));
        assert!(!version_is_newer("invalid", "0.1.1"));
    }
}
