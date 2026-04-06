use forksync_config::{AgentConfig, AgentProvider};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use thiserror::Error;
use tracing::{debug, instrument, warn};

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
    pub conflict_commit_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRepairOutcome {
    Repaired,
    Failed,
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
    #[error("failed to run `{command}`: {source}")]
    Io {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("agent command `{command}` failed with status {status}: {stderr}")]
    CommandFailed {
        command: String,
        status: i32,
        stderr: String,
    },
    #[error("agent did not return a usable text response")]
    MissingTextResponse,
    #[error("agent response was not a valid tool call: {0}")]
    InvalidToolCall(String),
    #[error("tool execution failed: {0}")]
    ToolExecution(String),
    #[error("agent exhausted {attempts} step(s) without finishing")]
    ExhaustedSteps { attempts: u32 },
    #[error("failed to create temporary directory for provider execution: {source}")]
    CreateTempDir {
        #[source]
        source: std::io::Error,
    },
}

pub trait CodingAgent: Send + Sync {
    fn provider(&self) -> AgentProvider;
    fn repair(&self, request: &AgentRepairRequest) -> Result<AgentRepairResult, AgentError>;
}

pub trait AgentFactory: Send + Sync {
    fn build(&self, config: &AgentConfig) -> Result<Box<dyn CodingAgent>, AgentError>;
}

trait ModelBackend: Send + Sync {
    fn provider(&self) -> AgentProvider;
    fn complete(&self, config: &AgentConfig, prompt: &str) -> Result<String, AgentError>;
}

#[derive(Debug, Default)]
pub struct OpenCodeFactory;

impl AgentFactory for OpenCodeFactory {
    fn build(&self, config: &AgentConfig) -> Result<Box<dyn CodingAgent>, AgentError> {
        match config.provider {
            AgentProvider::OpenCode => Ok(Box::new(ToolLoopAgent::new(
                config.clone(),
                OpenCodeBackend,
            ))),
            other => Err(AgentError::ProviderNotImplemented(other)),
        }
    }
}

#[derive(Debug, Clone)]
struct ToolLoopAgent<B> {
    config: AgentConfig,
    backend: B,
}

impl<B> ToolLoopAgent<B> {
    fn new(config: AgentConfig, backend: B) -> Self {
        Self { config, backend }
    }
}

impl<B> CodingAgent for ToolLoopAgent<B>
where
    B: ModelBackend,
{
    fn provider(&self) -> AgentProvider {
        self.backend.provider()
    }

    #[instrument(skip_all, fields(repo_path = %request.repo_path.display(), trigger = ?request.trigger, provider = ?self.backend.provider()))]
    fn repair(&self, request: &AgentRepairRequest) -> Result<AgentRepairResult, AgentError> {
        let max_steps = self.config.max_attempts.max(1);
        let mut tool_results = Vec::new();
        let mut files_changed = Vec::new();

        for _step in 0..max_steps {
            let snapshot = ConflictSnapshot::gather(&request.repo_path)?;
            let all_conflict_markers_cleared = !snapshot.conflicted_files.is_empty()
                && snapshot
                    .conflicted_files
                    .iter()
                    .all(|file| !contains_conflict_markers(&file.contents));
            if !files_changed.is_empty()
                && (snapshot.conflicted_files.is_empty() || all_conflict_markers_cleared)
            {
                match execute_tool_call(
                    &request.repo_path,
                    AgentToolCall::Finish {
                        summary: "Auto-finished after the agent cleared all conflict markers."
                            .to_string(),
                    },
                ) {
                    Ok(ToolExecutionResult::Finished {
                        summary,
                        commit_sha,
                    }) => {
                        return Ok(AgentRepairResult {
                            outcome: AgentRepairOutcome::Repaired,
                            summary,
                            commit_sha: Some(commit_sha),
                            files_changed,
                        });
                    }
                    Ok(ToolExecutionResult::Edited { .. }) => {}
                    Err(error) => {
                        tool_results.push(format!("Tool error: {error}"));
                    }
                }
            }

            let prompt = build_agent_prompt(request, &snapshot, &tool_results);
            let raw_response = self.backend.complete(&self.config, &prompt)?;
            let tool_call = match parse_tool_call(&raw_response) {
                Ok(tool_call) => tool_call,
                Err(error) => {
                    warn!(error = %error, "agent returned an invalid tool call");
                    tool_results.push(format!("Tool error: {error}"));
                    continue;
                }
            };

            match execute_tool_call(&request.repo_path, tool_call) {
                Ok(ToolExecutionResult::Edited { path, note }) => {
                    if !files_changed.contains(&path) {
                        files_changed.push(path);
                    }
                    tool_results.push(note);
                }
                Ok(ToolExecutionResult::Finished {
                    summary,
                    commit_sha,
                }) => {
                    return Ok(AgentRepairResult {
                        outcome: AgentRepairOutcome::Repaired,
                        summary,
                        commit_sha: Some(commit_sha),
                        files_changed,
                    });
                }
                Err(error) => {
                    tool_results.push(format!("Tool error: {error}"));
                }
            }
        }

        Ok(AgentRepairResult {
            outcome: AgentRepairOutcome::Failed,
            summary: AgentError::ExhaustedSteps {
                attempts: max_steps,
            }
            .to_string(),
            commit_sha: None,
            files_changed,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct OpenCodeBackend;

impl ModelBackend for OpenCodeBackend {
    fn provider(&self) -> AgentProvider {
        AgentProvider::OpenCode
    }

    #[instrument(skip_all, fields(provider = "opencode"))]
    fn complete(&self, config: &AgentConfig, prompt: &str) -> Result<String, AgentError> {
        let scratch = TempDir::new().map_err(|source| AgentError::CreateTempDir { source })?;
        let opencode_binary = resolve_opencode_binary();
        let mut command = Command::new(&opencode_binary);
        command
            .arg("run")
            .arg("--pure")
            .arg("--format")
            .arg("json")
            .arg("--dir")
            .arg(scratch.path());

        if let Some(model) = &config.model {
            command.arg("-m").arg(model);
        }

        let rendered_command = format!(
            "{} run --pure --format json{} --dir {} <prompt>",
            opencode_binary.display(),
            config
                .model
                .as_ref()
                .map(|model| format!(" -m {model}"))
                .unwrap_or_default(),
            scratch.path().display()
        );

        let output = command
            .arg(prompt)
            .output()
            .map_err(|source| AgentError::Io {
                command: rendered_command.clone(),
                source,
            })?;
        debug!(
            command = %rendered_command,
            status = output.status.code().unwrap_or(-1),
            "completed OpenCode invocation"
        );

        if !output.status.success() {
            return Err(AgentError::CommandFailed {
                command: rendered_command,
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let text_parts = stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| event.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|event| {
                event
                    .get("part")
                    .and_then(|part| part.get("text"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();

        if text_parts.is_empty() {
            warn!("OpenCode response did not contain any text parts");
            return Err(AgentError::MissingTextResponse);
        }

        Ok(text_parts.join("\n"))
    }
}

fn resolve_opencode_binary() -> PathBuf {
    let default_binary = PathBuf::from("opencode");
    let Some(home) = std::env::var_os("HOME") else {
        return default_binary;
    };
    let installed = PathBuf::from(home).join(".opencode/bin/opencode");
    if installed.exists() {
        installed
    } else {
        default_binary
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConflictSnapshot {
    git_status: String,
    conflicted_files: Vec<ConflictFile>,
}

impl ConflictSnapshot {
    fn gather(repo_path: &Path) -> Result<Self, AgentError> {
        let git_status = run_git(repo_path, ["status", "--short"])?;
        let conflicted = run_git(repo_path, ["diff", "--name-only", "--diff-filter=U"])?;

        let conflicted_files = conflicted
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let path = PathBuf::from(line.trim());
                let absolute = repo_path.join(&path);
                let contents = fs::read_to_string(&absolute)
                    .unwrap_or_else(|_| "<binary-or-unreadable-file>".to_string());
                ConflictFile { path, contents }
            })
            .collect::<Vec<_>>();

        Ok(Self {
            git_status,
            conflicted_files,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConflictFile {
    path: PathBuf,
    contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "tool", rename_all = "snake_case")]
enum AgentToolCall {
    EditFile {
        path: String,
        old_text: String,
        new_text: String,
    },
    Finish {
        summary: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolExecutionResult {
    Edited { path: PathBuf, note: String },
    Finished { summary: String, commit_sha: String },
}

fn build_agent_prompt(
    request: &AgentRepairRequest,
    snapshot: &ConflictSnapshot,
    tool_results: &[String],
) -> String {
    let mut prompt = String::new();
    prompt.push_str(&request.system_prompt);
    prompt.push_str("\n\nYou are operating inside ForkSync's typed repair loop.\n");
    prompt.push_str("You do not have direct repository, shell, or git tool access.\n");
    prompt.push_str("Return exactly one JSON object with one of these shapes and nothing else:\n");
    prompt.push_str(
        r#"{"tool":"edit_file","path":"relative/path","old_text":"exact existing text","new_text":"replacement text"}"#,
    );
    prompt.push('\n');
    prompt.push_str(r#"{"tool":"finish","summary":"short summary"}"#);
    prompt.push_str("\n\nRules:\n");
    prompt.push_str(
        "- Use edit_file to resolve one conflict or make one needed code adjustment at a time.\n",
    );
    prompt.push_str(
        "- For conflicted files, prefer replacing the entire current file: set old_text to the full current file contents and new_text to the full resolved file contents.\n",
    );
    prompt.push_str(
        "- Preserve both the upstream side and the local side of the conflict unless one side is clearly obsolete or incorrect.\n",
    );
    prompt.push_str(
        "- Never add explanatory prose, annotations, or merge commentary to project files unless the repository already requires that content.\n",
    );
    prompt.push_str("- old_text must match the current file content exactly once.\n");
    prompt.push_str("- When the current cherry-pick conflict is fully resolved and ready for git cherry-pick --continue, call finish.\n");
    prompt.push_str(
        "- In conflict markers, the HEAD block is the upstream/generated side and the lower block is the local authored commit being replayed.\n",
    );
    prompt.push_str("- Do not output markdown, prose, or code fences.\n");
    prompt.push_str("- Only use paths relative to the repo root.\n");
    prompt.push_str("- Prefer the smallest correct edit.\n");
    prompt.push_str("\nRepair context:\n");
    prompt.push_str(&format!(
        "- Trigger: {:?}\n- Candidate branch: {}\n- Live branch: {}\n- Internal patch branch: {}\n",
        request.trigger, request.candidate_branch, request.live_branch, request.patch_branch
    ));
    if let Some(commit) = &request.conflict_commit_sha {
        prompt.push_str(&format!("- Conflicting commit: {commit}\n"));
    }
    if let Some(validation_summary) = &request.validation_summary {
        prompt.push_str(&format!("- Validation summary: {validation_summary}\n"));
    }
    prompt.push_str("\nCurrent git status:\n");
    prompt.push_str("```text\n");
    prompt.push_str(snapshot.git_status.trim());
    prompt.push_str("\n```\n");

    if snapshot.conflicted_files.is_empty() {
        prompt.push_str("\nNo unmerged files are currently reported. If the cherry-pick is ready, call finish.\n");
    } else {
        prompt.push_str("\nConflicted files:\n");
        for file in &snapshot.conflicted_files {
            prompt.push_str(&format!("\nFile: {}\n", file.path.display()));
            prompt.push_str("```text\n");
            prompt.push_str(&truncate_for_prompt(&file.contents, 12_000));
            prompt.push_str("\n```\n");
        }
    }

    if !tool_results.is_empty() {
        prompt.push_str("\nPrevious tool results:\n");
        for result in tool_results {
            prompt.push_str("- ");
            prompt.push_str(result);
            prompt.push('\n');
        }
    }

    prompt
}

fn truncate_for_prompt(contents: &str, limit: usize) -> String {
    if contents.len() <= limit {
        return contents.to_string();
    }

    format!(
        "{}\n...[truncated by ForkSync agent loop]...",
        &contents[..limit]
    )
}

fn parse_tool_call(response: &str) -> Result<AgentToolCall, AgentError> {
    let trimmed = response.trim();
    let trimmed = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix("```").unwrap_or(trimmed).trim();

    serde_json::from_str(trimmed)
        .map_err(|error| AgentError::InvalidToolCall(format!("{error}: {trimmed}")))
}

fn execute_tool_call(
    repo_path: &Path,
    tool_call: AgentToolCall,
) -> Result<ToolExecutionResult, AgentError> {
    match tool_call {
        AgentToolCall::EditFile {
            path,
            old_text,
            new_text,
        } => {
            let relative_path = sanitize_relative_path(&path)?;
            let absolute_path = repo_path.join(&relative_path);
            let current = if absolute_path.exists() {
                fs::read_to_string(&absolute_path).map_err(|error| {
                    AgentError::ToolExecution(format!(
                        "failed to read {}: {error}",
                        absolute_path.display()
                    ))
                })?
            } else {
                String::new()
            };

            let updated = if current.is_empty() && old_text.is_empty() {
                new_text
            } else {
                let matches = current.match_indices(&old_text).count();
                if matches != 1 {
                    return Err(AgentError::ToolExecution(format!(
                        "edit_file expected exactly one match for {} but found {}",
                        relative_path.display(),
                        matches
                    )));
                }
                current.replacen(&old_text, &new_text, 1)
            };

            if let Some(parent) = absolute_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    AgentError::ToolExecution(format!(
                        "failed to create parent directory for {}: {error}",
                        absolute_path.display()
                    ))
                })?;
            }
            fs::write(&absolute_path, updated).map_err(|error| {
                AgentError::ToolExecution(format!(
                    "failed to write {}: {error}",
                    absolute_path.display()
                ))
            })?;

            Ok(ToolExecutionResult::Edited {
                path: relative_path.clone(),
                note: format!("Edited {} successfully.", relative_path.display()),
            })
        }
        AgentToolCall::Finish { summary } => {
            run_git(repo_path, ["add", "-A"])?;

            let rendered_command = format!("git -C {} cherry-pick --continue", repo_path.display());
            let output = Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .arg("cherry-pick")
                .arg("--continue")
                .env("GIT_EDITOR", "true")
                .output()
                .map_err(|source| AgentError::Io {
                    command: rendered_command.clone(),
                    source,
                })?;

            if !output.status.success() {
                return Err(AgentError::ToolExecution(format!(
                    "finish could not continue the cherry-pick yet: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }

            let commit_sha = run_git(repo_path, ["rev-parse", "HEAD"])?;
            Ok(ToolExecutionResult::Finished {
                summary,
                commit_sha,
            })
        }
    }
}

fn sanitize_relative_path(path: &str) -> Result<PathBuf, AgentError> {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(AgentError::ToolExecution(format!(
            "path must stay inside the repo: {path}"
        )));
    }
    Ok(candidate)
}

fn contains_conflict_markers(contents: &str) -> bool {
    contents.contains("<<<<<<<") || contents.contains("=======") || contents.contains(">>>>>>>")
}

fn run_git<I, S>(repo_path: &Path, args: I) -> Result<String, AgentError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let rendered_args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let rendered_command = format!("git -C {} {}", repo_path.display(), rendered_args.join(" "));

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(&rendered_args)
        .output()
        .map_err(|source| AgentError::Io {
            command: rendered_command.clone(),
            source,
        })?;

    if !output.status.success() {
        return Err(AgentError::CommandFailed {
            command: rendered_command,
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct SequenceBackend {
        responses: Mutex<VecDeque<String>>,
    }

    impl SequenceBackend {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    impl ModelBackend for SequenceBackend {
        fn provider(&self) -> AgentProvider {
            AgentProvider::OpenCode
        }

        fn complete(&self, _config: &AgentConfig, _prompt: &str) -> Result<String, AgentError> {
            self.responses
                .lock()
                .expect("lock responses")
                .pop_front()
                .ok_or_else(|| AgentError::MissingTextResponse)
        }
    }

    #[test]
    fn tool_loop_repairs_conflict_and_continues_cherry_pick() {
        let temp = TempDir::new().expect("create tempdir");
        let repo = temp.path();

        git(repo, ["init", "-b", "main"]);
        git(repo, ["config", "user.name", "ForkSync Test"]);
        git(repo, ["config", "user.email", "forksync-test@example.com"]);
        fs::write(repo.join("README.md"), "seed repo\n").expect("write seed readme");
        git(repo, ["add", "README.md"]);
        git(repo, ["commit", "-m", "Initial commit"]);

        git(repo, ["switch", "-c", "feature"]);
        fs::write(repo.join("README.md"), "seed repo\nlocal change\n").expect("write local change");
        git(repo, ["add", "README.md"]);
        git(repo, ["commit", "-m", "Local readme change"]);
        let local_commit = git_output(repo, ["rev-parse", "HEAD"]);

        git(repo, ["switch", "main"]);
        fs::write(repo.join("README.md"), "seed repo\nupstream change\n")
            .expect("write upstream change");
        git(repo, ["add", "README.md"]);
        git(repo, ["commit", "-m", "Upstream readme change"]);

        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["cherry-pick", &local_commit])
            .output()
            .expect("run cherry-pick");
        assert!(
            !output.status.success(),
            "expected cherry-pick conflict\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let conflicted = fs::read_to_string(repo.join("README.md")).expect("read conflicted file");
        let backend = SequenceBackend::new(vec![
            serde_json::to_string(&AgentToolCall::EditFile {
                path: "README.md".to_string(),
                old_text: conflicted,
                new_text: "seed repo\nupstream change\nlocal change\n".to_string(),
            })
            .expect("serialize edit tool"),
            serde_json::to_string(&AgentToolCall::Finish {
                summary: "Resolved README conflict".to_string(),
            })
            .expect("serialize finish tool"),
        ]);

        let agent = ToolLoopAgent::new(AgentConfig::default(), backend);
        let result = agent
            .repair(&AgentRepairRequest {
                repo_path: repo.to_path_buf(),
                candidate_branch: "forksync/tmp/sync".to_string(),
                patch_branch: "forksync/patches".to_string(),
                live_branch: "forksync/live".to_string(),
                trigger: RepairTrigger::ReplayConflict,
                system_prompt: "Repair the current cherry-pick conflict.".to_string(),
                validation_summary: None,
                conflict_commit_sha: Some(local_commit.clone()),
            })
            .expect("repair conflict");

        assert_eq!(result.outcome, AgentRepairOutcome::Repaired);
        assert_eq!(
            fs::read_to_string(repo.join("README.md")).expect("read resolved readme"),
            "seed repo\nupstream change\nlocal change\n"
        );
        assert!(!result.files_changed.is_empty());
        assert_eq!(
            git_output(repo, ["log", "-1", "--pretty=%s"]),
            "Local readme change"
        );
    }

    #[test]
    fn tool_loop_auto_finishes_after_conflict_markers_are_cleared() {
        let temp = TempDir::new().expect("create tempdir");
        let repo = temp.path();

        git(repo, ["init", "-b", "main"]);
        git(repo, ["config", "user.name", "ForkSync Test"]);
        git(repo, ["config", "user.email", "forksync-test@example.com"]);
        fs::write(repo.join("README.md"), "seed repo\n").expect("write seed readme");
        git(repo, ["add", "README.md"]);
        git(repo, ["commit", "-m", "Initial commit"]);

        git(repo, ["switch", "-c", "feature"]);
        fs::write(repo.join("README.md"), "seed repo\nlocal change\n").expect("write local change");
        git(repo, ["add", "README.md"]);
        git(repo, ["commit", "-m", "Local readme change"]);
        let local_commit = git_output(repo, ["rev-parse", "HEAD"]);

        git(repo, ["switch", "main"]);
        fs::write(repo.join("README.md"), "seed repo\nupstream change\n")
            .expect("write upstream change");
        git(repo, ["add", "README.md"]);
        git(repo, ["commit", "-m", "Upstream readme change"]);

        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["cherry-pick", &local_commit])
            .output()
            .expect("run cherry-pick");
        assert!(
            !output.status.success(),
            "expected cherry-pick conflict\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let conflicted = fs::read_to_string(repo.join("README.md")).expect("read conflicted file");
        let backend = SequenceBackend::new(vec![
            serde_json::to_string(&AgentToolCall::EditFile {
                path: "README.md".to_string(),
                old_text: conflicted,
                new_text: "seed repo\nupstream change\nlocal change\n".to_string(),
            })
            .expect("serialize edit tool"),
        ]);

        let agent = ToolLoopAgent::new(AgentConfig::default(), backend);
        let result = agent
            .repair(&AgentRepairRequest {
                repo_path: repo.to_path_buf(),
                candidate_branch: "forksync/tmp/sync".to_string(),
                patch_branch: "forksync/patches".to_string(),
                live_branch: "forksync/live".to_string(),
                trigger: RepairTrigger::ReplayConflict,
                system_prompt: "Repair the current cherry-pick conflict.".to_string(),
                validation_summary: None,
                conflict_commit_sha: Some(local_commit.clone()),
            })
            .expect("repair conflict");

        assert_eq!(result.outcome, AgentRepairOutcome::Repaired);
        assert_eq!(
            fs::read_to_string(repo.join("README.md")).expect("read resolved readme"),
            "seed repo\nupstream change\nlocal change\n"
        );
        assert_eq!(
            git_output(repo, ["log", "-1", "--pretty=%s"]),
            "Local readme change"
        );
    }

    fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git command failed in {}\nargs: {:?}\nstdout:\n{}\nstderr:\n{}",
            cwd.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output<const N: usize>(cwd: &Path, args: [&str; N]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git command failed in {}\nargs: {:?}\nstdout:\n{}\nstderr:\n{}",
            cwd.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
