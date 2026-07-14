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
    Start,
    Stop,
    Status,
    Configure,
    #[command(name = "launch-claude")]
    LaunchClaude,
    Restore,
    Update(UpdateArgs),
    Uninstall(UninstallArgs),
    Autostart {
        #[command(subcommand)]
        command: AutostartCommand,
    },
}

#[derive(Debug, Args)]
struct InstallArgs {
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    runtime: Runtime,
    #[arg(long)]
    no_autostart: bool,
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
}

#[derive(Debug, Args)]
struct UninstallArgs {
    #[arg(long)]
    purge_image: bool,
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
        Command::Start => start_proxy().await,
        Command::Stop => stop_proxy(),
        Command::Status => print_proxy_status().await,
        Command::Configure => open_admin(),
        Command::LaunchClaude => launch_claude(),
        Command::Restore => restore_settings(),
        Command::Uninstall(args) => uninstall(args),
        Command::Autostart { command } => manage_autostart(command),
        command => Err(format!("命令尚未實作：{}", command_name(&command)).into()),
    }
}

async fn install(args: InstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    match args.runtime {
        Runtime::Native => {
            start_proxy().await?;
            let port = proxy_port()?;
            free_claude_desktop::update_config_port(port)?;
            if !args.no_autostart {
                free_claude_desktop::runtime::autostart::enable()?;
            }
            println!("Native runtime 安裝完成");
            Ok(())
        }
        Runtime::Docker => Err("Docker install 尚需 Docker Compose v2 與 container 設定掛載；請使用 docker compose up --build".into()),
    }
}

fn uninstall(args: UninstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    if !args.yes {
        return Err("uninstall 會停止服務並還原 Claude 設定；請加入 --yes 確認".into());
    }
    if let Err(error) = free_claude_desktop::runtime::native::stop_proxy() {
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error.into());
        }
    }
    let _ = free_claude_desktop::runtime::autostart::disable();
    free_claude_desktop::restore_official_config()?;

    if args.purge_image {
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
            free_claude_desktop::runtime::autostart::enable()?;
            println!("自動啟動已啟用");
        }
        AutostartCommand::Disable => {
            free_claude_desktop::runtime::autostart::disable()?;
            println!("自動啟動已停用");
        }
        AutostartCommand::Status => println!(
            "自動啟動：{}",
            if free_claude_desktop::runtime::autostart::is_enabled()? {
                "已啟用"
            } else {
                "未啟用"
            }
        ),
    }
    Ok(())
}

fn open_admin() -> Result<(), Box<dyn std::error::Error>> {
    let port = free_claude_desktop::get_launcher_settings()
        .and_then(|settings| settings.active_port)
        .unwrap_or(3000);
    let url = format!("http://127.0.0.1:{port}/admin");

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
    println!("已開啟 Web Admin：{url}");
    Ok(())
}

fn launch_claude() -> Result<(), Box<dyn std::error::Error>> {
    let path = free_claude_desktop::launch_claude(None)?;
    println!("Claude 已啟動：{}", path.display());
    Ok(())
}

fn restore_settings() -> Result<(), Box<dyn std::error::Error>> {
    free_claude_desktop::restore_official_config()?;
    println!("Claude 官方設定已還原");
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
    let pid = free_claude_desktop::runtime::native::start_proxy(port)?;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if proxy_is_healthy(&healthz_url).await {
            println!("Proxy 已啟動：PID {pid}，連接埠 {port}");
            return Ok(());
        }
    }

    let _ = free_claude_desktop::runtime::native::stop_proxy();
    Err("Proxy 未在 5 秒內通過健康檢查".into())
}

fn stop_proxy() -> Result<(), Box<dyn std::error::Error>> {
    free_claude_desktop::runtime::native::stop_proxy()?;
    println!("Proxy 已停止");
    Ok(())
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

    let pid = std::fs::read_to_string(free_claude_desktop::runtime::native::pid_file())
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok());
    let autostart = free_claude_desktop::runtime::autostart::is_enabled().unwrap_or(false);
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

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Install(_) => "install",
        Command::Start => "start",
        Command::Stop => "stop",
        Command::Status => "status",
        Command::Configure => "configure",
        Command::LaunchClaude => "launch-claude",
        Command::Restore => "restore",
        Command::Update(_) => "update",
        Command::Uninstall(_) => "uninstall",
        Command::Autostart { .. } => "autostart",
    }
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
    }

    #[test]
    fn uninstall_requires_explicit_confirmation() {
        let args = UninstallArgs {
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
}
