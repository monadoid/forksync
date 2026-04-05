use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureSummary {
    pub title: String,
    pub body: String,
    pub outcome: String,
    pub upstream_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailurePrPayload {
    pub branch: String,
    pub title_prefix: String,
    pub labels: Vec<String>,
    pub assign_users: Vec<String>,
    pub request_review_users: Vec<String>,
    pub mention_users: Vec<String>,
    pub summary: FailureSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailurePrHandle {
    pub number: u64,
    pub url: Option<String>,
}

#[derive(Debug, Error)]
pub enum GithubError {
    #[error("github reporting is not implemented")]
    NotImplemented,
}

pub trait FailureReporter: Send + Sync {
    fn upsert_failure_pr(&self, payload: &FailurePrPayload)
    -> Result<FailurePrHandle, GithubError>;
}
