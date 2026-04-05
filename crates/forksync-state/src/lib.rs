use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PersistedState {
    pub last_processed_upstream_sha: Option<String>,
    pub last_good_sync_sha: Option<String>,
    pub patch_base_sha: Option<String>,
    pub history: Vec<RunRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunRecord {
    pub recorded_at: String,
    pub outcome: RecordedOutcome,
    pub upstream_sha: Option<String>,
    pub live_sha: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordedOutcome {
    NoChange,
    SyncedDeterministic,
    SyncedAgentic,
    FailedValidation,
    FailedAgent,
    FailedAuth,
    FailedInfra,
    NeedsHumanReview,
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("state store is not implemented")]
    NotImplemented,
}

pub trait StateStore: Send + Sync {
    fn load(&self) -> Result<PersistedState, StateError>;
    fn save(&self, state: &PersistedState) -> Result<(), StateError>;
}
