mod init_wizard;
mod telemetry;

use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Args, Parser, Subcommand};
use dotenvy::from_path_override;
use forksync_agent::OpenCodeFactory;
use forksync_config::{
    AgentProvider, ConfigIoError, DEFAULT_CONFIG_PATH, DEFAULT_WORKFLOW_PATH, RepoConfig,
    RunnerPreset, TriggerSource, load_from_path, to_yaml_string,
};
use forksync_engine::{InitRequest, SyncEngine, SyncRequest, default_state_file_path};
use forksync_git::{GitBackend, SystemGitBackend};
use forksync_github::{NoopFailureReporter, generate_sync_workflow};
use init_wizard::{
    InitPushPreflight, resolve_init_preferences, run_init_wizard, should_run_init_wizard,
};
use forksync_state::{FileStateStore, StateStore};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, info, instrument};

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
    Dev(DevArgs),
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

    #[arg(long)]
    pub build_command: Option<String>,

    #[arg(long)]
    pub test_command: Option<String>,

    #[arg(long = "manual-push-output", default_value_t = false, alias = "no-auto-push")]
    pub manual_push_output: bool,

    #[arg(long, value_enum)]
    pub agent_provider: Option<AgentProvider>,
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

#[derive(Debug, Args)]
pub struct DevArgs {
    #[command(subcommand)]
    pub command: DevCommand,
}

#[derive(Debug, Subcommand)]
pub enum DevCommand {
    Demo(DevDemoArgs),
    Act(DevActArgs),
}

#[derive(Debug, Clone, Args)]
pub struct DevDemoArgs {
    #[arg(long, default_value = "demo", hide = true)]
    pub dest: String,

    #[arg(long, default_value_t = false)]
    pub auto: bool,

    #[arg(long, default_value_t = 1000, hide = true)]
    pub sleep_ms: u64,

    #[arg(long, default_value_t = false, hide = true)]
    pub pre_sync_only: bool,
}

#[derive(Debug, Clone, Args)]
pub struct DevActArgs {
    #[arg(long, default_value = "act-demo", hide = true)]
    pub dest: String,

    #[arg(long, default_value_t = 250, hide = true)]
    pub sleep_ms: u64,

    #[arg(long, default_value_t = false)]
    pub docker: bool,

    #[arg(long, default_value_t = false, hide = true)]
    pub pull: bool,
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    Publish,
    Add,
    Remove,
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let _telemetry = telemetry::init_telemetry(cli.verbose, cli.json_logs)?;
    let repo_path = std::env::current_dir().context("resolve current directory")?;
    let _ = from_path_override(repo_path.join(".env"));
    let config_path = resolve_path(&repo_path, &cli.config);
    debug!(
        repo_path = %repo_path.display(),
        config_path = %config_path.display(),
        "resolved CLI paths"
    );

    let result = match cli.command {
        Command::Init(args) => run_init(&repo_path, &config_path, args),
        Command::Sync(args) => run_sync(&repo_path, &config_path, args),
        Command::Validate(args) => run_validate(&repo_path, &config_path, args),
        Command::PrintConfig(args) => run_print_config(&config_path, args),
        Command::GenerateWorkflow(args) => run_generate_workflow(&repo_path, &config_path, args),
        Command::Status(args) => run_status(&repo_path, &config_path, args),
        Command::Dev(args) => run_dev(&repo_path, args),
        Command::Rollback(_) => Err(anyhow!("rollback is not implemented yet")),
        Command::Registry(_) => Err(anyhow!("registry commands are not implemented yet")),
    };

    match &result {
        Ok(()) => info!("command completed successfully"),
        Err(error) => tracing::error!(error = %error, "command failed"),
    }

    result
}

#[instrument(skip_all, fields(repo_path = %repo_path.display(), config_path = %config_path.display()))]
fn run_init(repo_path: &Path, config_path: &Path, args: InitArgs) -> Result<()> {
    let workflow_path = repo_path.join(DEFAULT_WORKFLOW_PATH);
    let state_path = default_state_file_path(repo_path, &RepoConfig::default());
    let engine = SyncEngine::new(
        SystemGitBackend,
        OpenCodeFactory,
        FileStateStore::new(state_path),
        NoopFailureReporter,
    );

    let push_preflight = probe_init_push_preflight(repo_path)?;
    let decision_inputs = InitDecisionInputs {
        requested_auto_push: if args.manual_push_output {
            Some(false)
        } else {
            None
        },
        requested_agent_provider: args.agent_provider,
    };
    let should_run_wizard = should_run_interactive_wizard(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        args.non_interactive,
        &decision_inputs,
    );
    let resolved_preferences = if should_run_wizard {
        run_interactive_init_wizard(InitWizardPromptContext {
            preflight: push_preflight.clone(),
            requested_auto_push: decision_inputs.requested_auto_push.map(|value| {
                if value {
                    init_wizard::AutoPushChoice::Yes
                } else {
                    init_wizard::AutoPushChoice::No
                }
            }),
            requested_agent_choice: decision_inputs
                .requested_agent_provider
                .and_then(init_wizard::agent_choice_from_provider),
        })?
    } else {
        resolve_init_plan(&push_preflight, &decision_inputs)
    };

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
        build_command: args.build_command,
        test_command: args.test_command,
        auto_push: resolved_preferences.auto_push_managed_refs,
        agent_provider: resolved_preferences.agent_provider,
    })?;
    info!(
        upstream_remote = %report.upstream_remote,
        upstream_branch = %report.upstream_branch,
        bootstrap_commit = %report.bootstrap_commit_sha,
        "initialized ForkSync repository"
    );

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
    if !resolved_preferences.auto_push_managed_refs {
        println!(
            "- ForkSync left managed branch publication manual: {}",
            push_preflight.safety_note
        );
    }
    for note in report.notes {
        println!("- {}", note);
    }
    println!("Next steps:");
    println!(
        "1. Work on `{}` like your normal fork branch.",
        report.output_branch
    );
    println!(
        "2. Treat `{}` as the machine-generated recovery/debug branch.",
        report.live_branch
    );
    println!(
        "3. Add your custom fork changes on `{}` and commit them there.",
        report.output_branch
    );
    println!("4. Run `forksync sync --trigger local-debug` to preview local sync behavior.");
    if !report.manual_push_branches.is_empty() {
        println!(
            "5. If you want to publish the managed branches now, run this exact command next:"
        );
        println!(
            "   git push origin {}",
            report
                .manual_push_branches
                .iter()
                .map(|branch| format!("{branch}:{branch}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    } else {
        println!("5. Automatic push already completed for the managed branches.");
    }

    Ok(())
}

fn probe_init_push_preflight(repo_path: &Path) -> Result<InitPreflight> {
    let git = SystemGitBackend;
    if !git.remote_exists(repo_path, "origin")? {
        return Ok(InitPreflight {
            safe_to_push_main_directly: false,
            safety_note: "ForkSync could not confirm a safe direct push to `main` because no origin remote was found."
                .to_string(),
        });
    }

    let output_branch = git
        .default_branch_for_remote(repo_path, "origin")
        .unwrap_or_else(|_| "main".to_string());
    if output_branch != "main" {
        return Ok(InitPreflight {
            safe_to_push_main_directly: false,
            safety_note: format!(
                "ForkSync could not confirm a safe direct push to `main` because origin default branch resolved to `{output_branch}`."
            ),
        });
    }

    let dry_run = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["push", "--dry-run", "origin", "HEAD:refs/heads/main"])
        .output()
        .context("run init push preflight against origin/main")?;

    if dry_run.status.success() {
        Ok(InitPreflight {
            safe_to_push_main_directly: true,
            safety_note: "ForkSync can dry-run a push to `main` successfully.".to_string(),
        })
    } else {
        let stderr = String::from_utf8_lossy(&dry_run.stderr).trim().to_string();
        Ok(InitPreflight {
            safe_to_push_main_directly: false,
            safety_note: if stderr.is_empty() {
                "ForkSync could not confirm a safe direct push to `main` because `git push --dry-run origin HEAD:refs/heads/main` did not succeed."
                    .to_string()
            } else {
                format!(
                    "ForkSync could not confirm a safe direct push to `main`: {}",
                    stderr
                )
            },
        })
    }
}

#[instrument(skip_all, fields(repo_path = %repo_path.display(), config_path = %config_path.display()))]
fn run_sync(repo_path: &Path, config_path: &Path, args: SyncArgs) -> Result<()> {
    let config = load_repo_config(config_path)?;
    let workflow_path = repo_path.join(DEFAULT_WORKFLOW_PATH);
    let state_path = default_state_file_path(repo_path, &config);
    let engine = SyncEngine::new(
        SystemGitBackend,
        OpenCodeFactory,
        FileStateStore::new(state_path),
        NoopFailureReporter,
    );

    let report = engine.sync(&SyncRequest {
        repo_path: repo_path.to_path_buf(),
        config_path: config_path.to_path_buf(),
        workflow_path,
        config,
        trigger: args.trigger,
        dry_run: args.dry_run,
        force: args.force,
        disable_agent: args.no_agent,
        disable_validation: args.no_validate,
        upstream_sha: args.upstream_sha,
    })?;
    info!(
        outcome = ?report.outcome,
        used_agent = report.used_agent,
        patch_commits_applied = report.patch_commits_applied,
        upstream_sha = report.upstream_sha.as_deref().unwrap_or("<none>"),
        "sync completed"
    );

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

#[instrument(skip_all, fields(repo_path = %repo_path.display(), config_path = %config_path.display()))]
fn run_validate(repo_path: &Path, config_path: &Path, args: ValidateArgs) -> Result<()> {
    let _config = load_repo_config(config_path)?;
    if args.git_state {
        SystemGitBackend.ensure_repo(repo_path)?;
    }
    println!("Configuration is valid: {}", config_path.display());
    Ok(())
}

#[instrument(skip_all, fields(config_path = %config_path.display()))]
fn run_print_config(config_path: &Path, args: PrintConfigArgs) -> Result<()> {
    let config = load_repo_config(config_path)?;
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
    let config = load_repo_config(config_path)?;
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
    info!(workflow_path = %workflow_path.display(), "generated workflow file");

    println!("Generated workflow at {}", workflow_path.display());
    Ok(())
}

#[instrument(skip_all, fields(repo_path = %repo_path.display(), config_path = %config_path.display()))]
fn run_status(repo_path: &Path, config_path: &Path, args: StatusArgs) -> Result<()> {
    let config = load_repo_config(config_path)?;
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
        "Author base SHA: {}",
        state.author_base_sha.as_deref().unwrap_or("<none>")
    );
    if args.history {
        println!("History entries: {}", state.history.len());
    }

    Ok(())
}

#[instrument(skip_all, fields(repo_path = %repo_path.display()))]
fn run_dev(repo_path: &Path, args: DevArgs) -> Result<()> {
    match args.command {
        DevCommand::Demo(args) => run_dev_demo(repo_path, args),
        DevCommand::Act(args) => run_dev_act(repo_path, args),
    }
}

#[derive(Debug, Clone)]
struct DevDemoPaths {
    root: PathBuf,
    upstream_working: PathBuf,
    upstream_remote: PathBuf,
    fork_remote: PathBuf,
    user_repo: PathBuf,
}

#[instrument(skip_all, fields(repo_root = %repo_root.display(), dest = %args.dest))]
fn run_dev_demo(repo_root: &Path, args: DevDemoArgs) -> Result<()> {
    let paths = create_dev_demo_repos(repo_root, &args.dest)?;

    println!(
        "Created local ForkSync demo repos under {}",
        paths.root.display()
    );
    println!("User clone: {}", paths.user_repo.display());
    println!("Upstream remote: {}", paths.upstream_remote.display());
    println!("Fork remote: {}", paths.fork_remote.display());

    if args.auto {
        run_dev_auto_demo(repo_root, &paths, args.sleep_ms, args.pre_sync_only)?;
    } else {
        println!();
        println!("Suggested local dogfood flow:");
        println!("  1. cd {}", shell_escape_path(&paths.user_repo));
        println!("  2. forksync init");
        println!("  3. git show main:.forksync.yml");
        println!("  4. echo \"local patch\" > PATCH.txt");
        println!("  5. git add PATCH.txt && git commit -m \"Add local patch\"");
        println!(
            "  6. echo \"upstream change\" > {}",
            shell_escape_path(&paths.upstream_working.join("UPSTREAM.txt"))
        );
        println!(
            "  7. git -C {} add UPSTREAM.txt",
            shell_escape_path(&paths.upstream_working)
        );
        println!(
            "  8. git -C {} commit -m \"Add upstream change\"",
            shell_escape_path(&paths.upstream_working)
        );
        println!(
            "  9. git -C {} push",
            shell_escape_path(&paths.upstream_working)
        );
        println!(" 10. forksync sync --trigger local-debug --no-agent");
        println!(" 11. git show main:PATCH.txt");
        println!(" 12. git show main:UPSTREAM.txt");
    }

    Ok(())
}

#[instrument(skip_all, fields(repo_root = %repo_root.display(), dest = %args.dest, docker = args.docker))]
fn run_dev_act(repo_root: &Path, args: DevActArgs) -> Result<()> {
    ensure_act_installed()?;

    let workflow_dir = repo_root.join("sandbox/act");
    fs::create_dir_all(&workflow_dir)
        .with_context(|| format!("create act scratch directory {}", workflow_dir.display()))?;
    let workflow_path = workflow_dir.join("forksync-dev-demo.yml");
    let binary_rel_path = if args.docker {
        None
    } else {
        Some(prepare_host_act_binary(repo_root, &workflow_dir)?)
    };
    fs::write(
        &workflow_path,
        render_dev_act_workflow(
            &args.dest,
            args.sleep_ms,
            args.docker,
            binary_rel_path.as_deref(),
        ),
    )
    .with_context(|| format!("write local act workflow {}", workflow_path.display()))?;

    println!(
        "Running ForkSync act workflow in {} mode.",
        if args.docker { "docker" } else { "host" }
    );
    info!(docker = args.docker, "starting local act workflow");

    let mut command = ProcessCommand::new("act");
    command.current_dir(repo_root);
    command.args(["workflow_dispatch", "-W"]);
    command.arg(&workflow_path);
    if args.pull {
        command.arg("--pull");
    } else {
        command.arg("--pull=false");
    }

    if args.docker {
        command.args(["-P", "ubuntu-latest=node:16-buster-slim"]);
    } else {
        command.args(["-P", "ubuntu-latest=-self-hosted"]);
        command.arg("--use-gitignore=false");
    }

    let output = command.output().context("run act local workflow")?;
    print_filtered_act_output(&output.stdout, output.status.success());
    print_filtered_act_output(&output.stderr, output.status.success());

    if !output.status.success() {
        return Err(anyhow!(
            "act local workflow failed with status {}",
            output.status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

fn create_dev_demo_repos(repo_root: &Path, dest: &str) -> Result<DevDemoPaths> {
    let root = if dest.starts_with('/') || dest.starts_with('.') {
        repo_root.join(dest)
    } else {
        repo_root.join("sandbox/repos").join(dest)
    };
    let upstream_working = root.join("upstream-working");
    let upstream_remote = root.join("upstream-remote.git");
    let fork_remote = root.join("fork-remote.git");
    let user_repo = root.join("user-repo");

    if root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("remove existing demo directory {}", root.display()))?;
    }
    fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;

    git_in(
        repo_root,
        [
            &"init"[..],
            &"-b",
            &"main",
            upstream_working.to_str().unwrap(),
        ],
    )?;
    git_in(&upstream_working, ["config", "user.name", "ForkSync Demo"])?;
    git_in(
        &upstream_working,
        ["config", "user.email", "forksync-demo@example.com"],
    )?;
    fs::write(upstream_working.join("README.md"), "seed repo\n")
        .context("write upstream seed readme")?;
    git_in(&upstream_working, ["add", "README.md"])?;
    git_in(
        &upstream_working,
        ["commit", "-m", "Initial upstream commit"],
    )?;

    git_in(
        repo_root,
        [
            "clone",
            "--bare",
            upstream_working.to_str().expect("utf-8 path"),
            upstream_remote.to_str().expect("utf-8 path"),
        ],
    )?;
    git_in(
        &upstream_working,
        [
            "remote",
            "add",
            "origin",
            upstream_remote.to_str().expect("utf-8 path"),
        ],
    )?;
    git_in(&upstream_working, ["fetch", "origin"])?;
    git_in(
        &upstream_working,
        ["branch", "--set-upstream-to=origin/main", "main"],
    )?;
    git_in(
        repo_root,
        [
            "clone",
            "--bare",
            upstream_working.to_str().expect("utf-8 path"),
            fork_remote.to_str().expect("utf-8 path"),
        ],
    )?;
    git_in(
        repo_root,
        [
            "clone",
            fork_remote.to_str().expect("utf-8 path"),
            user_repo.to_str().expect("utf-8 path"),
        ],
    )?;
    git_in(&user_repo, ["config", "user.name", "ForkSync Demo"])?;
    git_in(
        &user_repo,
        ["config", "user.email", "forksync-demo@example.com"],
    )?;
    git_in(
        &user_repo,
        [
            "remote",
            "add",
            "upstream",
            upstream_remote.to_str().expect("utf-8 path"),
        ],
    )?;
    git_in(&user_repo, ["fetch", "upstream"])?;

    Ok(DevDemoPaths {
        root,
        upstream_working,
        upstream_remote,
        fork_remote,
        user_repo,
    })
}

fn run_dev_auto_demo(
    repo_root: &Path,
    paths: &DevDemoPaths,
    sleep_ms: u64,
    pre_sync_only: bool,
) -> Result<()> {
    let binary_path = std::env::current_exe().context("resolve forksync binary path")?;

    narrate(
        &format!(
            "Entering the user fork clone at {}.",
            paths.user_repo.display()
        ),
        sleep_ms,
    );
    narrate(
        "Running 'forksync init' to bootstrap ForkSync with defaults.",
        sleep_ms,
    );
    let init_output = run_forksync_capture(&binary_path, &paths.user_repo, ["init"])?;
    let init_stdout = String::from_utf8_lossy(&init_output.stdout);
    print_init_demo_summary(&init_stdout);

    narrate(
        "Editing the README in the user repo with \"local change\".",
        sleep_ms,
    );
    fs::write(
        paths.user_repo.join("README.md"),
        "seed repo\nlocal change\n",
    )
    .context("write local readme change")?;
    narrate(
        "Committing the user-side change on main as \"Local readme change\".",
        sleep_ms,
    );
    git_in(&paths.user_repo, ["add", "README.md"])?;
    git_in(&paths.user_repo, ["commit", "-m", "Local readme change"])?;

    narrate(
        "Editing the README in the upstream working repo with \"upstream change\".",
        sleep_ms,
    );
    fs::write(
        paths.upstream_working.join("README.md"),
        "seed repo\nupstream change\n",
    )
    .context("write upstream readme change")?;
    narrate(
        "Committing and pushing the upstream-side change as \"Upstream readme change\".",
        sleep_ms,
    );
    git_in(&paths.upstream_working, ["add", "README.md"])?;
    git_in(
        &paths.upstream_working,
        ["commit", "-m", "Upstream readme change"],
    )?;
    git_in(&paths.upstream_working, ["push"])?;

    if pre_sync_only {
        narrate(
            "Prepared the local fork and upstream repos through the pre-sync stage.",
            sleep_ms,
        );
        narrate(
            &format!("User repo ready at {}", paths.user_repo.display()),
            sleep_ms,
        );
        let _ = repo_root;
        return Ok(());
    }

    narrate(
        "Running 'forksync sync --trigger local-debug' so ForkSync can replay the local change on top of the updated upstream.",
        sleep_ms,
    );
    let sync_output = run_forksync_capture(
        &binary_path,
        &paths.user_repo,
        ["sync", "--trigger", "local-debug"],
    )?;
    let sync_stdout = String::from_utf8_lossy(&sync_output.stdout);
    print_sync_demo_summary(&sync_stdout);

    narrate("Final README on main.", sleep_ms);
    print_block(&git_output(&paths.user_repo, ["show", "main:README.md"])?);
    narrate("Final README on forksync/live.", sleep_ms);
    print_block(&git_output(
        &paths.user_repo,
        ["show", "forksync/live:README.md"],
    )?);
    narrate(
        &format!("Fork remote: {}", paths.fork_remote.display()),
        sleep_ms,
    );
    narrate(
        &format!("Upstream remote: {}", paths.upstream_remote.display()),
        sleep_ms,
    );
    let _ = repo_root;
    Ok(())
}

fn narrate(message: &str, sleep_ms: u64) {
    println!("\n[demo] {message}");
    if sleep_ms > 0 {
        sleep(Duration::from_millis(sleep_ms));
    }
}

fn run_forksync_capture<const N: usize>(
    binary_path: &Path,
    cwd: &Path,
    args: [&str; N],
) -> Result<std::process::Output> {
    let output = ProcessCommand::new(binary_path)
        .current_dir(cwd)
        .args(args)
        .output()
        .with_context(|| format!("run forksync in {}", cwd.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "forksync command failed in {} with status {}",
            cwd.display(),
            output.status.code().unwrap_or(-1)
        ));
    }
    Ok(output)
}

fn git_in<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .with_context(|| format!("run git in {}", cwd.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git command failed in {}\nargs: {:?}\nstdout:\n{}\nstderr:\n{}",
            cwd.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn git_output<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<String> {
    let output = ProcessCommand::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .with_context(|| format!("run git in {}", cwd.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git command failed in {}\nargs: {:?}\nstdout:\n{}\nstderr:\n{}",
            cwd.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn ensure_act_installed() -> Result<()> {
    if ProcessCommand::new("act")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    let brew_available = ProcessCommand::new("brew")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !brew_available {
        return Err(anyhow!(
            "`act` is not installed and Homebrew is unavailable. Install act first or add it to PATH."
        ));
    }

    let status = ProcessCommand::new("brew")
        .args(["install", "act"])
        .status()
        .context("install act with Homebrew")?;
    if !status.success() {
        return Err(anyhow!(
            "failed to install act with Homebrew (status {})",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn prepare_host_act_binary(repo_root: &Path, workflow_dir: &Path) -> Result<String> {
    let _ = workflow_dir;
    let binary_path = repo_root.join("target/release/forksync");
    let rebuild = match (fs::metadata(&binary_path), std::env::current_exe()) {
        (Ok(binary_meta), Ok(current_exe)) => match fs::metadata(current_exe) {
            Ok(current_meta) => match (binary_meta.modified(), current_meta.modified()) {
                (Ok(binary_modified), Ok(current_modified)) => binary_modified < current_modified,
                _ => false,
            },
            Err(_) => false,
        },
        (Err(_), _) => true,
        _ => false,
    };

    if rebuild {
        let status = ProcessCommand::new("cargo")
            .current_dir(repo_root)
            .args([
                "build",
                "--release",
                "--bin",
                "forksync",
                "--quiet",
                "--locked",
            ])
            .status()
            .context("build forksync binary for host-mode act")?;
        if !status.success() {
            return Err(anyhow!(
                "failed to build forksync binary for host-mode act (status {})",
                status.code().unwrap_or(-1)
            ));
        }
    }

    Ok(binary_path.display().to_string())
}

fn render_dev_act_workflow(
    dest: &str,
    sleep_ms: u64,
    docker: bool,
    host_binary_rel_path: Option<&str>,
) -> String {
    let setup_block = if docker {
        format!(
            "set -euo pipefail\nif ! command -v cargo >/dev/null 2>&1; then\n  apt-get update\n  apt-get install -y curl git build-essential ca-certificates pkg-config libssl-dev\n  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal\nfi\nif [ -f \"$HOME/.cargo/env\" ]; then . \"$HOME/.cargo/env\"; fi\ncargo build --release --bin forksync --locked --quiet\ntarget/release/forksync dev demo --auto --pre-sync-only --dest {dest} --sleep-ms {sleep_ms}"
        )
    } else {
        let binary_rel_path = host_binary_rel_path.expect("host binary path for host-mode act");
        format!(
            "set -euo pipefail\n\"{binary_rel_path}\" dev demo --auto --pre-sync-only --dest {dest} --sleep-ms {sleep_ms}"
        )
    };

    let action_inputs = if docker {
        format!(
            "          trigger: local-debug\n          working-directory: sandbox/repos/{dest}/user-repo\n          install-opencode: true\n          allow-build-fallback: true\n"
        )
    } else {
        format!(
            "          trigger: local-debug\n          working-directory: sandbox/repos/{dest}/user-repo\n          install-opencode: true\n          binary-path: {}\n",
            host_binary_rel_path.expect("host binary path for host-mode act")
        )
    };

    let action_env = if docker {
        format!(
            "          INPUT_TRIGGER: local-debug\n          INPUT_WORKING_DIRECTORY: sandbox/repos/{dest}/user-repo\n          INPUT_INSTALL_OPENCODE: true\n          INPUT_ALLOW_BUILD_FALLBACK: true\n"
        )
    } else {
        format!(
            "          INPUT_TRIGGER: local-debug\n          INPUT_WORKING_DIRECTORY: sandbox/repos/{dest}/user-repo\n          INPUT_INSTALL_OPENCODE: true\n          INPUT_BINARY_PATH: {}\n",
            host_binary_rel_path.expect("host binary path for host-mode act")
        )
    };

    format!(
        "name: ForkSync Local Dev Demo\n\non:\n  workflow_dispatch:\n\njobs:\n  demo:\n    runs-on: ubuntu-latest\n    defaults:\n      run:\n        working-directory: ${{{{ github.workspace }}}}\n    steps:\n      - name: Checkout repo\n        uses: actions/checkout@v4\n      - name: Cache ForkSync action runtime\n        uses: actions/cache@v4\n        with:\n          path: ~/.cache/forksync\n          key: ${{{{ runner.os }}}}-forksync-local-action\n          restore-keys: |\n            ${{{{ runner.os }}}}-forksync-local-\n      - name: Prepare local sync scenario\n        shell: bash\n        run: |\n{setup_block}\n      - name: Run local ForkSync action\n        uses: ./\n        env:\n{action_env}        with:\n{action_inputs}\n      - name: Show final README on main\n        shell: bash\n        run: |\n          git -C sandbox/repos/{dest}/user-repo show main:README.md\n      - name: Show final README on forksync/live\n        shell: bash\n        run: |\n          git -C sandbox/repos/{dest}/user-repo show forksync/live:README.md\n",
        setup_block = indent_block(&setup_block, "          "),
        action_env = action_env,
        action_inputs = action_inputs,
        dest = dest
    )
}

fn print_filtered_act_output(output: &[u8], success: bool) {
    let rendered = String::from_utf8_lossy(output);
    for line in rendered.lines() {
        if let Some(filtered) = filter_act_line(line, success) {
            println!("{filtered}");
        }
    }
}

fn filter_act_line(line: &str, success: bool) -> Option<String> {
    let stripped = line.trim_end();
    if stripped.is_empty()
        || stripped.starts_with("INFO[")
        || stripped.starts_with("WARN ")
        || stripped.starts_with("time=\"")
        || stripped.starts_with("level=")
    {
        return None;
    }

    let content = if let Some((_, rest)) = stripped.split_once("| ") {
        rest.trim().to_string()
    } else if success {
        return None;
    } else {
        stripped.to_string()
    };

    if content.is_empty()
        || content.starts_with("Compiling ")
        || content.starts_with("Finished `")
        || content.starts_with("Running `")
        || content.starts_with("cargo ")
        || content.starts_with("git version ")
    {
        return None;
    }

    Some(
        content
            .replace(['⭐', '✅', '🏁', '⚠', '❌'], "")
            .trim()
            .to_string(),
    )
}

fn print_init_demo_summary(stdout: &str) {
    let branches = find_prefixed_line(stdout, "Branches: ")
        .unwrap_or_else(|| String::from("patch=forksync/patches, live=forksync/live, output=main"));
    let bootstrap = find_prefixed_line(stdout, "Bootstrap commit: ");
    println!("Initialized ForkSync with {branches}");
    if let Some(bootstrap) = bootstrap {
        println!("Bootstrap commit: {bootstrap}");
    }
}

fn print_sync_demo_summary(stdout: &str) {
    if let Some(outcome) = find_prefixed_line(stdout, "Sync outcome: ") {
        println!("Sync outcome: {outcome}");
    }
    if let Some(upstream_sha) = find_prefixed_line(stdout, "Upstream SHA: ") {
        println!("Upstream SHA: {upstream_sha}");
    }
    for line in stdout.lines().filter(|line| line.starts_with("- ")) {
        println!("{line}");
    }
}

fn find_prefixed_line(output: &str, prefix: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix).map(str::to_string))
}

fn print_block(contents: &str) {
    for line in contents.lines() {
        println!("  {line}");
    }
}

fn indent_block(contents: &str, prefix: &str) -> String {
    contents
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn shell_escape_path(path: &Path) -> String {
    let rendered = path.display().to_string();
    format!("\"{rendered}\"")
}

fn resolve_path(repo_path: &Path, configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        repo_path.join(configured)
    }
}

fn load_repo_config(config_path: &Path) -> Result<RepoConfig> {
    match load_from_path(config_path) {
        Ok(config) => Ok(config),
        Err(ConfigIoError::Read { path, source })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Err(anyhow!(
                "no ForkSync config found at {}. Run `forksync init` from the fork repo root first, or pass `--config` to point at an existing config file.",
                path.display()
            ))
        }
        Err(error) => Err(error.into()),
    }
}
