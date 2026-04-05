use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSpec {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchDerivationRequest {
    pub repo_path: PathBuf,
    pub patch_branch: String,
    pub recorded_patch_base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchCommit {
    pub sha: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRequest {
    pub repo_path: PathBuf,
    pub candidate_branch: String,
    pub patch_commits: Vec<PatchCommit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStatus {
    Clean,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayResult {
    pub status: ReplayStatus,
    pub applied_commits: Vec<String>,
    pub conflict_commit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchUpdateMode {
    FastForwardOnly,
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchUpdateRequest {
    pub repo_path: PathBuf,
    pub branch: String,
    pub target_sha: String,
    pub mode: BranchUpdateMode,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("git backend is not implemented")]
    NotImplemented,
}

pub trait GitBackend: Send + Sync {
    fn fetch_remote(&self, repo_path: &PathBuf, remote: &RemoteSpec) -> Result<(), GitError>;

    fn resolve_remote_head(
        &self,
        repo_path: &PathBuf,
        remote: &RemoteSpec,
        branch: &str,
    ) -> Result<String, GitError>;

    fn create_candidate_branch(
        &self,
        repo_path: &PathBuf,
        branch: &str,
        from_sha: &str,
    ) -> Result<(), GitError>;

    fn derive_patch_commits(
        &self,
        request: &PatchDerivationRequest,
    ) -> Result<Vec<PatchCommit>, GitError>;

    fn replay_patch_stack(&self, request: &ReplayRequest) -> Result<ReplayResult, GitError>;

    fn update_branch(&self, request: &BranchUpdateRequest) -> Result<(), GitError>;
}
