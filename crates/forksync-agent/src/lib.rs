use forksync_config::{AgentConfig, AgentProvider};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairTrigger {
    ReplayConflict,
    ValidationFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRepairRequest {
    pub repo_path: PathBuf,
    pub candidate_branch: String,
    pub patch_branch: String,
    pub live_branch: String,
    pub trigger: RepairTrigger,
    pub system_prompt: String,
    pub validation_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRepairOutcome {
    Repaired,
    NeedsHumanReview,
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRepairResult {
    pub outcome: AgentRepairOutcome,
    pub summary: String,
    pub commit_sha: Option<String>,
    pub files_changed: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent provider {0:?} is not yet implemented")]
    ProviderNotImplemented(AgentProvider),
}

pub trait CodingAgent: Send + Sync {
    fn provider(&self) -> AgentProvider;
    fn repair(&self, request: &AgentRepairRequest) -> Result<AgentRepairResult, AgentError>;
}

pub trait AgentFactory: Send + Sync {
    fn build(&self, config: &AgentConfig) -> Result<Box<dyn CodingAgent>, AgentError>;
}

#[derive(Debug, Default)]
pub struct OpenCodeFactory;

impl AgentFactory for OpenCodeFactory {
    fn build(&self, config: &AgentConfig) -> Result<Box<dyn CodingAgent>, AgentError> {
        Ok(Box::new(OpenCodeAgent {
            config: config.clone(),
        }))
    }
}

#[derive(Debug, Clone)]
pub struct OpenCodeAgent {
    config: AgentConfig,
}

impl CodingAgent for OpenCodeAgent {
    fn provider(&self) -> AgentProvider {
        self.config.provider
    }

    fn repair(&self, _request: &AgentRepairRequest) -> Result<AgentRepairResult, AgentError> {
        Err(AgentError::ProviderNotImplemented(self.config.provider))
    }
}
