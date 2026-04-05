use forksync_config::{PermissionLevel, RepoConfig, RunnerPreset, TriggerMode};
use std::fmt::Write as _;
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

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopFailureReporter;

impl FailureReporter for NoopFailureReporter {
    fn upsert_failure_pr(
        &self,
        _payload: &FailurePrPayload,
    ) -> Result<FailurePrHandle, GithubError> {
        Err(GithubError::NotImplemented)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedWorkflow {
    pub path: String,
    pub contents: String,
}

pub fn generate_sync_workflow(config: &RepoConfig) -> GeneratedWorkflow {
    let mut contents = String::new();

    contents.push_str("name: ForkSync\n\n");
    contents.push_str("on:\n");

    if config.sync.trigger_modes.contains(&TriggerMode::Schedule) {
        contents.push_str("  schedule:\n");
        let _ = writeln!(contents, "    - cron: '{}'", config.sync.poll_cron);
    }

    if config
        .sync
        .trigger_modes
        .contains(&TriggerMode::WorkflowDispatch)
    {
        contents.push_str("  workflow_dispatch:\n");
    }

    if config
        .sync
        .trigger_modes
        .contains(&TriggerMode::RepositoryDispatch)
    {
        contents.push_str("  repository_dispatch:\n");
    }

    contents.push_str("\npermissions:\n");
    let _ = writeln!(
        contents,
        "  contents: {}",
        render_permission(config.workflow.permissions.contents)
    );
    let _ = writeln!(
        contents,
        "  pull-requests: {}",
        render_permission(config.workflow.permissions.pull_requests)
    );
    let _ = writeln!(
        contents,
        "  issues: {}",
        render_permission(config.workflow.permissions.issues)
    );
    let _ = writeln!(
        contents,
        "  actions: {}",
        render_permission(config.workflow.permissions.actions)
    );

    contents.push_str("\nconcurrency:\n");
    contents.push_str("  group: forksync-${{ github.repository }}\n");
    contents.push_str("  cancel-in-progress: false\n");

    contents.push_str("\njobs:\n");
    contents.push_str("  sync:\n");
    let _ = writeln!(
        contents,
        "    runs-on: {}",
        render_runner(config.workflow.runner)
    );
    let _ = writeln!(
        contents,
        "    timeout-minutes: {}",
        config.workflow.timeout_minutes
    );
    contents.push_str("    steps:\n");
    contents.push_str("      - name: Checkout fork\n");
    contents.push_str("        uses: actions/checkout@v4\n");
    contents.push_str("        with:\n");
    contents.push_str("          fetch-depth: 0\n");
    contents.push_str("      - name: Run ForkSync\n");
    contents.push_str("        run: |\n");
    contents.push_str(
        "          echo \"Replace this placeholder with the published ForkSync action or installer once available.\"\n",
    );
    contents.push_str(
        "          echo \"Local dogfood uses the CLI directly: forksync sync --trigger schedule\"\n",
    );

    GeneratedWorkflow {
        path: ".github/workflows/forksync.yml".to_string(),
        contents,
    }
}

fn render_permission(permission: PermissionLevel) -> &'static str {
    match permission {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
    }
}

fn render_runner(runner: RunnerPreset) -> &'static str {
    match runner {
        RunnerPreset::UbuntuLatest => "ubuntu-latest",
        RunnerPreset::WindowsLatest => "windows-latest",
        RunnerPreset::MacosLatest => "macos-latest",
        RunnerPreset::SelfHosted => "self-hosted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forksync_config::RepoConfig;

    #[test]
    fn generated_workflow_includes_default_schedule_and_manual_trigger() {
        let config = RepoConfig::default();

        let workflow = generate_sync_workflow(&config);

        assert_eq!(workflow.path, ".github/workflows/forksync.yml");
        assert!(workflow.contents.contains("name: ForkSync"));
        assert!(workflow.contents.contains("workflow_dispatch"));
        assert!(workflow.contents.contains("cron: '*/15 * * * *'"));
        assert!(workflow.contents.contains("contents: write"));
        assert!(workflow.contents.contains("pull-requests: write"));
    }
}
