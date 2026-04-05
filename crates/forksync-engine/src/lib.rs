use forksync_agent::AgentFactory;
use forksync_config::{
    ConfigIoError, DEFAULT_STATE_FILE, RepoConfig, RunnerPreset, TriggerSource, ValidationMode,
    write_to_path,
};
use forksync_git::{
    GitBackend, GitError, PatchDerivationRequest, RemoteSpec, ReplayRequest, ReplayStatus,
};
use forksync_github::{FailureReporter, generate_sync_workflow};
use forksync_state::{PersistedState, RecordedOutcome, RunRecord, StateError, StateStore};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

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
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRequest {
    pub repo_path: PathBuf,
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
    NeedsHumanReview,
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
    #[error("failed to write file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sync requires a clean worktree")]
    DirtyWorktree,
    #[error("validation mode `{0:?}` is not implemented in the local dogfood slice yet")]
    UnsupportedValidation(ValidationMode),
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

    pub fn init(&self, request: &InitRequest) -> Result<InitReport, EngineError> {
        self.git.ensure_repo(&request.repo_path)?;

        if !request.force && request.config_path.exists() {
            return Err(EngineError::PathExists {
                path: request.config_path.clone(),
            });
        }

        if !request.force && request.install_workflow && request.workflow_path.exists() {
            return Err(EngineError::PathExists {
                path: request.workflow_path.clone(),
            });
        }

        let upstream_remote = self.resolve_upstream_remote(request)?;
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

        let current_ref = self.git.current_ref(&request.repo_path)?;
        let current_head = self.git.head_sha(&request.repo_path)?;
        let output_branch = if current_ref == "HEAD" || current_ref.len() == 40 {
            upstream_branch.clone()
        } else {
            current_ref.clone()
        };

        let mut config = RepoConfig::for_init(upstream_repo.clone(), upstream_branch.clone());
        config.upstream.remote_name = upstream_remote.clone();
        config.branches.output = output_branch.clone();
        config.workflow.runner = request.runner;

        if output_branch != "main" {
            config.branches.output_mode = forksync_config::OutputMode::Custom;
        }

        if request.create_branches {
            self.git.create_or_reset_branch(
                &request.repo_path,
                &config.branches.patch,
                &current_head,
            )?;
            self.git.create_or_reset_branch(
                &request.repo_path,
                &config.branches.live,
                &current_head,
            )?;
            self.git
                .checkout(&request.repo_path, &config.branches.patch)?;
        }

        write_to_path(&request.config_path, &config)?;
        ensure_forksync_gitignore_rules(&request.repo_path)?;

        let workflow_path = if request.install_workflow {
            let generated = generate_sync_workflow(&config);
            write_plain_file(&request.workflow_path, &generated.contents)?;
            Some(request.workflow_path.clone())
        } else {
            None
        };

        let initial_state = PersistedState {
            last_processed_upstream_sha: None,
            last_good_sync_sha: None,
            patch_base_sha: Some(current_head),
            history: Vec::new(),
        };
        self.state.save(&initial_state)?;

        let mut notes = vec![
            "Generated .forksync.yml from detected defaults.".to_string(),
            "Created local management branches for patches and live state.".to_string(),
            "Ensured local ForkSync state paths are ignored by Git.".to_string(),
        ];

        if request.create_branches {
            notes.push(format!(
                "Checked out {} so the generated files can be committed into the patch layer.",
                config.branches.patch
            ));
        }

        if request.initial_sync {
            notes.push(
                "Initial sync is intentionally deferred until after you commit the generated files."
                    .to_string(),
            );
        }

        if upstream_remote == "origin" {
            notes.push(
                "No dedicated upstream remote was detected, so init fell back to origin."
                    .to_string(),
            );
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
            notes,
        })
    }

    pub fn sync(&self, request: &SyncRequest) -> Result<SyncReport, EngineError> {
        self.git.ensure_repo(&request.repo_path)?;

        if !self.git.worktree_clean(&request.repo_path)? {
            return Err(EngineError::DirtyWorktree);
        }

        if !request.disable_validation && request.config.validation.mode != ValidationMode::None {
            return Err(EngineError::UnsupportedValidation(
                request.config.validation.mode,
            ));
        }

        let _ = (&self.agents, &self.failure_reporter);

        let remote_name = request.config.upstream.remote_name.clone();
        self.git.fetch_remote(
            &request.repo_path,
            &RemoteSpec {
                name: remote_name.clone(),
            },
        )?;

        let upstream_sha = match &request.upstream_sha {
            Some(sha) => sha.clone(),
            None => self.git.resolve_remote_head(
                &request.repo_path,
                &remote_name,
                &request.config.upstream.branch,
            )?,
        };

        let mut state = self.state.load()?;
        if !request.force
            && state.last_processed_upstream_sha.as_deref() == Some(upstream_sha.as_str())
        {
            return Ok(SyncReport {
                outcome: SyncOutcome::NoChange,
                used_agent: false,
                upstream_sha: Some(upstream_sha),
                patch_commits_applied: 0,
                notes: vec!["Upstream SHA already processed.".to_string()],
            });
        }

        let original_ref = self.git.current_ref(&request.repo_path)?;
        let candidate_branch = format!("{}/sync", request.config.advanced.temp_branch_prefix);
        self.git
            .create_or_reset_branch(&request.repo_path, &candidate_branch, &upstream_sha)?;

        let patch_base = state
            .patch_base_sha
            .clone()
            .unwrap_or_else(|| upstream_sha.clone());
        let patch_commits = self.git.derive_patch_commits(&PatchDerivationRequest {
            repo_path: request.repo_path.clone(),
            patch_branch: request.config.branches.patch.clone(),
            base_ref: patch_base,
        })?;

        let replay = self.git.replay_patch_stack(&ReplayRequest {
            repo_path: request.repo_path.clone(),
            candidate_branch: candidate_branch.clone(),
            patch_commits: patch_commits.clone(),
        })?;

        if replay.status == ReplayStatus::Conflict {
            self.git.checkout(&request.repo_path, &original_ref)?;
            if request.config.sync.prune_temp_branches {
                self.git
                    .delete_branch(&request.repo_path, &candidate_branch)?;
            }

            state.history.push(RunRecord {
                recorded_at: "local-debug".to_string(),
                outcome: RecordedOutcome::NeedsHumanReview,
                upstream_sha: Some(upstream_sha.clone()),
                live_sha: None,
                notes: vec!["Patch replay hit a cherry-pick conflict.".to_string()],
            });
            self.state.save(&state)?;

            return Ok(SyncReport {
                outcome: SyncOutcome::NeedsHumanReview,
                used_agent: false,
                upstream_sha: Some(upstream_sha),
                patch_commits_applied: replay.applied_commits.len(),
                notes: vec![
                    "Patch replay conflict detected. Agent repair is not wired yet.".to_string(),
                ],
            });
        }

        let candidate_head = replay
            .head_sha
            .clone()
            .unwrap_or_else(|| upstream_sha.clone());

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
        if state.patch_base_sha.is_none() {
            state.patch_base_sha = Some(candidate_head.clone());
        }
        state.history.push(RunRecord {
            recorded_at: "local-debug".to_string(),
            outcome: RecordedOutcome::SyncedDeterministic,
            upstream_sha: Some(upstream_sha.clone()),
            live_sha: Some(candidate_head.clone()),
            notes: vec![format!(
                "Applied {} patch commit(s).",
                replay.applied_commits.len()
            )],
        });
        self.state.save(&state)?;

        Ok(SyncReport {
            outcome: SyncOutcome::SyncedDeterministic,
            used_agent: false,
            upstream_sha: Some(upstream_sha),
            patch_commits_applied: replay.applied_commits.len(),
            notes: vec![
                format!("Updated {}.", request.config.branches.live),
                if request.config.sync.update_output_branch {
                    format!("Updated {}.", request.config.branches.output)
                } else {
                    "Skipped output branch update by config.".to_string()
                },
            ],
        })
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
}

pub fn default_state_file_path(repo_path: &std::path::Path, config: &RepoConfig) -> PathBuf {
    repo_path
        .join(&config.storage.state_dir)
        .join(DEFAULT_STATE_FILE)
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
