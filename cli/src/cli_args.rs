use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "freeclaude", about = "FreeClaudeDesktop 管理工具")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Install(InstallArgs),
    Start(HiddenRuntimeArgs),
    Stop(HiddenRuntimeArgs),
    Status(HiddenRuntimeArgs),
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
pub(crate) struct InstallArgs {
    #[arg(long)]
    pub(crate) no_autostart: bool,
    /// 已移除：Docker runtime。保留隱藏參數以提供友善遷移錯誤。
    #[arg(long, hide = true)]
    pub(crate) runtime: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct HiddenRuntimeArgs {
    /// 已移除：Docker runtime。保留隱藏參數以提供友善遷移錯誤。
    #[arg(long, hide = true)]
    pub(crate) runtime: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    #[arg(long)]
    pub(crate) check: bool,
    /// 已移除：Docker runtime。
    #[arg(long, hide = true)]
    pub(crate) runtime: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct UninstallArgs {
    /// 已移除：Docker runtime。
    #[arg(long, hide = true)]
    pub(crate) runtime: Option<String>,
    /// 已移除：Docker image 清理。
    #[arg(long, hide = true)]
    pub(crate) purge_image: bool,
    /// 保留隱藏的 --yes 以相容舊腳本，uninstall 本身不需確認。
    #[arg(long, hide = true)]
    pub(crate) yes: bool,
}

#[derive(Debug, Args)]
pub(crate) struct PurgeArgs {
    #[arg(long)]
    pub(crate) yes: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AutostartCommand {
    Enable,
    Disable,
    Status,
}
