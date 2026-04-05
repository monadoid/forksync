use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use forksync_config::{
    AgentProvider, ConflictStrategy, OutputMode, PatchDerivationMode, PermissionLevel, ProductMode,
    PromptProfile, RepoVisibility, RunnerPreset, SyncStrategy, TriggerSource, UpstreamAuthMode,
    ValidationMode,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "forksync",
    version,
    about = "Keep forks synced with upstream automatically"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    #[arg(long, global = true, default_value = ".forksync.yml")]
    pub config: PathBuf,

    #[arg(long, global = true, default_value_t = false)]
    pub verbose: bool,

    #[arg(long, global = true, default_value_t = false)]
    pub json_logs: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init(InitArgs),
    Sync(SyncArgs),
    Validate(ValidateArgs),
    PrintConfig(PrintConfigArgs),
    GenerateWorkflow(GenerateWorkflowArgs),
    Status(StatusArgs),
    Rollback(RollbackArgs),
    Registry(RegistryArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long, default_value_t = false)]
    pub non_interactive: bool,

    #[arg(long, default_value_t = false)]
    pub force: bool,

    #[arg(long, default_value_t = true)]
    pub detect_upstream: bool,

    #[arg(long, default_value_t = true)]
    pub initial_sync: bool,

    #[arg(long, default_value_t = true)]
    pub install_workflow: bool,

    #[arg(long, default_value_t = true)]
    pub create_branches: bool,

    #[arg(long, value_enum, default_value_t = RunnerPreset::UbuntuLatest)]
    pub runner: RunnerPreset,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    #[arg(long, default_value_t = false)]
    pub force: bool,

    #[arg(long, default_value_t = false)]
    pub no_agent: bool,

    #[arg(long, default_value_t = false)]
    pub no_validate: bool,

    #[arg(long, value_enum)]
    pub trigger: Option<TriggerSource>,

    #[arg(long)]
    pub upstream_sha: Option<String>,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    #[arg(long, default_value_t = true)]
    pub config_only: bool,

    #[arg(long, default_value_t = false)]
    pub git_state: bool,
}

#[derive(Debug, Args)]
pub struct PrintConfigArgs {
    #[arg(long, default_value_t = false)]
    pub json: bool,

    #[arg(long, default_value_t = true)]
    pub effective: bool,
}

#[derive(Debug, Args)]
pub struct GenerateWorkflowArgs {
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(long, default_value_t = true)]
    pub history: bool,
}

#[derive(Debug, Args)]
pub struct RollbackArgs {
    #[arg(long)]
    pub to: Option<String>,

    #[arg(long, default_value_t = false)]
    pub push: bool,
}

#[derive(Debug, Args)]
pub struct RegistryArgs {
    #[command(subcommand)]
    pub command: RegistryCommand,
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    Publish,
    Add,
    Remove,
    List,
}

#[allow(dead_code)]
fn schema_markers() {
    let _ = (
        ProductMode::ActionOnlyPolling,
        RepoVisibility::Auto,
        OutputMode::Main,
        SyncStrategy::ReplayPatchStack,
        PatchDerivationMode::SinceRecordedPatchBase,
        ConflictStrategy::AgentThenPr,
        ValidationMode::None,
        AgentProvider::OpenCode,
        PromptProfile::Reckless,
        UpstreamAuthMode::Anonymous,
        RunnerPreset::UbuntuLatest,
        PermissionLevel::Write,
    );
}

fn main() -> Result<()> {
    let _ = Cli::parse();
    println!("ForkSync schema scaffold only. Command execution is not implemented yet.");
    Ok(())
}
