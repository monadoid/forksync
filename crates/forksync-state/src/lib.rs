use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
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
    #[error("failed to create state directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read state file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse state file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("failed to serialize state file {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("failed to write state file {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub trait StateStore: Send + Sync {
    fn load(&self) -> Result<PersistedState, StateError>;
    fn save(&self, state: &PersistedState) -> Result<(), StateError>;
}

#[derive(Debug, Clone)]
pub struct FileStateStore {
    path: PathBuf,
}

impl FileStateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl StateStore for FileStateStore {
    fn load(&self) -> Result<PersistedState, StateError> {
        if !self.path.exists() {
            return Ok(PersistedState::default());
        }

        let contents = fs::read_to_string(&self.path).map_err(|source| StateError::Read {
            path: self.path.clone(),
            source,
        })?;

        serde_yaml::from_str(&contents).map_err(|source| StateError::Parse {
            path: self.path.clone(),
            source,
        })
    }

    fn save(&self, state: &PersistedState) -> Result<(), StateError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| StateError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let rendered = serde_yaml::to_string(state).map_err(|source| StateError::Serialize {
            path: self.path.clone(),
            source,
        })?;

        fs::write(&self.path, rendered).map_err(|source| StateError::Write {
            path: self.path.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_state_file_loads_default_state() {
        let temp = tempdir().expect("create tempdir");
        let store = FileStateStore::new(temp.path().join("state.yml"));

        let state = store.load().expect("load default state");

        assert_eq!(state, PersistedState::default());
    }

    #[test]
    fn save_then_load_round_trips_state() {
        let temp = tempdir().expect("create tempdir");
        let store = FileStateStore::new(temp.path().join("nested/state.yml"));
        let expected = PersistedState {
            last_processed_upstream_sha: Some("abc123".to_string()),
            last_good_sync_sha: Some("def456".to_string()),
            patch_base_sha: Some("base789".to_string()),
            history: vec![RunRecord {
                recorded_at: "2026-04-05T09:00:00Z".to_string(),
                outcome: RecordedOutcome::SyncedDeterministic,
                upstream_sha: Some("abc123".to_string()),
                live_sha: Some("def456".to_string()),
                notes: vec!["clean sync".to_string()],
            }],
        };

        store.save(&expected).expect("save state");
        let actual = store.load().expect("reload state");

        assert_eq!(actual, expected);
    }
}
