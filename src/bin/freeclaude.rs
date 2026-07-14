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
        Command::Start => start_proxy(),
        Command::Stop => stop_proxy(),
        Command::Status => print_proxy_status().await,
        Command::Configure => open_admin(),
        Command::LaunchClaude => launch_claude(),
        Command::Restore => restore_settings(),
        command => Err(format!("命令尚未實作：{}", command_name(&command)).into()),
    }
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

fn start_proxy() -> Result<(), Box<dyn std::error::Error>> {
    let port = proxy_port()?;
    let pid = free_claude_desktop::runtime::native::start_proxy(port)?;
    println!("Proxy 已啟動：PID {pid}，連接埠 {port}");
    Ok(())
}

fn stop_proxy() -> Result<(), Box<dyn std::error::Error>> {
    free_claude_desktop::runtime::native::stop_proxy()?;
    println!("Proxy 已停止");
    Ok(())
}

async fn print_proxy_status() -> Result<(), Box<dyn std::error::Error>> {
    let proxy_url = std::env::var("FREECLAUDE_PROXY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
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

    println!("Proxy 正常運作：{healthz_url}");
    Ok(())
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
}
