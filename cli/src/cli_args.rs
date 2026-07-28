use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "freeclaude", about = "FreeClaudeDesktop 管理工具")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
pub(crate) struct InstallArgs {
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    pub(crate) runtime: Runtime,
    #[arg(long)]
    pub(crate) no_autostart: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RuntimeArgs {
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    pub(crate) runtime: Runtime,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum Runtime {
    Native,
    Docker,
}

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    #[arg(long)]
    pub(crate) check: bool,
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    pub(crate) runtime: Runtime,
}

#[derive(Debug, Args)]
pub(crate) struct UninstallArgs {
    #[arg(long, value_enum, default_value_t = Runtime::Native)]
    pub(crate) runtime: Runtime,
    #[arg(long)]
    pub(crate) purge_image: bool,
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
