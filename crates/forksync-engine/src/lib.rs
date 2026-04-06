use forksync_agent::{AgentFactory, AgentRepairOutcome, AgentRepairRequest, RepairTrigger};
use forksync_config::{
    AgentProvider, ConfigIoError, DEFAULT_CONFIG_PATH, DEFAULT_STATE_FILE, DEFAULT_WORKFLOW_PATH,
    NamedCommand, RepoConfig, RunnerPreset, TriggerSource, ValidationConfig, ValidationMode,
    load_from_path, write_to_path,
};
use forksync_git::{
    GitBackend, GitError, LeasedRefUpdate, PatchDerivationRequest, RemoteSpec, ReplayRequest,
    ReplayStatus,
};
use forksync_github::{
    FailureDetails, FailureReporter, build_failure_pr_payload, generate_sync_workflow,
};
use forksync_state::{PersistedState, RecordedOutcome, RunRecord, StateError, StateStore};
use fs4::fs_std::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::TempDir;
use thiserror::Error;
use tracing::{info, instrument, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRequest {
    pub repo_path: PathBuf,
    pub config_path: PathBuf,
    pub workflow_path: PathBuf,
    pub force: bool,
    pub detect_upstream: bool,
    pub initial_sync: bool,
    pub install_workflow: bool,
    pub create_branches: bool,
    pub runner: RunnerPreset,
    pub upstream_remote: Option<String>,
    pub upstream_repo: Option<String>,
    pub upstream_branch: Option<String>,
    pub build_command: Option<String>,
    pub test_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    pub config_path: PathBuf,
    pub workflow_path: Option<PathBuf>,
    pub upstream_remote: String,
    pub upstream_repo: String,
    pub upstream_branch: String,
    pub patch_branch: String,
    pub live_branch: String,
    pub output_branch: String,
    pub bootstrap_commit_sha: String,
    pub pushed_branches: Vec<String>,
    pub failed_push_branches: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRequest {
    pub repo_path: PathBuf,
    pub config_path: PathBuf,
    pub workflow_path: PathBuf,
    pub config: RepoConfig,
    pub trigger: Option<TriggerSource>,
    pub dry_run: bool,
    pub force: bool,
    pub disable_agent: bool,
    pub disable_validation: bool,
    pub upstream_sha: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    NoChange,
    SyncedDeterministic,
    SyncedAgentic,
    FailedValidation,
    FailedAgent,
    FailedAuth,
    FailedInfra,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub outcome: SyncOutcome,
    pub used_agent: bool,
    pub upstream_sha: Option<String>,
    pub patch_commits_applied: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Config(#[from] ConfigIoError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("refusing to overwrite existing file without --force: {path}")]
    PathExists { path: PathBuf },
    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create temporary worktree staging directory: {source}")]
    CreateTempDir {
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("path {path} must be inside repository {repo_path}")]
    PathOutsideRepo { path: PathBuf, repo_path: PathBuf },
    #[error("sync requires a clean worktree")]
    DirtyWorktree,
    #[error("another ForkSync sync is already running for this repository: {lock_path}")]
    ConcurrentSync { lock_path: PathBuf },
    #[error("failed to acquire sync lock at {path}: {source}")]
    AcquireSyncLock {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("validation configuration is incomplete: {message}")]
    ValidationConfiguration { message: String },
    #[error("failed to execute validation command `{command}` in {working_directory}: {source}")]
    ValidationCommandIo {
        command: String,
        working_directory: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BranchUpdateOutcome {
    ResetBackground,
    ResetCheckedOut,
    SkippedCheckedOut,
}

#[derive(Debug, Clone)]
struct ValidationStep {
    name: String,
    command: String,
    required: bool,
}

#[derive(Debug, Clone)]
struct ValidationFailure {
    notes: Vec<String>,
}

enum ValidationFailureOrError {
    Failure(ValidationFailure),
    Engine(EngineError),
}

struct RepoSyncLock {
    _file: File,
}

impl RepoSyncLock {
    // This lock only protects one checkout on one machine. GitHub-hosted runs
    // must rely on remote leased pushes plus workflow concurrency instead.
    fn try_acquire(lock_path: &Path, trigger: Option<TriggerSource>) -> Result<Self, EngineError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| EngineError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|source| EngineError::AcquireSyncLock {
                path: lock_path.to_path_buf(),
                source,
            })?;

        match file.try_lock_exclusive() {
            Ok(true) => {}
            Ok(false) => {
                return Err(EngineError::ConcurrentSync {
                    lock_path: lock_path.to_path_buf(),
                });
            }
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(EngineError::ConcurrentSync {
                    lock_path: lock_path.to_path_buf(),
                });
            }
            Err(source) => {
                return Err(EngineError::AcquireSyncLock {
                    path: lock_path.to_path_buf(),
                    source,
                });
            }
        }

        let trigger_label = trigger
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "unspecified".to_string());
        let lock_metadata = format!("pid={}\ntrigger={}\n", std::process::id(), trigger_label);
        file.set_len(0)
            .map_err(|source| EngineError::AcquireSyncLock {
                path: lock_path.to_path_buf(),
                source,
            })?;
        file.write_all(lock_metadata.as_bytes()).map_err(|source| {
            EngineError::AcquireSyncLock {
                path: lock_path.to_path_buf(),
                source,
            }
        })?;
        file.sync_data()
            .map_err(|source| EngineError::AcquireSyncLock {
                path: lock_path.to_path_buf(),
                source,
            })?;

        Ok(Self { _file: file })
    }
}

pub struct SyncEngine<G, A, S, R> {
    git: G,
    agents: A,
    state: S,
    failure_reporter: R,
}

impl<G, A, S, R> SyncEngine<G, A, S, R>
where
    G: GitBackend,
    A: AgentFactory,
    S: StateStore,
    R: FailureReporter,
{
    pub fn new(git: G, agents: A, state: S, failure_reporter: R) -> Self {
        Self {
            git,
            agents,
            state,
            failure_reporter,
        }
    }

    #[instrument(skip_all, fields(repo_path = %request.repo_path.display()))]
    pub fn init(&self, request: &InitRequest) -> Result<InitReport, EngineError> {
        self.git.ensure_repo(&request.repo_path)?;

        if !request.force && request.config_path.exists() {
            return self.report_existing_init(request);
        }

        if !request.force && request.install_workflow && request.workflow_path.exists() {
            return Err(EngineError::PathExists {
                path: request.workflow_path.clone(),
            });
        }

        let upstream_remote = self.resolve_upstream_remote(request)?;
        info!(upstream_remote = %upstream_remote, "bootstrapping repository with ForkSync");
        self.git.fetch_remote(
            &request.repo_path,
            &RemoteSpec {
                name: upstream_remote.clone(),
            },
        )?;

        let upstream_repo = match &request.upstream_repo {
            Some(repo) => repo.clone(),
            None => self
                .git
                .get_remote_url(&request.repo_path, &upstream_remote)?,
        };

        let upstream_branch = match &request.upstream_branch {
            Some(branch) => branch.clone(),
            None => self
                .git
                .default_branch_for_remote(&request.repo_path, &upstream_remote)?,
        };

        let current_head = self.git.head_sha(&request.repo_path)?;
        let current_ref = self.git.current_ref(&request.repo_path)?;
        let worktree_clean = self.git.worktree_clean(&request.repo_path)?;
        let output_branch = if self.git.remote_exists(&request.repo_path, "origin")? {
            self.git
                .default_branch_for_remote(&request.repo_path, "origin")
                .unwrap_or_else(|_| upstream_branch.clone())
        } else if current_ref == "HEAD" || current_ref.len() == 40 {
            upstream_branch.clone()
        } else {
            current_ref.clone()
        };

        let mut config = RepoConfig::for_init(upstream_repo.clone(), upstream_branch.clone());
        config.upstream.remote_name = upstream_remote.clone();
        config.branches.output = output_branch.clone();
        config.workflow.runner = request.runner;
        apply_init_validation_defaults(&mut config, request)?;

        if output_branch != "main" {
            config.branches.output_mode = forksync_config::OutputMode::Custom;
        }
        let bootstrap_commit_sha = self.write_managed_commit(
            &request.repo_path,
            &request.config_path,
            &request.workflow_path,
            request.install_workflow,
            &config,
            &current_head,
            "Initialize ForkSync bootstrap",
        )?;
        let workflow_path = request
            .install_workflow
            .then(|| request.workflow_path.clone());
        ensure_local_exclude_rules(&self.git.git_dir(&request.repo_path)?)?;

        let mut local_branch_updates = Vec::new();
        if request.create_branches {
            local_branch_updates.push((
                config.branches.patch.clone(),
                self.update_local_branch(
                    &request.repo_path,
                    &config.branches.patch,
                    &bootstrap_commit_sha,
                    &current_ref,
                    worktree_clean,
                )?,
            ));
            local_branch_updates.push((
                config.branches.live.clone(),
                self.update_local_branch(
                    &request.repo_path,
                    &config.branches.live,
                    &bootstrap_commit_sha,
                    &current_ref,
                    worktree_clean,
                )?,
            ));
        }
        local_branch_updates.push((
            config.branches.output.clone(),
            self.update_local_branch(
                &request.repo_path,
                &config.branches.output,
                &bootstrap_commit_sha,
                &current_ref,
                worktree_clean,
            )?,
        ));

        let initial_state = PersistedState {
            last_processed_upstream_sha: None,
            last_good_sync_sha: Some(bootstrap_commit_sha.clone()),
            author_base_sha: Some(bootstrap_commit_sha.clone()),
            history: Vec::new(),
        };
        self.state.save(&initial_state)?;

        let mut notes = vec![
            "Generated the ForkSync bootstrap commit from typed defaults in a detached temporary worktree.".to_string(),
            "Ensured local ForkSync state paths are ignored by Git.".to_string(),
        ];

        if request.create_branches {
            notes.push(format!(
                "Prepared {} and {} from the bootstrap commit without switching your current checkout.",
                config.branches.patch, config.branches.live
            ));
        }

        for (branch, outcome) in &local_branch_updates {
            match outcome {
                BranchUpdateOutcome::ResetBackground => notes.push(format!(
                    "Updated local branch {} in the background.",
                    branch
                )),
                BranchUpdateOutcome::ResetCheckedOut => notes.push(format!(
                    "Updated your checked-out branch {} to the ForkSync bootstrap commit.",
                    branch
                )),
                BranchUpdateOutcome::SkippedCheckedOut => notes.push(format!(
                    "Left checked-out branch {} untouched locally because it is not safe to rewrite in place. Switch to the managed output branch when you are ready to author there.",
                    branch
                )),
            }
        }

        if current_ref != output_branch {
            notes.push(format!(
                "Detected output branch {} while leaving your current branch {} checked out.",
                output_branch, current_ref
            ));
        }

        if request.initial_sync {
            notes.push(
                "Initial sync is still intentionally deferred; run `forksync sync --trigger local-debug` after you add your first commit on the output branch."
                    .to_string(),
            );
        }

        if config.validation.mode != ValidationMode::None {
            notes.push(format!(
                "Configured {:?} validation from init flags.",
                config.validation.mode
            ));
        }

        if upstream_remote == "origin" {
            notes.push(
                "No dedicated upstream remote was detected, so init fell back to origin."
                    .to_string(),
            );
        }

        let (pushed_branches, failed_push_branches) = self.push_init_branches(
            &request.repo_path,
            &config,
            &bootstrap_commit_sha,
            request.create_branches,
            &mut notes,
        )?;
        if pushed_branches.is_empty() {
            notes.push(
                "Did not push bootstrap refs automatically. If you have an origin remote, push the managed branches manually."
                    .to_string(),
            );
        } else {
            notes.push(format!(
                "Pushed bootstrap refs to origin: {}.",
                pushed_branches.join(", ")
            ));
        }

        if current_ref == output_branch {
            if worktree_clean {
                notes.push(format!(
                    "You can keep working directly on {} now.",
                    output_branch
                ));
            } else {
                notes.push(format!(
                    "You can keep working on {}. ForkSync left your existing local changes untouched.",
                    output_branch
                ));
            }
        } else {
            notes.push(format!(
                "When you are ready to start making custom fork changes, switch to {}.",
                output_branch
            ));
        }

        Ok(InitReport {
            config_path: request.config_path.clone(),
            workflow_path,
            upstream_remote,
            upstream_repo,
            upstream_branch,
            patch_branch: config.branches.patch,
            live_branch: config.branches.live,
            output_branch,
            bootstrap_commit_sha,
            pushed_branches,
            failed_push_branches,
            notes,
        })
    }

    #[instrument(skip_all, fields(repo_path = %request.repo_path.display(), trigger = ?request.trigger))]
    pub fn sync(&self, request: &SyncRequest) -> Result<SyncReport, EngineError> {
        self.git.ensure_repo(&request.repo_path)?;
        let _sync_lock = RepoSyncLock::try_acquire(
            &default_sync_lock_path(&request.repo_path, &request.config),
            request.trigger,
        )?;

        if !self.git.worktree_clean(&request.repo_path)? {
            return Err(EngineError::DirtyWorktree);
        }

        let _ = &self.failure_reporter;

        let remote_name = request.config.upstream.remote_name.clone();
        self.git.fetch_remote(
            &request.repo_path,
            &RemoteSpec {
                name: remote_name.clone(),
            },
        )?;
        info!(
            remote_name = %remote_name,
            branch = %request.config.upstream.branch,
            "fetched upstream state"
        );

        let origin_exists = self.git.remote_exists(&request.repo_path, "origin")?;
        let observed_remote_live_sha = if !request.dry_run && origin_exists {
            self.git.resolve_remote_branch_tip(
                &request.repo_path,
                "origin",
                &request.config.branches.live,
            )?
        } else {
            None
        };
        let observed_remote_output_sha =
            if !request.dry_run && origin_exists && request.config.sync.update_output_branch {
                self.git.resolve_remote_branch_tip(
                    &request.repo_path,
                    "origin",
                    &request.config.branches.output,
                )?
            } else {
                None
            };

        let upstream_sha = match &request.upstream_sha {
            Some(sha) => sha.clone(),
            None => self.git.resolve_remote_head(
                &request.repo_path,
                &remote_name,
                &request.config.upstream.branch,
            )?,
        };

        let mut state = self.state.load()?;
        let original_ref = self.git.current_ref(&request.repo_path)?;
        let author_base = state
            .author_base_sha
            .clone()
            .unwrap_or_else(|| request.config.branches.output.clone());
        let mut ignored_patch_paths = vec![PathBuf::from(DEFAULT_CONFIG_PATH)];
        if request.workflow_path.exists() {
            ignored_patch_paths.push(PathBuf::from(DEFAULT_WORKFLOW_PATH));
        }
        let author_commits = self.git.derive_patch_commits(&PatchDerivationRequest {
            repo_path: request.repo_path.clone(),
            patch_branch: request.config.branches.output.clone(),
            base_ref: author_base,
            ignored_paths: ignored_patch_paths,
        })?;

        if !request.force
            && state.last_processed_upstream_sha.as_deref() == Some(upstream_sha.as_str())
            && author_commits.is_empty()
        {
            info!(upstream_sha = %upstream_sha, "sync exited early with no changes");
            return Ok(SyncReport {
                outcome: SyncOutcome::NoChange,
                used_agent: false,
                upstream_sha: Some(upstream_sha),
                patch_commits_applied: 0,
                notes: vec!["Upstream SHA already processed and no new user commits exist on the output branch.".to_string()],
            });
        }

        if request.config.branches.patch != request.config.branches.output {
            let _ = self.update_local_branch(
                &request.repo_path,
                &request.config.branches.patch,
                &request.config.branches.output,
                &original_ref,
                true,
            )?;
        }

        let candidate_branch = format!("{}/sync", request.config.advanced.temp_branch_prefix);
        self.git
            .create_or_reset_branch(&request.repo_path, &candidate_branch, &upstream_sha)?;
        let managed_commit = self.write_managed_commit(
            &request.repo_path,
            &request.config_path,
            &request.workflow_path,
            request.workflow_path.exists(),
            &request.config,
            &candidate_branch,
            "Refresh ForkSync managed files",
        )?;
        self.git
            .create_or_reset_branch(&request.repo_path, &candidate_branch, &managed_commit)?;

        let mut used_agent = false;
        let mut sync_notes = Vec::new();
        let mut remaining_commits = author_commits.clone();
        let mut applied_commit_count = 0usize;
        let candidate_head = loop {
            let replay = self.git.replay_patch_stack(&ReplayRequest {
                repo_path: request.repo_path.clone(),
                candidate_branch: candidate_branch.clone(),
                patch_commits: remaining_commits.clone(),
            })?;
            applied_commit_count += replay.applied_commits.len();

            if replay.status == ReplayStatus::Clean {
                break replay
                    .head_sha
                    .clone()
                    .unwrap_or_else(|| managed_commit.clone());
            }

            let Some(conflict_commit_sha) = replay.conflict_commit.clone() else {
                return self.finish_failed_agent_sync(
                    &request.config,
                    &request.repo_path,
                    &candidate_branch,
                    &original_ref,
                    request.config.sync.prune_temp_branches,
                    &mut state,
                    upstream_sha,
                    applied_commit_count,
                    vec!["Patch replay reported a conflict without identifying the conflicting commit.".to_string()],
                    true,
                );
            };

            if request.disable_agent
                || !request.config.agent.enabled
                || request.config.agent.provider == AgentProvider::Disabled
            {
                return self.finish_failed_agent_sync(
                    &request.config,
                    &request.repo_path,
                    &candidate_branch,
                    &original_ref,
                    request.config.sync.prune_temp_branches,
                    &mut state,
                    upstream_sha,
                    applied_commit_count,
                    vec![
                        "Patch replay hit a conflict, but agent repair is disabled for this run."
                            .to_string(),
                    ],
                    true,
                );
            }

            let agent = match self.agents.build(&request.config.agent) {
                Ok(agent) => agent,
                Err(error) => {
                    return self.finish_failed_agent_sync(
                        &request.config,
                        &request.repo_path,
                        &candidate_branch,
                        &original_ref,
                        request.config.sync.prune_temp_branches,
                        &mut state,
                        upstream_sha,
                        applied_commit_count,
                        vec![format!(
                            "Patch replay hit a conflict, but the configured agent could not start: {error}"
                        )],
                        true,
                    );
                }
            };

            let repair_request = AgentRepairRequest {
                repo_path: request.repo_path.clone(),
                candidate_branch: candidate_branch.clone(),
                patch_branch: request.config.branches.patch.clone(),
                live_branch: request.config.branches.live.clone(),
                trigger: RepairTrigger::ReplayConflict,
                system_prompt: format!(
                    "Repair the current ForkSync cherry-pick conflict on candidate branch `{}` so that user-authored commits from `{}` keep working on top of upstream `{}`. Resolve the active conflict, make any minimal supporting code edits needed, and stop once the current cherry-pick is ready to continue.",
                    candidate_branch, request.config.branches.output, upstream_sha
                ),
                validation_summary: None,
                conflict_commit_sha: Some(conflict_commit_sha.clone()),
            };

            let repair_result = match agent.repair(&repair_request) {
                Ok(result) => result,
                Err(error) => {
                    return self.finish_failed_agent_sync(
                        &request.config,
                        &request.repo_path,
                        &candidate_branch,
                        &original_ref,
                        request.config.sync.prune_temp_branches,
                        &mut state,
                        upstream_sha,
                        applied_commit_count,
                        vec![format!(
                            "Patch replay hit a conflict, but agent repair failed to run: {error}"
                        )],
                        true,
                    );
                }
            };

            match repair_result.outcome {
                AgentRepairOutcome::Repaired => {
                    used_agent = true;
                    applied_commit_count += 1;
                    sync_notes.push(format!(
                        "Agent repair succeeded via {:?}: {}",
                        agent.provider(),
                        repair_result.summary
                    ));
                    let Some(conflict_index) = remaining_commits
                        .iter()
                        .position(|commit| commit.sha == conflict_commit_sha)
                    else {
                        return self.finish_failed_agent_sync(
                            &request.config,
                            &request.repo_path,
                            &candidate_branch,
                            &original_ref,
                            request.config.sync.prune_temp_branches,
                            &mut state,
                            upstream_sha,
                            applied_commit_count,
                            vec![format!(
                                "Agent repair completed, but ForkSync could not find conflict commit {} in the remaining patch stack.",
                                conflict_commit_sha
                            )],
                            false,
                        );
                    };
                    remaining_commits = remaining_commits
                        .into_iter()
                        .skip(conflict_index + 1)
                        .collect();
                    if remaining_commits.is_empty() {
                        break self.git.head_sha(&request.repo_path)?;
                    }
                }
                AgentRepairOutcome::Failed | AgentRepairOutcome::NoChange => {
                    return self.finish_failed_agent_sync(
                        &request.config,
                        &request.repo_path,
                        &candidate_branch,
                        &original_ref,
                        request.config.sync.prune_temp_branches,
                        &mut state,
                        upstream_sha,
                        applied_commit_count,
                        vec![format!(
                            "Patch replay hit a conflict, and the configured agent did not produce a repaired candidate: {}",
                            repair_result.summary
                        )],
                        true,
                    );
                }
            }
        };

        if !request.disable_validation && request.config.validation.mode != ValidationMode::None {
            match self.run_validation(&request.repo_path, &request.config.validation) {
                Ok(()) => {
                    sync_notes.push(format!(
                        "Validation passed in {:?} mode.",
                        request.config.validation.mode
                    ));
                }
                Err(ValidationFailureOrError::Failure(ValidationFailure { notes })) => {
                    return self.finish_failed_validation_sync(
                        &request.config,
                        &request.repo_path,
                        &candidate_branch,
                        &original_ref,
                        request.config.sync.prune_temp_branches,
                        &mut state,
                        upstream_sha,
                        applied_commit_count,
                        notes,
                    );
                }
                Err(ValidationFailureOrError::Engine(error)) => return Err(error),
            }
        }

        if !request.dry_run && origin_exists {
            let mut remote_updates = vec![LeasedRefUpdate {
                remote_ref: format!("refs/heads/{}", request.config.branches.live),
                expected_old_sha: observed_remote_live_sha,
                new_sha: candidate_head.clone(),
            }];
            if request.config.sync.update_output_branch {
                remote_updates.push(LeasedRefUpdate {
                    remote_ref: format!("refs/heads/{}", request.config.branches.output),
                    expected_old_sha: observed_remote_output_sha,
                    new_sha: candidate_head.clone(),
                });
            }
            self.git
                .push_leased_ref_updates(&request.repo_path, "origin", &remote_updates)?;
            info!(
                ref_count = remote_updates.len(),
                "published managed refs to origin"
            );
        }

        if !request.dry_run {
            self.git.create_or_reset_branch(
                &request.repo_path,
                &request.config.branches.live,
                &candidate_head,
            )?;

            if request.config.sync.update_output_branch {
                self.git.create_or_reset_branch(
                    &request.repo_path,
                    &request.config.branches.output,
                    &candidate_head,
                )?;
            }
        }

        self.git.checkout(&request.repo_path, &original_ref)?;
        if request.config.sync.prune_temp_branches {
            self.git
                .delete_branch(&request.repo_path, &candidate_branch)?;
        }

        state.last_processed_upstream_sha = Some(upstream_sha.clone());
        state.last_good_sync_sha = Some(candidate_head.clone());
        if !request.dry_run && request.config.sync.update_output_branch {
            state.author_base_sha = Some(candidate_head.clone());
        }
        state.history.push(RunRecord {
            recorded_at: "local-debug".to_string(),
            outcome: if used_agent {
                RecordedOutcome::SyncedAgentic
            } else {
                RecordedOutcome::SyncedDeterministic
            },
            upstream_sha: Some(upstream_sha.clone()),
            live_sha: Some(candidate_head.clone()),
            notes: vec![format!(
                "Applied {} user commit(s) from {}.",
                applied_commit_count, request.config.branches.output
            )],
        });
        self.state.save(&state)?;

        let outcome = if used_agent {
            SyncOutcome::SyncedAgentic
        } else {
            SyncOutcome::SyncedDeterministic
        };
        info!(
            outcome = ?outcome,
            used_agent,
            patch_commits_applied = applied_commit_count,
            "sync completed"
        );

        Ok(SyncReport {
            outcome,
            used_agent,
            upstream_sha: Some(upstream_sha),
            patch_commits_applied: applied_commit_count,
            notes: {
                sync_notes.push(format!("Updated {}.", request.config.branches.live));
                if origin_exists {
                    sync_notes.push(
                        "Published managed refs to origin with explicit force-with-lease protection."
                            .to_string(),
                    );
                }
                sync_notes.push(if request.config.sync.update_output_branch {
                    format!(
                        "Updated {} from latest upstream plus user commits on {}.",
                        request.config.branches.output, request.config.branches.output
                    )
                } else {
                    "Skipped output branch update by config.".to_string()
                });
                sync_notes
            },
        })
    }

    fn finish_failed_agent_sync(
        &self,
        config: &RepoConfig,
        repo_path: &Path,
        candidate_branch: &str,
        original_ref: &str,
        prune_temp_branches: bool,
        state: &mut PersistedState,
        upstream_sha: String,
        patch_commits_applied: usize,
        notes: Vec<String>,
        abort_cherry_pick: bool,
    ) -> Result<SyncReport, EngineError> {
        warn!(
            upstream_sha = %upstream_sha,
            patch_commits_applied,
            abort_cherry_pick,
            "sync failed in agent repair path"
        );
        if abort_cherry_pick {
            let _ = self.git.abort_cherry_pick(repo_path);
        }
        self.git.checkout(repo_path, original_ref)?;
        if prune_temp_branches {
            self.git.delete_branch(repo_path, candidate_branch)?;
        }

        state.history.push(RunRecord {
            recorded_at: "local-debug".to_string(),
            outcome: RecordedOutcome::FailedAgent,
            upstream_sha: Some(upstream_sha.clone()),
            live_sha: None,
            notes: notes.clone(),
        });
        self.state.save(state)?;
        self.report_failure_surface(
            config,
            state,
            SyncOutcome::FailedAgent,
            Some(upstream_sha.as_str()),
            &notes,
        );

        Ok(SyncReport {
            outcome: SyncOutcome::FailedAgent,
            used_agent: false,
            upstream_sha: Some(upstream_sha),
            patch_commits_applied,
            notes,
        })
    }

    fn finish_failed_validation_sync(
        &self,
        config: &RepoConfig,
        repo_path: &Path,
        candidate_branch: &str,
        original_ref: &str,
        prune_temp_branches: bool,
        state: &mut PersistedState,
        upstream_sha: String,
        patch_commits_applied: usize,
        notes: Vec<String>,
    ) -> Result<SyncReport, EngineError> {
        warn!(
            upstream_sha = %upstream_sha,
            patch_commits_applied,
            "sync failed in validation path"
        );
        self.git.checkout(repo_path, original_ref)?;
        if prune_temp_branches {
            self.git.delete_branch(repo_path, candidate_branch)?;
        }

        state.history.push(RunRecord {
            recorded_at: "local-debug".to_string(),
            outcome: RecordedOutcome::FailedValidation,
            upstream_sha: Some(upstream_sha.clone()),
            live_sha: None,
            notes: notes.clone(),
        });
        self.state.save(state)?;
        self.report_failure_surface(
            config,
            state,
            SyncOutcome::FailedValidation,
            Some(upstream_sha.as_str()),
            &notes,
        );

        Ok(SyncReport {
            outcome: SyncOutcome::FailedValidation,
            used_agent: false,
            upstream_sha: Some(upstream_sha),
            patch_commits_applied,
            notes,
        })
    }

    fn run_validation(
        &self,
        repo_path: &Path,
        validation: &ValidationConfig,
    ) -> Result<(), ValidationFailureOrError> {
        let working_directory = resolve_validation_workdir(repo_path, validation);
        let steps = validation_steps(validation).map_err(ValidationFailureOrError::Engine)?;

        for step in steps {
            let output = shell_command(&step.command, &working_directory).map_err(|source| {
                ValidationFailureOrError::Engine(EngineError::ValidationCommandIo {
                    command: step.command.clone(),
                    working_directory: working_directory.clone(),
                    source,
                })
            })?;

            if !output.status.success() {
                if !step.required {
                    continue;
                }
                let status = output.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let mut notes = vec![format!(
                    "Validation step `{}` failed with status {}.",
                    step.name, status
                )];
                if !stderr.is_empty() {
                    notes.push(format!("stderr: {}", stderr));
                }
                return Err(ValidationFailureOrError::Failure(ValidationFailure {
                    notes,
                }));
            }
        }

        Ok(())
    }

    fn report_failure_surface(
        &self,
        config: &RepoConfig,
        state: &PersistedState,
        outcome: SyncOutcome,
        upstream_sha: Option<&str>,
        notes: &[String],
    ) {
        let should_open_pr = match outcome {
            SyncOutcome::FailedAgent => {
                config.notifications.on_failure.open_pr && config.safety.open_pr_on_failed_agent
            }
            SyncOutcome::FailedValidation => {
                config.notifications.on_failure.open_pr
                    && config.safety.open_pr_on_failed_validation
            }
            SyncOutcome::FailedAuth | SyncOutcome::FailedInfra => {
                config.notifications.on_failure.open_pr
            }
            SyncOutcome::NoChange
            | SyncOutcome::SyncedDeterministic
            | SyncOutcome::SyncedAgentic => false,
        };
        if !should_open_pr {
            return;
        }

        let failure_count = state
            .history
            .iter()
            .filter(|record| is_failure_record(record.outcome))
            .count();
        let details = FailureDetails {
            outcome: format!("{outcome:?}"),
            upstream_sha: upstream_sha.map(ToOwned::to_owned),
            notes: notes.to_vec(),
            is_first_failure: failure_count == 1,
        };

        let Some(payload) = build_failure_pr_payload(config, &details) else {
            return;
        };

        if let Err(error) = self.failure_reporter.upsert_failure_pr(&payload) {
            warn!(error = %error, outcome = ?outcome, "best-effort failure PR reporting failed");
        }
    }

    fn resolve_upstream_remote(&self, request: &InitRequest) -> Result<String, EngineError> {
        if let Some(remote) = &request.upstream_remote {
            return Ok(remote.clone());
        }

        if request.detect_upstream && self.git.remote_exists(&request.repo_path, "upstream")? {
            return Ok("upstream".to_string());
        }

        Ok("origin".to_string())
    }

    fn report_existing_init(&self, request: &InitRequest) -> Result<InitReport, EngineError> {
        let config = load_from_path(&request.config_path)?;
        let current_head = self.git.head_sha(&request.repo_path)?;
        let state = self.state.load()?;
        let bootstrap_commit_sha = state
            .author_base_sha
            .or(state.last_good_sync_sha)
            .unwrap_or(current_head);

        Ok(InitReport {
            config_path: request.config_path.clone(),
            workflow_path: request.workflow_path.exists().then(|| request.workflow_path.clone()),
            upstream_remote: config.upstream.remote_name.clone(),
            upstream_repo: config.upstream.repo.clone(),
            upstream_branch: config.upstream.branch.clone(),
            patch_branch: config.branches.patch.clone(),
            live_branch: config.branches.live.clone(),
            output_branch: config.branches.output.clone(),
            bootstrap_commit_sha,
            pushed_branches: Vec::new(),
            failed_push_branches: Vec::new(),
            notes: vec![
                "ForkSync is already initialized in this repo. Nothing changed.".to_string(),
                "Re-run `forksync init --force` if you need to regenerate managed files or repair the bootstrap state.".to_string(),
            ],
        })
    }

    fn write_managed_commit(
        &self,
        repo_path: &Path,
        config_path: &Path,
        workflow_path: &Path,
        install_workflow: bool,
        config: &RepoConfig,
        base_ref: &str,
        message: &str,
    ) -> Result<String, EngineError> {
        let temp_root = TempDir::new().map_err(|source| EngineError::CreateTempDir { source })?;
        let worktree_path = temp_root.path().join("bootstrap");
        self.git
            .add_detached_worktree(repo_path, &worktree_path, base_ref)?;

        let result = (|| {
            let config_rel = repo_relative_path(repo_path, config_path)?;
            let config_path = worktree_path.join(&config_rel);
            write_to_path(&config_path, config)?;

            let mut commit_paths = vec![config_rel, PathBuf::from(".gitignore")];

            if install_workflow {
                let workflow_rel = repo_relative_path(repo_path, workflow_path)?;
                let generated = generate_sync_workflow(config);
                write_plain_file(&worktree_path.join(&workflow_rel), &generated.contents)?;
                commit_paths.push(workflow_rel);
            }

            ensure_forksync_gitignore_rules(&worktree_path)?;

            if self.git.paths_clean(&worktree_path, &commit_paths)? {
                return self.git.head_sha(&worktree_path).map_err(EngineError::from);
            }

            self.git
                .commit_paths(&worktree_path, &commit_paths, message)
                .map_err(EngineError::from)
        })();

        let cleanup = self.git.remove_worktree(repo_path, &worktree_path);
        match (result, cleanup) {
            (Ok(commit_sha), Ok(())) => Ok(commit_sha),
            (Err(err), Ok(())) => Err(err),
            (Ok(_), Err(err)) => Err(EngineError::from(err)),
            (Err(err), Err(_)) => Err(err),
        }
    }

    fn update_local_branch(
        &self,
        repo_path: &Path,
        branch: &str,
        target: &str,
        current_ref: &str,
        worktree_clean: bool,
    ) -> Result<BranchUpdateOutcome, EngineError> {
        if current_ref == branch {
            if worktree_clean {
                self.git.hard_reset(repo_path, target)?;
                return Ok(BranchUpdateOutcome::ResetCheckedOut);
            }
            return Ok(BranchUpdateOutcome::SkippedCheckedOut);
        }

        self.git.create_or_reset_branch(repo_path, branch, target)?;
        Ok(BranchUpdateOutcome::ResetBackground)
    }

    fn push_init_branches(
        &self,
        repo_path: &Path,
        config: &RepoConfig,
        bootstrap_commit_sha: &str,
        create_branches: bool,
        notes: &mut Vec<String>,
    ) -> Result<(Vec<String>, Vec<String>), EngineError> {
        if !self.git.remote_exists(repo_path, "origin")? {
            notes.push("No origin remote found, so ForkSync skipped automatic push.".to_string());
            return Ok((Vec::new(), Vec::new()));
        }

        let mut branches = Vec::new();
        if create_branches {
            branches.push(config.branches.patch.clone());
            branches.push(config.branches.live.clone());
        }
        if !branches.contains(&config.branches.output) {
            branches.push(config.branches.output.clone());
        }

        let mut pushed = Vec::new();
        let mut failed = Vec::new();
        for branch in branches {
            let refspec = format!("{bootstrap_commit_sha}:refs/heads/{branch}");
            match self.git.push_refspec(repo_path, "origin", &refspec) {
                Ok(()) => pushed.push(branch),
                Err(err) => {
                    failed.push(branch.clone());
                    notes.push(format!(
                        "Automatic push for branch {} failed: {}",
                        branch, err
                    ));
                }
            }
        }

        Ok((pushed, failed))
    }
}

fn resolve_validation_workdir(repo_path: &Path, validation: &ValidationConfig) -> PathBuf {
    let working_directory = PathBuf::from(&validation.working_directory);
    if working_directory == PathBuf::from(".") {
        repo_path.to_path_buf()
    } else if working_directory.is_absolute() {
        working_directory
    } else {
        repo_path.join(working_directory)
    }
}

fn apply_init_validation_defaults(
    config: &mut RepoConfig,
    request: &InitRequest,
) -> Result<(), EngineError> {
    match (&request.build_command, &request.test_command) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(EngineError::ValidationConfiguration {
            message: "`--test-command` requires `--build-command` during init".to_string(),
        }),
        (Some(build_command), None) => {
            config.validation.mode = ValidationMode::BuildOnly;
            config.validation.build_command = Some(build_command.clone());
            Ok(())
        }
        (Some(build_command), Some(test_command)) => {
            config.validation.mode = ValidationMode::BuildAndTests;
            config.validation.build_command = Some(build_command.clone());
            config.validation.test_command = Some(test_command.clone());
            Ok(())
        }
    }
}

fn validation_steps(validation: &ValidationConfig) -> Result<Vec<ValidationStep>, EngineError> {
    let mut steps = Vec::new();

    if let Some(install_command) = validation.install_command.clone() {
        steps.push(ValidationStep {
            name: "install".to_string(),
            command: install_command,
            required: false,
        });
    }

    match validation.mode {
        ValidationMode::None => {}
        ValidationMode::BuildOnly => {
            steps.push(required_validation_step(
                "build",
                validation.build_command.as_ref(),
            )?);
        }
        ValidationMode::BuildAndTests => {
            steps.push(required_validation_step(
                "build",
                validation.build_command.as_ref(),
            )?);
            steps.push(required_validation_step(
                "test",
                validation.test_command.as_ref(),
            )?);
        }
        ValidationMode::Custom => {
            if validation.additional_commands.is_empty() {
                return Err(EngineError::ValidationConfiguration {
                    message: "validation.mode=custom requires at least one additional command"
                        .to_string(),
                });
            }
            steps.extend(validation.additional_commands.iter().cloned().map(
                |NamedCommand {
                     name,
                     command,
                     required,
                 }| ValidationStep {
                    name,
                    command,
                    required,
                },
            ));
        }
    }

    Ok(steps)
}

fn required_validation_step(
    name: &str,
    command: Option<&String>,
) -> Result<ValidationStep, EngineError> {
    let Some(command) = command.cloned() else {
        return Err(EngineError::ValidationConfiguration {
            message: format!("validation mode requires a `{}` command", name),
        });
    };

    Ok(ValidationStep {
        name: name.to_string(),
        command,
        required: true,
    })
}

fn shell_command(command: &str, cwd: &Path) -> Result<std::process::Output, std::io::Error> {
    let mut process = if cfg!(windows) {
        let mut cmd = ProcessCommand::new("cmd");
        cmd.args(["/C", command]);
        cmd
    } else {
        let mut cmd = ProcessCommand::new("sh");
        cmd.args(["-lc", command]);
        cmd
    };

    process.current_dir(cwd).output()
}

fn is_failure_record(outcome: RecordedOutcome) -> bool {
    matches!(
        outcome,
        RecordedOutcome::FailedValidation
            | RecordedOutcome::FailedAgent
            | RecordedOutcome::FailedAuth
            | RecordedOutcome::FailedInfra
    )
}

pub fn default_state_file_path(repo_path: &std::path::Path, config: &RepoConfig) -> PathBuf {
    repo_path
        .join(&config.storage.state_dir)
        .join(DEFAULT_STATE_FILE)
}

pub fn default_sync_lock_path(repo_path: &std::path::Path, config: &RepoConfig) -> PathBuf {
    repo_path.join(&config.storage.state_dir).join("sync.lock")
}

fn write_plain_file(path: &PathBuf, contents: &str) -> Result<(), EngineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| EngineError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    fs::write(path, contents).map_err(|source| EngineError::WriteFile {
        path: path.clone(),
        source,
    })
}

fn repo_relative_path(repo_path: &Path, path: &Path) -> Result<PathBuf, EngineError> {
    path.strip_prefix(repo_path)
        .map(|relative| relative.to_path_buf())
        .map_err(|_| EngineError::PathOutsideRepo {
            path: path.to_path_buf(),
            repo_path: repo_path.to_path_buf(),
        })
}

fn ensure_local_exclude_rules(git_dir: &Path) -> Result<(), EngineError> {
    let exclude_path = git_dir.join("info/exclude");
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent).map_err(|source| EngineError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let mut contents = if exclude_path.exists() {
        fs::read_to_string(&exclude_path).map_err(|source| EngineError::WriteFile {
            path: exclude_path.clone(),
            source,
        })?
    } else {
        String::new()
    };

    let required_rules = [".forksync/state/", ".forksync/tmp/"];
    let mut changed = false;
    for rule in required_rules {
        if !contents.lines().any(|line| line.trim() == rule) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(rule);
            contents.push('\n');
            changed = true;
        }
    }

    if changed {
        fs::write(&exclude_path, contents).map_err(|source| EngineError::WriteFile {
            path: exclude_path,
            source,
        })?;
    }

    Ok(())
}

fn ensure_forksync_gitignore_rules(repo_path: &std::path::Path) -> Result<(), EngineError> {
    let gitignore_path = repo_path.join(".gitignore");
    let mut contents = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path).map_err(|source| EngineError::WriteFile {
            path: gitignore_path.clone(),
            source,
        })?
    } else {
        String::new()
    };

    let required_rules = [".forksync/state/", ".forksync/tmp/"];
    let mut changed = false;
    for rule in required_rules {
        if !contents.lines().any(|line| line.trim() == rule) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(rule);
            contents.push('\n');
            changed = true;
        }
    }

    if changed {
        fs::write(&gitignore_path, contents).map_err(|source| EngineError::WriteFile {
            path: gitignore_path,
            source,
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_init_validation_defaults_sets_build_and_test_mode() {
        let mut config = RepoConfig::default();
        let request = base_init_request();
        let request = InitRequest {
            build_command: Some("cargo build --workspace".to_string()),
            test_command: Some("cargo test --workspace".to_string()),
            ..request
        };

        apply_init_validation_defaults(&mut config, &request).expect("apply init validation");

        assert_eq!(config.validation.mode, ValidationMode::BuildAndTests);
        assert_eq!(
            config.validation.build_command.as_deref(),
            Some("cargo build --workspace")
        );
        assert_eq!(
            config.validation.test_command.as_deref(),
            Some("cargo test --workspace")
        );
    }

    #[test]
    fn apply_init_validation_defaults_rejects_test_without_build() {
        let mut config = RepoConfig::default();
        let request = InitRequest {
            test_command: Some("cargo test --workspace".to_string()),
            ..base_init_request()
        };

        let error = apply_init_validation_defaults(&mut config, &request)
            .expect_err("test command without build command should fail");

        assert!(matches!(error, EngineError::ValidationConfiguration { .. }));
        assert_eq!(config.validation.mode, ValidationMode::None);
    }

    #[test]
    fn validation_steps_require_build_command_for_build_only_mode() {
        let validation = ValidationConfig {
            mode: ValidationMode::BuildOnly,
            ..ValidationConfig::default()
        };

        let error = validation_steps(&validation).expect_err("missing build command should fail");

        assert!(matches!(error, EngineError::ValidationConfiguration { .. }));
    }

    #[test]
    fn validation_steps_expand_build_and_test_sequence() {
        let validation = ValidationConfig {
            mode: ValidationMode::BuildAndTests,
            build_command: Some("cargo build --workspace".to_string()),
            test_command: Some("cargo test --workspace".to_string()),
            ..ValidationConfig::default()
        };

        let steps = validation_steps(&validation).expect("validation steps");

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[0].command, "cargo build --workspace");
        assert!(steps[0].required);
        assert_eq!(steps[1].name, "test");
        assert_eq!(steps[1].command, "cargo test --workspace");
        assert!(steps[1].required);
    }

    fn base_init_request() -> InitRequest {
        InitRequest {
            repo_path: PathBuf::from("/tmp/forksync"),
            config_path: PathBuf::from("/tmp/forksync/.forksync.yml"),
            workflow_path: PathBuf::from("/tmp/forksync/.github/workflows/forksync.yml"),
            force: false,
            detect_upstream: true,
            initial_sync: false,
            install_workflow: true,
            create_branches: true,
            runner: RunnerPreset::UbuntuLatest,
            upstream_remote: None,
            upstream_repo: None,
            upstream_branch: None,
            build_command: None,
            test_command: None,
        }
    }
}
