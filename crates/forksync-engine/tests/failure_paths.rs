use forksync_agent::{AgentFactory, CodingAgent};
use forksync_config::AgentConfig;
use forksync_config::{RepoConfig, TriggerSource, ValidationMode};
use forksync_engine::{EngineError, SyncEngine, SyncRequest};
use forksync_git::{
    GitBackend, GitError, LeasedRefUpdate, PatchCommit, PatchDerivationRequest, RemoteSpec,
    ReplayRequest, ReplayResult, ReplayStatus,
};
use forksync_github::{FailurePrHandle, FailurePrPayload, FailureReporter, GithubError};
use forksync_state::{PersistedState, RunRecord, StateError, StateStore};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

#[test]
fn sync_surfaces_auth_failure_without_touching_state() {
    let temp = TempDir::new().expect("create tempdir");
    let repo_path = temp.path().join("repo");
    fs::create_dir_all(&repo_path).expect("create repo dir");

    let store = RecordingStateStore::new(PersistedState::default());
    let engine = SyncEngine::new(
        AuthFailureGit::default(),
        PanicAgentFactory,
        store.clone(),
        NoopFailureReporter,
    );

    let err = engine
        .sync(&sync_request(&repo_path))
        .expect_err("sync should fail with auth error");

    match err {
        EngineError::Git(GitError::CommandFailed { stderr, .. }) => {
            assert!(stderr.contains("Authentication failed"));
        }
        other => panic!("expected auth failure, got {other:?}"),
    }

    assert_eq!(store.load_count(), 0);
    assert_eq!(store.save_count(), 0);
}

#[test]
fn sync_surfaces_infra_failure_after_candidate_build_without_saving_state() {
    let temp = TempDir::new().expect("create tempdir");
    let repo_path = temp.path().join("repo");
    fs::create_dir_all(&repo_path).expect("create repo dir");

    let store = RecordingStateStore::new(PersistedState {
        author_base_sha: Some("bootstrap".to_string()),
        ..PersistedState::default()
    });
    let engine = SyncEngine::new(
        InfraFailureGit::default(),
        PanicAgentFactory,
        store.clone(),
        NoopFailureReporter,
    );

    let err = engine
        .sync(&sync_request(&repo_path))
        .expect_err("sync should fail with infra error");

    match err {
        EngineError::Git(GitError::Io { source, .. }) => {
            assert_eq!(source.kind(), std::io::ErrorKind::ConnectionReset);
        }
        other => panic!("expected infra failure, got {other:?}"),
    }

    assert_eq!(store.load_count(), 1);
    assert_eq!(store.save_count(), 0);
    assert!(store.saved_history().is_empty());
}

#[test]
fn sync_reports_failure_payload_when_validation_fails() {
    let temp = TempDir::new().expect("create tempdir");
    let repo_path = temp.path().join("repo");
    fs::create_dir_all(&repo_path).expect("create repo dir");

    let store = RecordingStateStore::new(PersistedState {
        author_base_sha: Some("bootstrap".to_string()),
        ..PersistedState::default()
    });
    let reporter = CapturingFailureReporter::default();
    let engine = SyncEngine::new(
        ValidationFailureGit::default(),
        PanicAgentFactory,
        store,
        reporter.clone(),
    );

    let mut request = sync_request(&repo_path);
    request.config.validation.mode = ValidationMode::BuildOnly;
    request.config.validation.build_command = Some("exit 17".to_string());
    request.disable_validation = false;

    let report = engine
        .sync(&request)
        .expect("sync should return failed validation report");

    assert_eq!(
        report.outcome,
        forksync_engine::SyncOutcome::FailedValidation
    );
    let payload = reporter.payloads();
    assert_eq!(payload.len(), 1);
    assert_eq!(payload[0].branch, "forksync/conflicts");
    assert_eq!(payload[0].summary.outcome, "FailedValidation");
    assert!(
        payload[0]
            .summary
            .body
            .contains("Validation step `build` failed")
    );
}

#[test]
fn sync_reports_validation_failure_to_failure_reporter() {
    let temp = TempDir::new().expect("create tempdir");
    let repo_path = temp.path().join("repo");
    fs::create_dir_all(&repo_path).expect("create repo dir");

    let store = RecordingStateStore::new(PersistedState {
        author_base_sha: Some("bootstrap".to_string()),
        ..PersistedState::default()
    });
    let reporter = RecordingFailureReporter::default();
    let engine = SyncEngine::new(
        ValidationFailureGit::default(),
        PanicAgentFactory,
        store.clone(),
        reporter.clone(),
    );

    let mut request = sync_request(&repo_path);
    request.disable_validation = false;
    request.config.validation.mode = ValidationMode::BuildOnly;
    request.config.validation.build_command = Some("exit 17".to_string());

    let report = engine
        .sync(&request)
        .expect("sync should surface failed validation as report");

    assert_eq!(
        report.outcome,
        forksync_engine::SyncOutcome::FailedValidation
    );
    assert_eq!(reporter.payloads().len(), 1);
    let payload = &reporter.payloads()[0];
    assert_eq!(payload.branch, "forksync/conflicts");
    assert_eq!(payload.summary.outcome, "FailedValidation");
    assert!(
        payload
            .summary
            .body
            .contains("Validation step `build` failed with status 17.")
    );
}

#[test]
fn sync_keeps_failed_validation_report_when_failure_pr_reporting_fails() {
    let temp = TempDir::new().expect("create tempdir");
    let repo_path = temp.path().join("repo");
    fs::create_dir_all(&repo_path).expect("create repo dir");

    let store = RecordingStateStore::new(PersistedState {
        author_base_sha: Some("bootstrap".to_string()),
        ..PersistedState::default()
    });
    let reporter = RecordingFailureReporter::failing();
    let engine = SyncEngine::new(
        ValidationFailureGit::default(),
        PanicAgentFactory,
        store.clone(),
        reporter,
    );

    let mut request = sync_request(&repo_path);
    request.disable_validation = false;
    request.config.validation.mode = ValidationMode::BuildOnly;
    request.config.validation.build_command = Some("exit 17".to_string());

    let report = engine
        .sync(&request)
        .expect("sync should still return the validation failure report");

    assert_eq!(
        report.outcome,
        forksync_engine::SyncOutcome::FailedValidation
    );
    assert_eq!(
        store.saved_history().last().map(|record| record.outcome),
        Some(forksync_state::RecordedOutcome::FailedValidation)
    );
}

#[derive(Clone)]
struct RecordingStateStore {
    inner: Arc<Mutex<RecordingState>>,
}

#[derive(Debug, Default, Clone)]
struct CapturingFailureReporter {
    payloads: Arc<Mutex<Vec<FailurePrPayload>>>,
}

impl CapturingFailureReporter {
    fn payloads(&self) -> Vec<FailurePrPayload> {
        self.payloads.lock().expect("lock reporter").clone()
    }
}

#[derive(Default)]
struct RecordingState {
    state: PersistedState,
    load_count: usize,
    save_count: usize,
    saved_history: Vec<RunRecord>,
}

impl RecordingStateStore {
    fn new(state: PersistedState) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RecordingState {
                state,
                ..RecordingState::default()
            })),
        }
    }

    fn load_count(&self) -> usize {
        self.inner.lock().expect("lock store").load_count
    }

    fn save_count(&self) -> usize {
        self.inner.lock().expect("lock store").save_count
    }

    fn saved_history(&self) -> Vec<RunRecord> {
        self.inner.lock().expect("lock store").saved_history.clone()
    }
}

impl StateStore for RecordingStateStore {
    fn load(&self) -> Result<PersistedState, StateError> {
        let mut guard = self.inner.lock().expect("lock store");
        guard.load_count += 1;
        Ok(guard.state.clone())
    }

    fn save(&self, state: &PersistedState) -> Result<(), StateError> {
        let mut guard = self.inner.lock().expect("lock store");
        guard.save_count += 1;
        guard.state = state.clone();
        guard.saved_history = state.history.clone();
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct PanicAgentFactory;

impl AgentFactory for PanicAgentFactory {
    fn build(
        &self,
        _config: &AgentConfig,
    ) -> Result<Box<dyn CodingAgent>, forksync_agent::AgentError> {
        panic!("agent factory should not be called in auth/infra failure tests");
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct NoopFailureReporter;

impl FailureReporter for NoopFailureReporter {
    fn upsert_failure_pr(
        &self,
        _payload: &forksync_github::FailurePrPayload,
    ) -> Result<forksync_github::FailurePrHandle, forksync_github::GithubError> {
        Err(forksync_github::GithubError::NotImplemented)
    }
}

impl FailureReporter for CapturingFailureReporter {
    fn upsert_failure_pr(
        &self,
        payload: &FailurePrPayload,
    ) -> Result<FailurePrHandle, GithubError> {
        self.payloads
            .lock()
            .expect("lock reporter")
            .push(payload.clone());
        Ok(FailurePrHandle {
            number: 1,
            url: Some("https://example.com/pr/1".to_string()),
        })
    }
}

#[derive(Debug, Clone)]
struct RecordingFailureReporter {
    payloads: Arc<Mutex<Vec<forksync_github::FailurePrPayload>>>,
    fail: bool,
}

impl Default for RecordingFailureReporter {
    fn default() -> Self {
        Self {
            payloads: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        }
    }
}

impl RecordingFailureReporter {
    fn failing() -> Self {
        Self {
            payloads: Arc::new(Mutex::new(Vec::new())),
            fail: true,
        }
    }

    fn payloads(&self) -> Vec<forksync_github::FailurePrPayload> {
        self.payloads.lock().expect("lock reporter").clone()
    }
}

impl FailureReporter for RecordingFailureReporter {
    fn upsert_failure_pr(
        &self,
        payload: &forksync_github::FailurePrPayload,
    ) -> Result<forksync_github::FailurePrHandle, forksync_github::GithubError> {
        self.payloads
            .lock()
            .expect("lock reporter")
            .push(payload.clone());
        if self.fail {
            Err(forksync_github::GithubError::NotImplemented)
        } else {
            Ok(forksync_github::FailurePrHandle {
                number: 1,
                url: Some("https://example.com/pr/1".to_string()),
            })
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct AuthFailureGit;

impl GitBackend for AuthFailureGit {
    fn ensure_repo(&self, _repo_path: &Path) -> Result<(), GitError> {
        Ok(())
    }

    fn git_dir(&self, _repo_path: &Path) -> Result<PathBuf, GitError> {
        unreachable!("git_dir is not used in auth failure test")
    }

    fn worktree_clean(&self, _repo_path: &Path) -> Result<bool, GitError> {
        Ok(true)
    }

    fn paths_clean(&self, _repo_path: &Path, _paths: &[PathBuf]) -> Result<bool, GitError> {
        unreachable!("paths_clean is not reached on auth failure")
    }

    fn current_ref(&self, _repo_path: &Path) -> Result<String, GitError> {
        unreachable!("current_ref is not reached on auth failure")
    }

    fn checkout(&self, _repo_path: &Path, _reference: &str) -> Result<(), GitError> {
        unreachable!("checkout is not reached on auth failure")
    }

    fn hard_reset(&self, _repo_path: &Path, _target: &str) -> Result<(), GitError> {
        unreachable!("hard_reset is not reached on auth failure")
    }

    fn head_sha(&self, _repo_path: &Path) -> Result<String, GitError> {
        unreachable!("head_sha is not reached on auth failure")
    }

    fn remote_exists(&self, _repo_path: &Path, _remote_name: &str) -> Result<bool, GitError> {
        unreachable!("remote_exists is not reached on auth failure")
    }

    fn get_remote_url(&self, _repo_path: &Path, _remote_name: &str) -> Result<String, GitError> {
        unreachable!("get_remote_url is not reached on auth failure")
    }

    fn fetch_remote(&self, repo_path: &Path, remote: &RemoteSpec) -> Result<(), GitError> {
        Err(GitError::CommandFailed {
            command: format!("git -C {} fetch {}", repo_path.display(), remote.name),
            status: 128,
            stderr: "fatal: Authentication failed".to_string(),
        })
    }

    fn default_branch_for_remote(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
    ) -> Result<String, GitError> {
        unreachable!("default_branch_for_remote is not reached on auth failure")
    }

    fn resolve_remote_head(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _branch: &str,
    ) -> Result<String, GitError> {
        unreachable!("resolve_remote_head is not reached on auth failure")
    }

    fn resolve_remote_branch_tip(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _branch: &str,
    ) -> Result<Option<String>, GitError> {
        unreachable!("resolve_remote_branch_tip is not reached on auth failure")
    }

    fn local_branch_exists(&self, _repo_path: &Path, _branch: &str) -> Result<bool, GitError> {
        unreachable!("local_branch_exists is not reached on auth failure")
    }

    fn create_or_reset_branch(
        &self,
        _repo_path: &Path,
        _branch: &str,
        _target: &str,
    ) -> Result<(), GitError> {
        unreachable!("create_or_reset_branch is not reached on auth failure")
    }

    fn commit_paths(
        &self,
        _repo_path: &Path,
        _paths: &[PathBuf],
        _message: &str,
    ) -> Result<String, GitError> {
        unreachable!("commit_paths is not reached on auth failure")
    }

    fn push_branch(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _branch: &str,
    ) -> Result<(), GitError> {
        unreachable!("push_branch is not reached on auth failure")
    }

    fn push_refspec(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _refspec: &str,
    ) -> Result<(), GitError> {
        unreachable!("push_refspec is not reached on auth failure")
    }

    fn push_leased_ref_updates(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _updates: &[LeasedRefUpdate],
    ) -> Result<(), GitError> {
        unreachable!("push_leased_ref_updates is not reached on auth failure")
    }

    fn fetch_branch_to_local_ref(
        &self,
        _repo_path: &Path,
        _remote_spec: &str,
        _branch: &str,
        _local_ref: &str,
    ) -> Result<(), GitError> {
        unreachable!("fetch_branch_to_local_ref is not reached on auth failure")
    }

    fn merge_base(&self, _repo_path: &Path, _left: &str, _right: &str) -> Result<String, GitError> {
        unreachable!("merge_base is not reached on auth failure")
    }

    fn add_detached_worktree(
        &self,
        _repo_path: &Path,
        _worktree_path: &Path,
        _target: &str,
    ) -> Result<(), GitError> {
        unreachable!("add_detached_worktree is not reached on auth failure")
    }

    fn remove_worktree(&self, _repo_path: &Path, _worktree_path: &Path) -> Result<(), GitError> {
        unreachable!("remove_worktree is not reached on auth failure")
    }

    fn delete_branch(&self, _repo_path: &Path, _branch: &str) -> Result<(), GitError> {
        unreachable!("delete_branch is not reached on auth failure")
    }

    fn abort_cherry_pick(&self, _repo_path: &Path) -> Result<(), GitError> {
        unreachable!("abort_cherry_pick is not reached on auth failure")
    }

    fn derive_patch_commits(
        &self,
        _request: &PatchDerivationRequest,
    ) -> Result<Vec<PatchCommit>, GitError> {
        unreachable!("derive_patch_commits is not reached on auth failure")
    }

    fn replay_patch_stack(&self, _request: &ReplayRequest) -> Result<ReplayResult, GitError> {
        unreachable!("replay_patch_stack is not reached on auth failure")
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct InfraFailureGit;

impl GitBackend for InfraFailureGit {
    fn ensure_repo(&self, _repo_path: &Path) -> Result<(), GitError> {
        Ok(())
    }

    fn git_dir(&self, _repo_path: &Path) -> Result<PathBuf, GitError> {
        Ok(PathBuf::from("/tmp/forksync-test.git"))
    }

    fn worktree_clean(&self, _repo_path: &Path) -> Result<bool, GitError> {
        Ok(true)
    }

    fn paths_clean(&self, _repo_path: &Path, _paths: &[PathBuf]) -> Result<bool, GitError> {
        Ok(false)
    }

    fn current_ref(&self, _repo_path: &Path) -> Result<String, GitError> {
        Ok("main".to_string())
    }

    fn checkout(&self, _repo_path: &Path, _reference: &str) -> Result<(), GitError> {
        Ok(())
    }

    fn hard_reset(&self, _repo_path: &Path, _target: &str) -> Result<(), GitError> {
        Ok(())
    }

    fn head_sha(&self, _repo_path: &Path) -> Result<String, GitError> {
        Ok("current-head".to_string())
    }

    fn remote_exists(&self, _repo_path: &Path, remote_name: &str) -> Result<bool, GitError> {
        Ok(remote_name == "origin")
    }

    fn get_remote_url(&self, _repo_path: &Path, remote_name: &str) -> Result<String, GitError> {
        Ok(format!("git@example.com/{remote_name}.git"))
    }

    fn fetch_remote(&self, _repo_path: &Path, _remote: &RemoteSpec) -> Result<(), GitError> {
        Ok(())
    }

    fn default_branch_for_remote(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
    ) -> Result<String, GitError> {
        Ok("main".to_string())
    }

    fn resolve_remote_head(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _branch: &str,
    ) -> Result<String, GitError> {
        Ok("upstream-sha".to_string())
    }

    fn resolve_remote_branch_tip(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        branch: &str,
    ) -> Result<Option<String>, GitError> {
        Ok(Some(format!("observed-{branch}-sha")))
    }

    fn local_branch_exists(&self, _repo_path: &Path, _branch: &str) -> Result<bool, GitError> {
        Ok(false)
    }

    fn create_or_reset_branch(
        &self,
        _repo_path: &Path,
        _branch: &str,
        _target: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn commit_paths(
        &self,
        _repo_path: &Path,
        _paths: &[PathBuf],
        _message: &str,
    ) -> Result<String, GitError> {
        Ok("managed-commit".to_string())
    }

    fn push_branch(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _branch: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn push_refspec(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _refspec: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn push_leased_ref_updates(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _updates: &[LeasedRefUpdate],
    ) -> Result<(), GitError> {
        Err(GitError::Io {
            command: "git push --atomic --force-with-lease".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::ConnectionReset, "network dropped"),
        })
    }

    fn fetch_branch_to_local_ref(
        &self,
        _repo_path: &Path,
        _remote_spec: &str,
        _branch: &str,
        _local_ref: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn merge_base(&self, _repo_path: &Path, _left: &str, _right: &str) -> Result<String, GitError> {
        Ok("merge-base".to_string())
    }

    fn add_detached_worktree(
        &self,
        _repo_path: &Path,
        worktree_path: &Path,
        _target: &str,
    ) -> Result<(), GitError> {
        fs::create_dir_all(worktree_path).map_err(|source| GitError::Io {
            command: format!("mkdir -p {}", worktree_path.display()),
            source,
        })?;
        Ok(())
    }

    fn remove_worktree(&self, _repo_path: &Path, worktree_path: &Path) -> Result<(), GitError> {
        let _ = fs::remove_dir_all(worktree_path);
        Ok(())
    }

    fn delete_branch(&self, _repo_path: &Path, _branch: &str) -> Result<(), GitError> {
        Ok(())
    }

    fn abort_cherry_pick(&self, _repo_path: &Path) -> Result<(), GitError> {
        Ok(())
    }

    fn derive_patch_commits(
        &self,
        _request: &PatchDerivationRequest,
    ) -> Result<Vec<PatchCommit>, GitError> {
        Ok(vec![PatchCommit {
            sha: "patch-1".to_string(),
            summary: "local change".to_string(),
            excluded_paths: Vec::new(),
            source_name: None,
        }])
    }

    fn replay_patch_stack(&self, _request: &ReplayRequest) -> Result<ReplayResult, GitError> {
        Ok(ReplayResult {
            status: ReplayStatus::Clean,
            applied_commits: vec!["patch-1".to_string()],
            conflict_commit: None,
            head_sha: Some("candidate-head".to_string()),
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct ValidationFailureGit;

impl GitBackend for ValidationFailureGit {
    fn ensure_repo(&self, _repo_path: &Path) -> Result<(), GitError> {
        Ok(())
    }

    fn git_dir(&self, _repo_path: &Path) -> Result<PathBuf, GitError> {
        Ok(PathBuf::from("/tmp/forksync-test.git"))
    }

    fn worktree_clean(&self, _repo_path: &Path) -> Result<bool, GitError> {
        Ok(true)
    }

    fn paths_clean(&self, _repo_path: &Path, _paths: &[PathBuf]) -> Result<bool, GitError> {
        Ok(false)
    }

    fn current_ref(&self, _repo_path: &Path) -> Result<String, GitError> {
        Ok("main".to_string())
    }

    fn checkout(&self, _repo_path: &Path, _reference: &str) -> Result<(), GitError> {
        Ok(())
    }

    fn hard_reset(&self, _repo_path: &Path, _target: &str) -> Result<(), GitError> {
        Ok(())
    }

    fn head_sha(&self, _repo_path: &Path) -> Result<String, GitError> {
        Ok("current-head".to_string())
    }

    fn remote_exists(&self, _repo_path: &Path, remote_name: &str) -> Result<bool, GitError> {
        Ok(remote_name == "origin")
    }

    fn get_remote_url(&self, _repo_path: &Path, remote_name: &str) -> Result<String, GitError> {
        Ok(format!("git@example.com/{remote_name}.git"))
    }

    fn fetch_remote(&self, _repo_path: &Path, _remote: &RemoteSpec) -> Result<(), GitError> {
        Ok(())
    }

    fn default_branch_for_remote(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
    ) -> Result<String, GitError> {
        Ok("main".to_string())
    }

    fn resolve_remote_head(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _branch: &str,
    ) -> Result<String, GitError> {
        Ok("upstream-sha".to_string())
    }

    fn resolve_remote_branch_tip(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        branch: &str,
    ) -> Result<Option<String>, GitError> {
        Ok(Some(format!("observed-{branch}-sha")))
    }

    fn local_branch_exists(&self, _repo_path: &Path, _branch: &str) -> Result<bool, GitError> {
        Ok(false)
    }

    fn create_or_reset_branch(
        &self,
        _repo_path: &Path,
        _branch: &str,
        _target: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn commit_paths(
        &self,
        _repo_path: &Path,
        _paths: &[PathBuf],
        _message: &str,
    ) -> Result<String, GitError> {
        Ok("managed-commit".to_string())
    }

    fn push_branch(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _branch: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn push_refspec(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _refspec: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn push_leased_ref_updates(
        &self,
        _repo_path: &Path,
        _remote_name: &str,
        _updates: &[LeasedRefUpdate],
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn fetch_branch_to_local_ref(
        &self,
        _repo_path: &Path,
        _remote_spec: &str,
        _branch: &str,
        _local_ref: &str,
    ) -> Result<(), GitError> {
        Ok(())
    }

    fn merge_base(&self, _repo_path: &Path, _left: &str, _right: &str) -> Result<String, GitError> {
        Ok("merge-base".to_string())
    }

    fn add_detached_worktree(
        &self,
        _repo_path: &Path,
        worktree_path: &Path,
        _target: &str,
    ) -> Result<(), GitError> {
        fs::create_dir_all(worktree_path).map_err(|source| GitError::Io {
            command: format!("mkdir -p {}", worktree_path.display()),
            source,
        })?;
        Ok(())
    }

    fn remove_worktree(&self, _repo_path: &Path, worktree_path: &Path) -> Result<(), GitError> {
        let _ = fs::remove_dir_all(worktree_path);
        Ok(())
    }

    fn delete_branch(&self, _repo_path: &Path, _branch: &str) -> Result<(), GitError> {
        Ok(())
    }

    fn abort_cherry_pick(&self, _repo_path: &Path) -> Result<(), GitError> {
        Ok(())
    }

    fn derive_patch_commits(
        &self,
        _request: &PatchDerivationRequest,
    ) -> Result<Vec<PatchCommit>, GitError> {
        Ok(vec![PatchCommit {
            sha: "patch-1".to_string(),
            summary: "local change".to_string(),
            excluded_paths: Vec::new(),
            source_name: None,
        }])
    }

    fn replay_patch_stack(&self, _request: &ReplayRequest) -> Result<ReplayResult, GitError> {
        Ok(ReplayResult {
            status: ReplayStatus::Clean,
            applied_commits: vec!["patch-1".to_string()],
            conflict_commit: None,
            head_sha: Some("candidate-head".to_string()),
        })
    }
}

fn sync_request(repo_path: &Path) -> SyncRequest {
    let mut config = RepoConfig::default();
    config.upstream.remote_name = "upstream".to_string();
    config.upstream.branch = "main".to_string();
    config.branches.patch = "forksync/patches".to_string();
    config.branches.live = "forksync/live".to_string();
    config.branches.output = "main".to_string();
    config.validation.mode = ValidationMode::None;
    config.agent.enabled = false;

    SyncRequest {
        repo_path: repo_path.to_path_buf(),
        config_path: repo_path.join(".forksync.yml"),
        workflow_path: repo_path.join(".github/workflows/forksync.yml"),
        config,
        trigger: Some(TriggerSource::LocalDebug),
        dry_run: false,
        force: false,
        disable_agent: true,
        disable_validation: true,
        upstream_sha: None,
    }
}
