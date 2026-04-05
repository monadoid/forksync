use forksync_agent::AgentFactory;
use forksync_config::{RepoConfig, TriggerSource};
use forksync_git::GitBackend;
use forksync_github::FailureReporter;
use forksync_state::StateStore;
use std::path::PathBuf;
use thiserror::Error;

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
    pub notes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("sync engine is not implemented")]
    NotImplemented,
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

    pub fn sync(&self, _request: &SyncRequest) -> Result<SyncReport, EngineError> {
        let _ = (&self.git, &self.agents, &self.state, &self.failure_reporter);
        Err(EngineError::NotImplemented)
    }
}
