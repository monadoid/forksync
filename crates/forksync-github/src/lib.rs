use forksync_config::{AgentProvider, PermissionLevel, RepoConfig, RunnerPreset, TriggerMode};
use serde::Deserialize;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
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
    pub base_branch: String,
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
        base_branch: config.branches.output.clone(),
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
    #[error("gh CLI is not available")]
    MissingGh,
    #[error("gh command `{command}` failed with status {status}: {stderr}")]
    CommandFailed {
        command: String,
        status: i32,
        stderr: String,
    },
    #[error("failed to run `{command}`: {source}")]
    Io {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse gh JSON response: {0}")]
    Parse(String),
}

pub trait FailureReporter: Send + Sync {
    fn upsert_failure_pr(&self, payload: &FailurePrPayload)
    -> Result<FailurePrHandle, GithubError>;
}

impl<T> FailureReporter for Box<T>
where
    T: FailureReporter + ?Sized,
{
    fn upsert_failure_pr(
        &self,
        payload: &FailurePrPayload,
    ) -> Result<FailurePrHandle, GithubError> {
        (**self).upsert_failure_pr(payload)
    }
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

#[derive(Debug, Clone)]
pub struct GhCliFailureReporter {
    repo_path: PathBuf,
}

impl GhCliFailureReporter {
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
        }
    }
}

impl FailureReporter for GhCliFailureReporter {
    fn upsert_failure_pr(
        &self,
        payload: &FailurePrPayload,
    ) -> Result<FailurePrHandle, GithubError> {
        ensure_gh_exists()?;
        let body = render_pr_body(payload);
        if let Some(existing) = find_existing_pr(&self.repo_path, payload)? {
            let mut args = vec![
                "pr".to_string(),
                "edit".to_string(),
                existing.number.to_string(),
                "--title".to_string(),
                payload.summary.title.clone(),
                "--body".to_string(),
                body,
            ];
            for label in &payload.labels {
                args.push("--add-label".to_string());
                args.push(label.clone());
            }
            run_gh(&self.repo_path, &args)?;
            return Ok(existing);
        }

        let mut args = vec![
            "pr".to_string(),
            "create".to_string(),
            "--base".to_string(),
            payload.base_branch.clone(),
            "--head".to_string(),
            payload.branch.clone(),
            "--title".to_string(),
            payload.summary.title.clone(),
            "--body".to_string(),
            body,
        ];
        for label in &payload.labels {
            args.push("--label".to_string());
            args.push(label.clone());
        }
        for assignee in &payload.assign_users {
            args.push("--assignee".to_string());
            args.push(assignee.clone());
        }
        for reviewer in &payload.request_review_users {
            args.push("--reviewer".to_string());
            args.push(reviewer.clone());
        }

        let url = run_gh_capture(&self.repo_path, &args)?;
        Ok(FailurePrHandle {
            number: 0,
            url: (!url.trim().is_empty()).then(|| url.trim().to_string()),
        })
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
    contents.push_str("      - name: Cache ForkSync action runtime\n");
    contents.push_str("        uses: actions/cache@v4\n");
    contents.push_str("        with:\n");
    contents.push_str("          path: ~/.cache/forksync\n");
    let _ = writeln!(
        contents,
        "          key: ${{{{ runner.os }}}}-forksync-runtime-{}",
        cache_key_fragment(&config.workflow.action_ref)
    );
    contents.push_str("          restore-keys: |\n");
    contents.push_str("            ${{ runner.os }}-forksync-runtime-\n");
    contents.push_str("      - name: Run ForkSync action\n");
    let _ = writeln!(contents, "        uses: {}", config.workflow.action_ref);
    contents.push_str("        with:\n");
    contents.push_str("          trigger: schedule\n");
    let _ = writeln!(
        contents,
        "          install-opencode: {}",
        if config.agent.enabled && config.agent.provider == AgentProvider::OpenCode {
            "true"
        } else {
            "false"
        }
    );

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

fn cache_key_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

#[derive(Debug, Deserialize)]
struct GhPullRequestSummary {
    number: u64,
    url: Option<String>,
}

fn ensure_gh_exists() -> Result<(), GithubError> {
    let output = Command::new("gh").arg("--version").output();
    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => Err(GithubError::MissingGh),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Err(GithubError::MissingGh),
        Err(source) => Err(GithubError::Io {
            command: "gh --version".to_string(),
            source,
        }),
    }
}

fn find_existing_pr(
    repo_path: &Path,
    payload: &FailurePrPayload,
) -> Result<Option<FailurePrHandle>, GithubError> {
    let output = run_gh_capture(
        repo_path,
        &[
            "pr".to_string(),
            "list".to_string(),
            "--head".to_string(),
            payload.branch.clone(),
            "--base".to_string(),
            payload.base_branch.clone(),
            "--state".to_string(),
            "open".to_string(),
            "--json".to_string(),
            "number,url".to_string(),
        ],
    )?;
    let prs = serde_json::from_str::<Vec<GhPullRequestSummary>>(&output)
        .map_err(|error| GithubError::Parse(error.to_string()))?;
    Ok(prs.into_iter().next().map(|pr| FailurePrHandle {
        number: pr.number,
        url: pr.url,
    }))
}

fn render_pr_body(payload: &FailurePrPayload) -> String {
    let mut body = payload.summary.body.clone();
    if !payload.mention_users.is_empty() {
        let mentions = payload
            .mention_users
            .iter()
            .map(|user| format!("@{user}"))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = write!(body, "\n\n{}", mentions);
    }
    body
}

fn run_gh(repo_path: &Path, args: &[String]) -> Result<(), GithubError> {
    let command = format!("gh {}", args.join(" "));
    let output = Command::new("gh")
        .current_dir(repo_path)
        .args(args)
        .output()
        .map_err(|source| GithubError::Io {
            command: command.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(GithubError::CommandFailed {
            command,
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

fn run_gh_capture(repo_path: &Path, args: &[String]) -> Result<String, GithubError> {
    let command = format!("gh {}", args.join(" "));
    let output = Command::new("gh")
        .current_dir(repo_path)
        .args(args)
        .output()
        .map_err(|source| GithubError::Io {
            command: command.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(GithubError::CommandFailed {
            command,
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
        assert!(workflow.contents.contains("uses: actions/cache@v4"));
        assert!(workflow.contents.contains("path: ~/.cache/forksync"));
        assert!(workflow.contents.contains("uses: monadoid/forksync@v1"));
        assert!(workflow.contents.contains("trigger: schedule"));
        assert!(workflow.contents.contains("install-opencode: true"));
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
                pr_branch: "forksync/conflicts".to_string(),
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
        assert_eq!(first_payload.branch, "forksync/conflicts");
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

        assert!(workflow.contents.contains("install-opencode: false"));
    }

    #[test]
    fn cache_key_fragment_sanitizes_action_ref() {
        assert_eq!(
            cache_key_fragment("monadoid/forksync@v1.2.3"),
            "monadoid-forksync-v1-2-3"
        );
    }
}
