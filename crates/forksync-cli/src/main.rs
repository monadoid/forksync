use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Args, Parser, Subcommand};
use forksync_agent::OpenCodeFactory;
use forksync_config::{
    DEFAULT_CONFIG_PATH, DEFAULT_WORKFLOW_PATH, RepoConfig, RunnerPreset, TriggerSource,
    load_from_path, to_yaml_string,
};
use forksync_engine::{InitRequest, SyncEngine, SyncRequest, default_state_file_path};
use forksync_git::{GitBackend, SystemGitBackend};
use forksync_github::{NoopFailureReporter, generate_sync_workflow};
use forksync_state::{FileStateStore, StateStore};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "forksync",
    version,
    about = "Keep forks synced with upstream automatically"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    #[arg(long, global = true, default_value = DEFAULT_CONFIG_PATH)]
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

    #[arg(long = "no-detect-upstream", default_value_t = false)]
    pub no_detect_upstream: bool,

    #[arg(long, default_value_t = false)]
    pub initial_sync: bool,

    #[arg(long = "no-install-workflow", default_value_t = false)]
    pub no_install_workflow: bool,

    #[arg(long = "no-create-branches", default_value_t = false)]
    pub no_create_branches: bool,

    #[arg(long, value_enum, default_value_t = RunnerPreset::UbuntuLatest)]
    pub runner: RunnerPreset,

    #[arg(long)]
    pub upstream_remote: Option<String>,

    #[arg(long)]
    pub upstream_repo: Option<String>,

    #[arg(long)]
    pub upstream_branch: Option<String>,
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

    #[arg(long, action = ArgAction::SetTrue)]
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_path = std::env::current_dir().context("resolve current directory")?;
    let config_path = resolve_path(&repo_path, &cli.config);

    match cli.command {
        Command::Init(args) => run_init(&repo_path, &config_path, args),
        Command::Sync(args) => run_sync(&repo_path, &config_path, args),
        Command::Validate(args) => run_validate(&repo_path, &config_path, args),
        Command::PrintConfig(args) => run_print_config(&config_path, args),
        Command::GenerateWorkflow(args) => run_generate_workflow(&repo_path, &config_path, args),
        Command::Status(args) => run_status(&repo_path, &config_path, args),
        Command::Rollback(_) => Err(anyhow!("rollback is not implemented yet")),
        Command::Registry(_) => Err(anyhow!("registry commands are not implemented yet")),
    }
}

fn run_init(repo_path: &Path, config_path: &Path, args: InitArgs) -> Result<()> {
    let workflow_path = repo_path.join(DEFAULT_WORKFLOW_PATH);
    let state_path = default_state_file_path(repo_path, &RepoConfig::default());
    let engine = SyncEngine::new(
        SystemGitBackend,
        OpenCodeFactory,
        FileStateStore::new(state_path),
        NoopFailureReporter,
    );

    let report = engine.init(&InitRequest {
        repo_path: repo_path.to_path_buf(),
        config_path: config_path.to_path_buf(),
        workflow_path: workflow_path.clone(),
        force: args.force,
        detect_upstream: !args.no_detect_upstream,
        initial_sync: args.initial_sync,
        install_workflow: !args.no_install_workflow,
        create_branches: !args.no_create_branches,
        runner: args.runner,
        upstream_remote: args.upstream_remote,
        upstream_repo: args.upstream_repo,
        upstream_branch: args.upstream_branch,
    })?;

    println!("Initialized ForkSync in {}", repo_path.display());
    println!(
        "Config path in bootstrap commit: {}",
        report.config_path.display()
    );
    if let Some(workflow) = report.workflow_path {
        println!("Workflow path in bootstrap commit: {}", workflow.display());
    }
    println!(
        "Upstream: {} ({}) via remote {}",
        report.upstream_repo, report.upstream_branch, report.upstream_remote
    );
    println!(
        "Branches: patch={}, live={}, output={}",
        report.patch_branch, report.live_branch, report.output_branch
    );
    println!("Bootstrap commit: {}", report.bootstrap_commit_sha);
    if !report.pushed_branches.is_empty() {
        println!("Pushed: {}", report.pushed_branches.join(", "));
    }
    for note in report.notes {
        println!("- {}", note);
    }
    println!("Next steps:");
    println!(
        "1. Switch to `{}` to inspect the bootstrap and start your patch layer.",
        report.patch_branch
    );
    println!(
        "2. Treat `{}` as machine-managed output under the current model.",
        report.output_branch
    );
    println!(
        "3. Add your custom fork changes on `{}` and commit them there.",
        report.patch_branch
    );
    println!("4. Run `forksync sync --trigger local-debug` to preview local sync behavior.");
    println!(
        "5. If automatic push failed, push `{}`, `{}`, and `{}` to origin manually.",
        report.patch_branch, report.live_branch, report.output_branch
    );

    Ok(())
}

fn run_sync(repo_path: &Path, config_path: &Path, args: SyncArgs) -> Result<()> {
    let config = load_from_path(config_path)?;
    let state_path = default_state_file_path(repo_path, &config);
    let engine = SyncEngine::new(
        SystemGitBackend,
        OpenCodeFactory,
        FileStateStore::new(state_path),
        NoopFailureReporter,
    );

    let report = engine.sync(&SyncRequest {
        repo_path: repo_path.to_path_buf(),
        config,
        trigger: args.trigger,
        dry_run: args.dry_run,
        force: args.force,
        disable_agent: args.no_agent,
        disable_validation: args.no_validate,
        upstream_sha: args.upstream_sha,
    })?;

    println!("Sync outcome: {:?}", report.outcome);
    if let Some(upstream_sha) = report.upstream_sha {
        println!("Upstream SHA: {upstream_sha}");
    }
    println!("Patch commits applied: {}", report.patch_commits_applied);
    for note in report.notes {
        println!("- {}", note);
    }

    Ok(())
}

fn run_validate(repo_path: &Path, config_path: &Path, args: ValidateArgs) -> Result<()> {
    let _config = load_from_path(config_path)?;
    if args.git_state {
        SystemGitBackend.ensure_repo(repo_path)?;
    }
    println!("Configuration is valid: {}", config_path.display());
    Ok(())
}

fn run_print_config(config_path: &Path, args: PrintConfigArgs) -> Result<()> {
    let config = load_from_path(config_path)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&config).context("serialize config as JSON")?
        );
    } else {
        println!(
            "{}",
            to_yaml_string(&config).context("serialize config as YAML")?
        );
    }
    Ok(())
}

fn run_generate_workflow(
    repo_path: &Path,
    config_path: &Path,
    args: GenerateWorkflowArgs,
) -> Result<()> {
    let config = load_from_path(config_path)?;
    let workflow = generate_sync_workflow(&config);
    let workflow_path = repo_path.join(&workflow.path);

    if workflow_path.exists() && !args.force {
        return Err(anyhow!(
            "workflow file already exists at {} (pass --force to overwrite)",
            workflow_path.display()
        ));
    }

    if let Some(parent) = workflow_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create workflow directory {}", parent.display()))?;
    }
    std::fs::write(&workflow_path, workflow.contents)
        .with_context(|| format!("write workflow to {}", workflow_path.display()))?;

    println!("Generated workflow at {}", workflow_path.display());
    Ok(())
}

fn run_status(repo_path: &Path, config_path: &Path, args: StatusArgs) -> Result<()> {
    let config = load_from_path(config_path)?;
    let state_path = default_state_file_path(repo_path, &config);
    let state = FileStateStore::new(state_path.clone()).load()?;

    println!("ForkSync status for {}", repo_path.display());
    println!("Config: {}", config_path.display());
    println!("State: {}", state_path.display());
    println!(
        "Last processed upstream SHA: {}",
        state
            .last_processed_upstream_sha
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "Last good sync SHA: {}",
        state.last_good_sync_sha.as_deref().unwrap_or("<none>")
    );
    println!(
        "Patch base SHA: {}",
        state.patch_base_sha.as_deref().unwrap_or("<none>")
    );
    if args.history {
        println!("History entries: {}", state.history.len());
    }

    Ok(())
}

fn resolve_path(repo_path: &Path, configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        repo_path.join(configured)
    }
}
