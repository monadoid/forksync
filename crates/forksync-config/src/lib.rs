use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, instrument};

pub const DEFAULT_CONFIG_PATH: &str = ".forksync.yml";
pub const DEFAULT_WORKFLOW_PATH: &str = ".github/workflows/forksync.yml";
pub const DEFAULT_STATE_FILE: &str = "state.yml";

#[derive(Debug, Error)]
pub enum ConfigIoError {
    #[error("failed to read config at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("failed to create parent directory for {path}: {source}")]
    CreateParentDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize config for {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("failed to write config to {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RepoConfig {
    pub version: u32,
    pub product_mode: ProductModeConfig,
    pub upstream: UpstreamConfig,
    pub branches: BranchConfig,
    pub sync: SyncConfig,
    pub validation: ValidationConfig,
    pub agent: AgentConfig,
    pub notifications: NotificationConfig,
    pub auth: AuthConfig,
    pub workflow: WorkflowConfig,
    pub storage: StorageConfig,
    pub safety: SafetyConfig,
    pub future: FutureConfig,
    pub advanced: AdvancedConfig,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            version: 1,
            product_mode: ProductModeConfig::default(),
            upstream: UpstreamConfig::default(),
            branches: BranchConfig::default(),
            sync: SyncConfig::default(),
            validation: ValidationConfig::default(),
            agent: AgentConfig::default(),
            notifications: NotificationConfig::default(),
            auth: AuthConfig::default(),
            workflow: WorkflowConfig::default(),
            storage: StorageConfig::default(),
            safety: SafetyConfig::default(),
            future: FutureConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}

impl RepoConfig {
    pub fn for_init(upstream_repo: impl Into<String>, upstream_branch: impl Into<String>) -> Self {
        let mut config = Self::default();
        config.upstream.repo = upstream_repo.into();
        config.upstream.branch = upstream_branch.into();
        config
    }
}

pub fn from_yaml_str(input: &str) -> Result<RepoConfig, serde_yaml::Error> {
    serde_yaml::from_str(input)
}

pub fn to_yaml_string(config: &RepoConfig) -> Result<String, serde_yaml::Error> {
    serde_yaml::to_string(config)
}

#[instrument(skip_all, fields(path = %path.as_ref().display()))]
pub fn load_from_path(path: impl AsRef<Path>) -> Result<RepoConfig, ConfigIoError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path).map_err(|source| ConfigIoError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let config = from_yaml_str(&contents).map_err(|source| ConfigIoError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    debug!("loaded ForkSync config");
    Ok(config)
}

#[instrument(skip_all, fields(path = %path.as_ref().display()))]
pub fn write_to_path(path: impl AsRef<Path>, config: &RepoConfig) -> Result<(), ConfigIoError> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigIoError::CreateParentDir {
            path: path.to_path_buf(),
            source,
        })?;
    }

    let rendered = to_yaml_string(config).map_err(|source| ConfigIoError::Serialize {
        path: path.to_path_buf(),
        source,
    })?;

    fs::write(path, rendered).map_err(|source| ConfigIoError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    debug!("wrote ForkSync config");
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProductModeConfig {
    pub mode: ProductMode,
    pub allow_future_hosted_migration: bool,
}

impl Default for ProductModeConfig {
    fn default() -> Self {
        Self {
            mode: ProductMode::ActionOnlyPolling,
            allow_future_hosted_migration: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpstreamConfig {
    pub repo: String,
    pub branch: String,
    pub visibility: RepoVisibility,
    pub remote_name: String,
    pub detect_from_fork_parent: bool,
}

impl Default for UpstreamConfig {
    fn default() -> Self {
        Self {
            repo: "auto-detect-parent".to_string(),
            branch: "auto-detect-default".to_string(),
            visibility: RepoVisibility::Auto,
            remote_name: "upstream".to_string(),
            detect_from_fork_parent: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BranchConfig {
    pub patch: String,
    pub live: String,
    pub output: String,
    pub output_mode: OutputMode,
    pub protect_live_branch: bool,
    pub create_missing_branches: bool,
}

impl Default for BranchConfig {
    fn default() -> Self {
        Self {
            patch: "forksync/patches".to_string(),
            live: "forksync/live".to_string(),
            output: "main".to_string(),
            output_mode: OutputMode::Main,
            protect_live_branch: false,
            create_missing_branches: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SyncConfig {
    pub trigger_modes: Vec<TriggerMode>,
    pub poll_cron: String,
    pub strategy: SyncStrategy,
    pub patch_derivation: PatchDerivationMode,
    pub conflict_strategy: ConflictStrategy,
    pub update_output_branch: bool,
    pub direct_push_on_green: bool,
    pub reckless_mode_default: bool,
    pub reuse_failure_pr: bool,
    pub max_concurrent_runs: u32,
    pub dedupe_by_upstream_sha: bool,
    pub reprocess_same_sha_on_force: bool,
    pub prune_temp_branches: bool,
    pub backup_before_update: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            trigger_modes: vec![TriggerMode::Schedule, TriggerMode::WorkflowDispatch],
            poll_cron: "*/15 * * * *".to_string(),
            strategy: SyncStrategy::ReplayPatchStack,
            patch_derivation: PatchDerivationMode::SinceRecordedPatchBase,
            conflict_strategy: ConflictStrategy::AgentRequired,
            update_output_branch: true,
            direct_push_on_green: true,
            reckless_mode_default: true,
            reuse_failure_pr: true,
            max_concurrent_runs: 1,
            dedupe_by_upstream_sha: true,
            reprocess_same_sha_on_force: true,
            prune_temp_branches: true,
            backup_before_update: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ValidationConfig {
    pub mode: ValidationMode,
    pub working_directory: String,
    pub install_command: Option<String>,
    pub build_command: Option<String>,
    pub test_command: Option<String>,
    pub additional_commands: Vec<NamedCommand>,
    pub fail_on_flaky: bool,
    pub fail_on_missing_commands: bool,
    pub timeout_minutes: u32,
    pub future_auto_detect_commands: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            mode: ValidationMode::None,
            working_directory: ".".to_string(),
            install_command: None,
            build_command: None,
            test_command: None,
            additional_commands: Vec::new(),
            fail_on_flaky: true,
            fail_on_missing_commands: false,
            timeout_minutes: 30,
            future_auto_detect_commands: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamedCommand {
    pub name: String,
    pub command: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentConfig {
    pub enabled: bool,
    pub provider: AgentProvider,
    pub model: Option<String>,
    pub credential_mode: AgentCredentialMode,
    pub api_key_secret_name: Option<String>,
    pub hosted_profile: Option<String>,
    pub max_attempts: u32,
    pub max_runtime_minutes: u32,
    pub max_files_changed: Option<u32>,
    pub max_diff_lines: Option<u32>,
    pub allow_edit_any_file: bool,
    pub allow_new_commits: bool,
    pub allow_command_execution: bool,
    pub repair_validation_failures: bool,
    pub prompt_profile: PromptProfile,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: AgentProvider::OpenCode,
            model: Some("opencode/gpt-5-nano".to_string()),
            credential_mode: AgentCredentialMode::None,
            api_key_secret_name: None,
            hosted_profile: None,
            max_attempts: 3,
            max_runtime_minutes: 20,
            max_files_changed: None,
            max_diff_lines: None,
            allow_edit_any_file: true,
            allow_new_commits: true,
            allow_command_execution: true,
            repair_validation_failures: true,
            prompt_profile: PromptProfile::Reckless,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NotificationConfig {
    pub on_success: SuccessNotificationConfig,
    pub on_failure: FailureNotificationConfig,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            on_success: SuccessNotificationConfig::default(),
            on_failure: FailureNotificationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SuccessNotificationConfig {
    pub comment_on_success_pr: bool,
    pub write_job_summary: bool,
    pub create_check_summary: bool,
}

impl Default for SuccessNotificationConfig {
    fn default() -> Self {
        Self {
            comment_on_success_pr: false,
            write_job_summary: true,
            create_check_summary: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FailureNotificationConfig {
    pub open_pr: bool,
    pub reuse_existing_pr: bool,
    pub pr_branch: String,
    pub pr_title_prefix: String,
    pub pr_labels: Vec<String>,
    pub assign_users: Vec<String>,
    pub request_review_users: Vec<String>,
    pub mention_users: Vec<String>,
    pub mention_on_first_failure_only: bool,
}

impl Default for FailureNotificationConfig {
    fn default() -> Self {
        Self {
            open_pr: true,
            reuse_existing_pr: true,
            pr_branch: "forksync/conflicts".to_string(),
            pr_title_prefix: "[ForkSync]".to_string(),
            pr_labels: vec!["forksync".to_string(), "sync-failure".to_string()],
            assign_users: Vec::new(),
            request_review_users: Vec::new(),
            mention_users: Vec::new(),
            mention_on_first_failure_only: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AuthConfig {
    pub upstream_auth: UpstreamAuthConfig,
    pub git_push_auth: GitPushAuthConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            upstream_auth: UpstreamAuthConfig::default(),
            git_push_auth: GitPushAuthConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpstreamAuthConfig {
    pub mode: UpstreamAuthMode,
    pub pat_secret_name: Option<String>,
}

impl Default for UpstreamAuthConfig {
    fn default() -> Self {
        Self {
            mode: UpstreamAuthMode::Anonymous,
            pat_secret_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GitPushAuthConfig {
    pub use_github_token: bool,
    pub require_pull_request_permission: bool,
}

impl Default for GitPushAuthConfig {
    fn default() -> Self {
        Self {
            use_github_token: true,
            require_pull_request_permission: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WorkflowConfig {
    pub runner: RunnerPreset,
    pub timeout_minutes: u32,
    pub permissions: WorkflowPermissionConfig,
    pub action_ref: String,
    pub setup_wizard_enabled: bool,
    pub setup_workflow_dispatch_inputs_enabled: bool,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            runner: RunnerPreset::UbuntuLatest,
            timeout_minutes: 45,
            permissions: WorkflowPermissionConfig::default(),
            action_ref: "samfinton/forksync@main".to_string(),
            setup_wizard_enabled: true,
            setup_workflow_dispatch_inputs_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WorkflowPermissionConfig {
    pub contents: PermissionLevel,
    pub pull_requests: PermissionLevel,
    pub issues: PermissionLevel,
    pub actions: PermissionLevel,
}

impl Default for WorkflowPermissionConfig {
    fn default() -> Self {
        Self {
            contents: PermissionLevel::Write,
            pull_requests: PermissionLevel::Write,
            issues: PermissionLevel::Read,
            actions: PermissionLevel::Read,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StorageConfig {
    pub state_dir: String,
    pub persist_last_processed_upstream_sha: bool,
    pub persist_last_good_sync_sha: bool,
    #[serde(alias = "persist_patch_base_sha")]
    pub persist_author_base_sha: bool,
    pub persist_run_history: bool,
    pub max_history_entries: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            state_dir: ".forksync/state".to_string(),
            persist_last_processed_upstream_sha: true,
            persist_last_good_sync_sha: true,
            persist_author_base_sha: true,
            persist_run_history: true,
            max_history_entries: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SafetyConfig {
    pub open_pr_on_failed_validation: bool,
    pub open_pr_on_failed_agent: bool,
    pub block_on_auth_failures: bool,
    pub block_on_missing_upstream: bool,
    pub allow_force_push_output_branch: bool,
    pub never_expose_extra_repo_secrets_to_agent: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            open_pr_on_failed_validation: true,
            open_pr_on_failed_agent: true,
            block_on_auth_failures: true,
            block_on_missing_upstream: true,
            allow_force_push_output_branch: true,
            never_expose_extra_repo_secrets_to_agent: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FutureConfig {
    pub patch_registry_compatible_ids: bool,
    pub local_patch_identity: Option<PatchIdentityConfig>,
    pub reserved_registry_sources: Vec<RegistrySourceConfig>,
}

impl Default for FutureConfig {
    fn default() -> Self {
        Self {
            patch_registry_compatible_ids: true,
            local_patch_identity: None,
            reserved_registry_sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchIdentityConfig {
    pub patch_id: Option<String>,
    pub patch_name: Option<String>,
    pub patch_description: Option<String>,
    pub patch_semver: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySourceConfig {
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AdvancedConfig {
    pub env: BTreeMap<String, String>,
    pub git_user_name: Option<String>,
    pub git_user_email: Option<String>,
    pub temp_branch_prefix: String,
    pub lock_concurrency_group: bool,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            env: BTreeMap::new(),
            git_user_name: Some("forksync[bot]".to_string()),
            git_user_email: Some("forksync[bot]@users.noreply.github.com".to_string()),
            temp_branch_prefix: "forksync/tmp".to_string(),
            lock_concurrency_group: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProductMode {
    #[default]
    ActionOnlyPolling,
    HostedEvented,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum RepoVisibility {
    #[default]
    Auto,
    Public,
    Private,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    #[default]
    Main,
    LiveOnly,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TriggerMode {
    Schedule,
    WorkflowDispatch,
    RepositoryDispatch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    Schedule,
    Manual,
    RepositoryDispatch,
    LocalDebug,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncStrategy {
    #[default]
    ReplayPatchStack,
    MergeUpstream,
    RebasePatches,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum PatchDerivationMode {
    #[default]
    SinceRecordedPatchBase,
    SinceMergeBase,
    FullPatchBranchHistory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    #[default]
    #[serde(alias = "agent_then_pr", alias = "pr_only", alias = "fail_fast")]
    AgentRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum ValidationMode {
    #[default]
    None,
    BuildOnly,
    BuildAndTests,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    #[default]
    OpenCode,
    OpenAiCompatible,
    AnthropicCompatible,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentCredentialMode {
    #[default]
    None,
    BringYourOwnKey,
    HostedByForkSync,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptProfile {
    Conservative,
    Standard,
    #[default]
    Reckless,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamAuthMode {
    #[default]
    Anonymous,
    Pat,
    GitHubApp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunnerPreset {
    #[default]
    UbuntuLatest,
    WindowsLatest,
    MacosLatest,
    SelfHosted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    None,
    Read,
    #[default]
    Write,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_v1_plan() {
        let config = RepoConfig::default();

        assert_eq!(config.version, 1);
        assert_eq!(config.branches.patch, "forksync/patches");
        assert_eq!(config.branches.live, "forksync/live");
        assert_eq!(config.branches.output, "main");
        assert_eq!(config.sync.poll_cron, "*/15 * * * *");
        assert_eq!(
            config.sync.patch_derivation,
            PatchDerivationMode::SinceRecordedPatchBase
        );
        assert_eq!(config.validation.mode, ValidationMode::None);
        assert_eq!(config.agent.provider, AgentProvider::OpenCode);
        assert_eq!(config.agent.model.as_deref(), Some("opencode/gpt-5-nano"));
        assert_eq!(config.agent.credential_mode, AgentCredentialMode::None);
        assert_eq!(config.agent.prompt_profile, PromptProfile::Reckless);
        assert_eq!(config.auth.upstream_auth.mode, UpstreamAuthMode::Anonymous);
        assert_eq!(config.workflow.action_ref, "samfinton/forksync@main");
        assert!(config.safety.allow_force_push_output_branch);
    }

    #[test]
    fn yaml_round_trip_preserves_defaults() {
        let config = RepoConfig::default();
        let yaml = to_yaml_string(&config).expect("serialize default config");
        let decoded = from_yaml_str(&yaml).expect("deserialize default config");

        assert_eq!(decoded, config);
    }

    #[test]
    fn missing_sections_are_filled_from_defaults() {
        let parsed = from_yaml_str("version: 1\n").expect("parse minimal config");

        assert_eq!(parsed.upstream.remote_name, "upstream");
        assert_eq!(parsed.validation.mode, ValidationMode::None);
        assert_eq!(parsed.agent.provider, AgentProvider::OpenCode);
    }

    #[test]
    fn legacy_conflict_strategy_values_deserialize_to_agent_required() {
        let parsed = from_yaml_str("version: 1\nsync:\n  conflict_strategy: agent_then_pr\n")
            .expect("parse legacy conflict strategy");

        assert_eq!(
            parsed.sync.conflict_strategy,
            ConflictStrategy::AgentRequired
        );
    }
}
