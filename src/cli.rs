use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::model::Profile;

#[derive(Debug, Parser)]
#[command(
    name = "cura",
    version,
    about = "CURA — interactive GPU environment manager for CUDA and Metal"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Emit stable machine-readable output where supported.
    #[arg(long, global = true)]
    pub json: bool,

    /// Disable interactive terminal interfaces and prompts.
    #[arg(long, global = true)]
    pub no_interactive: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install a CUDA environment or the macOS Metal toolchain.
    Install(InstallArgs),
    /// List installed or available CUDA versions.
    List(ListArgs),
    /// Select an installed CUDA environment.
    Use(UseArgs),
    /// Run a command inside a CUDA environment.
    Run(RunArgs),
    /// Print shell environment exports.
    Env(EnvArgs),
    /// Remove an installed CUDA environment.
    #[command(alias = "uninstall")]
    Remove(RemoveArgs),
    /// Diagnose platform, GPU toolchain, and environment health.
    Doctor,
    /// Inspect or install the NVIDIA driver.
    Driver(DriverArgs),
    /// Inspect or install the Apple Metal toolchain on macOS.
    Metal(MetalArgs),
    /// Generate shell integration.
    Shell(ShellArgs),
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// CUDA version (for example cuda-12.9.1), or metal on macOS.
    pub version: String,
    #[arg(long, value_enum)]
    pub profile: Option<Profile>,
    /// Install through the operating system package manager.
    #[arg(long)]
    pub system: bool,
    /// Accept the reviewed plan without prompting.
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// Do not offer to install or upgrade an incompatible driver.
    #[arg(long)]
    pub no_driver: bool,
}

#[derive(Debug, Args)]
pub struct MetalArgs {
    #[command(subcommand)]
    pub command: MetalCommand,
}

#[derive(Debug, Subcommand)]
pub enum MetalCommand {
    /// Show the Metal GPU, runtime, Xcode, and compiler status.
    Status,
    /// Install Apple's Metal compiler toolchain through Xcode.
    Install {
        /// Accept the reviewed plan without prompting.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Query NVIDIA for versions available to install.
    #[arg(long)]
    pub available: bool,
}

#[derive(Debug, Args)]
pub struct UseArgs {
    pub version: String,
    /// Set the user-wide default.
    #[arg(long, conflicts_with = "local")]
    pub global: bool,
    /// Write selection in the current project (the default).
    #[arg(long)]
    pub local: bool,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long)]
    pub cuda: Option<String>,
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
}

#[derive(Debug, Args)]
pub struct EnvArgs {
    #[arg(long, value_enum)]
    pub shell: Option<ShellKind>,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    pub version: String,
    #[arg(long)]
    pub system: bool,
    #[arg(short = 'y', long)]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub struct DriverArgs {
    #[command(subcommand)]
    pub command: DriverCommand,
}

#[derive(Debug, Subcommand)]
pub enum DriverCommand {
    Status,
    Install {
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct ShellArgs {
    #[command(subcommand)]
    pub command: ShellCommand,
}

#[derive(Debug, Subcommand)]
pub enum ShellCommand {
    Init {
        #[arg(value_enum)]
        shell: ShellKind,
        /// Append the integration to the shell startup file after confirmation.
        #[arg(long)]
        write: bool,
        /// Override the startup file used with --write.
        #[arg(long)]
        rc_file: Option<PathBuf>,
    },
}
