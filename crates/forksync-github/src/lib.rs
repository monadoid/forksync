use forksync_config::{AgentProvider, PermissionLevel, RepoConfig, RunnerPreset, TriggerMode};
use std::fmt::Write as _;
use thiserror::Error;
use tracing::{debug, instrument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureDetails {
    pub outcome: String,
    pub upstream_sha: Option<String>,
    pub notes: Vec<String>,
    pub is_first_failure: bool,
}

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

pub fn build_failure_summary(config: &RepoConfig, details: &FailureDetails) -> FailureSummary {
    FailureSummary {
        title: format!(
            "{} ForkSync sync failure",
            config.notifications.on_failure.pr_title_prefix
        ),
        body: render_failure_body(details),
        outcome: details.outcome.clone(),
        upstream_sha: details.upstream_sha.clone(),
    }
}

pub fn build_failure_pr_payload(
    config: &RepoConfig,
    details: &FailureDetails,
) -> Option<FailurePrPayload> {
    if !config.notifications.on_failure.open_pr {
        return None;
    }

    let summary = build_failure_summary(config, details);
    let mention_users = if config
        .notifications
        .on_failure
        .mention_on_first_failure_only
        && !details.is_first_failure
    {
        Vec::new()
    } else {
        config.notifications.on_failure.mention_users.clone()
    };

    Some(FailurePrPayload {
        branch: config.notifications.on_failure.pr_branch.clone(),
        title_prefix: config.notifications.on_failure.pr_title_prefix.clone(),
        labels: config.notifications.on_failure.pr_labels.clone(),
        assign_users: config.notifications.on_failure.assign_users.clone(),
        request_review_users: config.notifications.on_failure.request_review_users.clone(),
        mention_users,
        summary,
    })
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

#[instrument(skip_all, fields(output_branch = %config.branches.output))]
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
    contents.push_str("      - name: Set up Rust toolchain\n");
    contents.push_str("        uses: dtolnay/rust-toolchain@stable\n");
    contents.push_str("      - name: Bootstrap ForkSync runtime dependencies\n");
    contents.push_str("        shell: bash\n");
    contents.push_str("        run: |\n");
    contents.push_str("          set -euo pipefail\n");
    contents.push_str("          cargo --version\n");
    contents.push_str("          git --version\n");
    if config.agent.enabled && config.agent.provider == AgentProvider::OpenCode {
        contents.push_str("          if ! command -v opencode >/dev/null 2>&1; then\n");
        contents.push_str("            curl -fsSL https://opencode.ai/install | bash\n");
        contents.push_str("          fi\n");
        contents.push_str("          opencode --version\n");
    }
    contents.push_str("      - name: Run ForkSync\n");
    contents.push_str("        run: |\n");
    contents.push_str("          cargo run --quiet --bin forksync -- sync --trigger schedule\n");

    let workflow = GeneratedWorkflow {
        path: ".github/workflows/forksync.yml".to_string(),
        contents,
    };
    debug!("generated ForkSync workflow contents");
    workflow
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

fn render_failure_body(details: &FailureDetails) -> String {
    let mut body = String::new();
    let _ = writeln!(body, "Outcome: {}", details.outcome);
    if let Some(upstream_sha) = &details.upstream_sha {
        let _ = writeln!(body, "Upstream SHA: {}", upstream_sha);
    }
    if !details.notes.is_empty() {
        body.push('\n');
        body.push_str("Notes:\n");
        for note in &details.notes {
            let _ = writeln!(body, "- {}", note);
        }
    }
    body.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use forksync_config::{
        AgentProvider, FailureNotificationConfig, NotificationConfig, RepoConfig,
    };

    #[test]
    fn generated_workflow_includes_default_schedule_and_manual_trigger() {
        let config = RepoConfig::default();

        let workflow = generate_sync_workflow(&config);

        assert_eq!(workflow.path, ".github/workflows/forksync.yml");
        assert!(workflow.contents.contains("name: ForkSync"));
        assert!(workflow.contents.contains("workflow_dispatch"));
        assert!(workflow.contents.contains("cron: '*/15 * * * *'"));
        assert!(workflow.contents.contains("dtolnay/rust-toolchain@stable"));
        assert!(
            workflow
                .contents
                .contains("curl -fsSL https://opencode.ai/install | bash")
        );
        assert!(workflow.contents.contains("opencode --version"));
        assert!(
            workflow
                .contents
                .contains("cargo run --quiet --bin forksync -- sync --trigger schedule")
        );
        assert!(workflow.contents.contains("contents: write"));
        assert!(workflow.contents.contains("pull-requests: write"));
    }

    #[test]
    fn build_failure_payload_honors_first_failure_mentions_and_reuse() {
        let mut config = RepoConfig::default();
        config.notifications = NotificationConfig {
            on_success: Default::default(),
            on_failure: FailureNotificationConfig {
                open_pr: true,
                reuse_existing_pr: true,
                pr_branch: "forksync/failure".to_string(),
                pr_title_prefix: "[ForkSync]".to_string(),
                pr_labels: vec!["forksync".to_string()],
                assign_users: vec!["assignee".to_string()],
                request_review_users: vec!["reviewer".to_string()],
                mention_users: vec!["alice".to_string(), "bob".to_string()],
                mention_on_first_failure_only: true,
            },
        };

        let first = FailureDetails {
            outcome: "FailedValidation".to_string(),
            upstream_sha: Some("abc123".to_string()),
            notes: vec!["build step failed".to_string()],
            is_first_failure: true,
        };
        let first_payload = build_failure_pr_payload(&config, &first).expect("payload");
        assert_eq!(first_payload.branch, "forksync/failure");
        assert_eq!(first_payload.title_prefix, "[ForkSync]");
        assert_eq!(first_payload.labels, vec!["forksync".to_string()]);
        assert_eq!(first_payload.assign_users, vec!["assignee".to_string()]);
        assert_eq!(
            first_payload.request_review_users,
            vec!["reviewer".to_string()]
        );
        assert_eq!(
            first_payload.mention_users,
            vec!["alice".to_string(), "bob".to_string()]
        );
        assert_eq!(first_payload.summary.outcome, "FailedValidation");
        assert!(
            first_payload
                .summary
                .body
                .contains("Outcome: FailedValidation")
        );
        assert!(first_payload.summary.body.contains("Upstream SHA: abc123"));
        assert!(first_payload.summary.body.contains("- build step failed"));

        let later = FailureDetails {
            is_first_failure: false,
            ..first.clone()
        };
        let later_payload = build_failure_pr_payload(&config, &later).expect("payload");
        assert!(later_payload.mention_users.is_empty());
    }

    #[test]
    fn build_failure_payload_can_be_disabled_cleanly() {
        let mut config = RepoConfig::default();
        config.notifications.on_failure.open_pr = false;

        let details = FailureDetails {
            outcome: "FailedInfra".to_string(),
            upstream_sha: None,
            notes: Vec::new(),
            is_first_failure: true,
        };

        assert!(build_failure_pr_payload(&config, &details).is_none());
    }

    #[test]
    fn generated_workflow_skips_opencode_install_when_agent_is_disabled() {
        let mut config = RepoConfig::default();
        config.agent.enabled = false;
        config.agent.provider = AgentProvider::Disabled;

        let workflow = generate_sync_workflow(&config);

        assert!(
            !workflow
                .contents
                .contains("curl -fsSL https://opencode.ai/install | bash")
        );
        assert!(!workflow.contents.contains("opencode --version"));
    }
}
