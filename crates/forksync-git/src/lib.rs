use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSpec {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchDerivationRequest {
    pub repo_path: PathBuf,
    pub patch_branch: String,
    pub base_ref: String,
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
    pub head_sha: Option<String>,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to run `{command}`: {source}")]
    Io {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("git command `{command}` failed with status {status}: {stderr}")]
    CommandFailed {
        command: String,
        status: i32,
        stderr: String,
    },
    #[error("path is not a git repository: {path}")]
    NotGitRepository { path: PathBuf },
    #[error("git worktree is dirty at {path}")]
    DirtyWorktree { path: PathBuf },
    #[error("remote `{remote}` was not found in {path}")]
    MissingRemote { remote: String, path: PathBuf },
    #[error("unable to determine default branch for remote `{remote}` in {path}")]
    MissingDefaultBranch { remote: String, path: PathBuf },
}

pub trait GitBackend: Send + Sync {
    fn ensure_repo(&self, repo_path: &Path) -> Result<(), GitError>;
    fn worktree_clean(&self, repo_path: &Path) -> Result<bool, GitError>;
    fn paths_clean(&self, repo_path: &Path, paths: &[PathBuf]) -> Result<bool, GitError>;
    fn current_ref(&self, repo_path: &Path) -> Result<String, GitError>;
    fn checkout(&self, repo_path: &Path, reference: &str) -> Result<(), GitError>;
    fn checkout_new_branch(
        &self,
        repo_path: &Path,
        branch: &str,
        target: &str,
    ) -> Result<(), GitError>;
    fn merge_ff_only(&self, repo_path: &Path, target: &str) -> Result<(), GitError>;
    fn head_sha(&self, repo_path: &Path) -> Result<String, GitError>;
    fn remote_exists(&self, repo_path: &Path, remote_name: &str) -> Result<bool, GitError>;
    fn get_remote_url(&self, repo_path: &Path, remote_name: &str) -> Result<String, GitError>;
    fn fetch_remote(&self, repo_path: &Path, remote: &RemoteSpec) -> Result<(), GitError>;
    fn default_branch_for_remote(
        &self,
        repo_path: &Path,
        remote_name: &str,
    ) -> Result<String, GitError>;
    fn resolve_remote_head(
        &self,
        repo_path: &Path,
        remote_name: &str,
        branch: &str,
    ) -> Result<String, GitError>;
    fn local_branch_exists(&self, repo_path: &Path, branch: &str) -> Result<bool, GitError>;
    fn create_or_reset_branch(
        &self,
        repo_path: &Path,
        branch: &str,
        target: &str,
    ) -> Result<(), GitError>;
    fn commit_paths(
        &self,
        repo_path: &Path,
        paths: &[PathBuf],
        message: &str,
    ) -> Result<String, GitError>;
    fn push_branch(
        &self,
        repo_path: &Path,
        remote_name: &str,
        branch: &str,
    ) -> Result<(), GitError>;
    fn push_refspec(
        &self,
        repo_path: &Path,
        remote_name: &str,
        refspec: &str,
    ) -> Result<(), GitError>;
    fn add_detached_worktree(
        &self,
        repo_path: &Path,
        worktree_path: &Path,
        target: &str,
    ) -> Result<(), GitError>;
    fn remove_worktree(&self, repo_path: &Path, worktree_path: &Path) -> Result<(), GitError>;
    fn delete_branch(&self, repo_path: &Path, branch: &str) -> Result<(), GitError>;
    fn derive_patch_commits(
        &self,
        request: &PatchDerivationRequest,
    ) -> Result<Vec<PatchCommit>, GitError>;
    fn replay_patch_stack(&self, request: &ReplayRequest) -> Result<ReplayResult, GitError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGitBackend;

impl SystemGitBackend {
    fn run_git<I, S>(&self, repo_path: &Path, args: I) -> Result<String, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args_vec: Vec<_> = args.into_iter().collect();
        let command = render_command(repo_path, &args_vec);
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(args_vec.iter().map(|arg| arg.as_ref()))
            .output()
            .map_err(|source| GitError::Io {
                command: command.clone(),
                source,
            })?;

        if !output.status.success() {
            return Err(GitError::CommandFailed {
                command,
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn run_git_allow_failure<I, S>(&self, repo_path: &Path, args: I) -> Result<(), GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args_vec: Vec<_> = args.into_iter().collect();
        let command = render_command(repo_path, &args_vec);
        let status = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(args_vec.iter().map(|arg| arg.as_ref()))
            .status()
            .map_err(|source| GitError::Io {
                command: command.clone(),
                source,
            })?;

        if !status.success() {
            return Err(GitError::CommandFailed {
                command,
                status: status.code().unwrap_or(-1),
                stderr: String::new(),
            });
        }

        Ok(())
    }
}

impl GitBackend for SystemGitBackend {
    fn ensure_repo(&self, repo_path: &Path) -> Result<(), GitError> {
        self.run_git(repo_path, ["rev-parse", "--git-dir"])
            .map(|_| ())
            .map_err(|_| GitError::NotGitRepository {
                path: repo_path.to_path_buf(),
            })
    }

    fn worktree_clean(&self, repo_path: &Path) -> Result<bool, GitError> {
        let status = self.run_git(repo_path, ["status", "--porcelain"])?;
        Ok(status.is_empty())
    }

    fn paths_clean(&self, repo_path: &Path, paths: &[PathBuf]) -> Result<bool, GitError> {
        if paths.is_empty() {
            return Ok(true);
        }

        let mut args = vec![
            "status".to_string(),
            "--porcelain".to_string(),
            "--".to_string(),
        ];
        args.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
        let output = self.run_git(repo_path, args)?;
        Ok(output.is_empty())
    }

    fn current_ref(&self, repo_path: &Path) -> Result<String, GitError> {
        let branch = self.run_git(repo_path, ["branch", "--show-current"])?;
        if !branch.is_empty() {
            return Ok(branch);
        }

        self.head_sha(repo_path)
    }

    fn checkout(&self, repo_path: &Path, reference: &str) -> Result<(), GitError> {
        self.run_git(repo_path, ["checkout", "--quiet", reference])
            .map(|_| ())
    }

    fn checkout_new_branch(
        &self,
        repo_path: &Path,
        branch: &str,
        target: &str,
    ) -> Result<(), GitError> {
        self.run_git(repo_path, ["checkout", "--quiet", "-B", branch, target])
            .map(|_| ())
    }

    fn merge_ff_only(&self, repo_path: &Path, target: &str) -> Result<(), GitError> {
        self.run_git(repo_path, ["merge", "--ff-only", target])
            .map(|_| ())
    }

    fn head_sha(&self, repo_path: &Path) -> Result<String, GitError> {
        self.run_git(repo_path, ["rev-parse", "HEAD"])
    }

    fn remote_exists(&self, repo_path: &Path, remote_name: &str) -> Result<bool, GitError> {
        let remotes = self.run_git(repo_path, ["remote"])?;
        Ok(remotes.lines().any(|line| line.trim() == remote_name))
    }

    fn get_remote_url(&self, repo_path: &Path, remote_name: &str) -> Result<String, GitError> {
        if !self.remote_exists(repo_path, remote_name)? {
            return Err(GitError::MissingRemote {
                remote: remote_name.to_string(),
                path: repo_path.to_path_buf(),
            });
        }

        self.run_git(repo_path, ["remote", "get-url", remote_name])
    }

    fn fetch_remote(&self, repo_path: &Path, remote: &RemoteSpec) -> Result<(), GitError> {
        if !self.remote_exists(repo_path, &remote.name)? {
            return Err(GitError::MissingRemote {
                remote: remote.name.clone(),
                path: repo_path.to_path_buf(),
            });
        }

        self.run_git(repo_path, ["fetch", "--prune", &remote.name])
            .map(|_| ())
    }

    fn default_branch_for_remote(
        &self,
        repo_path: &Path,
        remote_name: &str,
    ) -> Result<String, GitError> {
        let symbolic_ref = format!("refs/remotes/{remote_name}/HEAD");
        if let Ok(remote_head) = self.run_git(
            repo_path,
            ["symbolic-ref", "--quiet", "--short", &symbolic_ref],
        ) {
            if let Some(stripped) = remote_head.strip_prefix(&format!("{remote_name}/")) {
                return Ok(stripped.to_string());
            }
        }

        let remote_info = self.run_git(repo_path, ["remote", "show", remote_name])?;
        for line in remote_info.lines() {
            if let Some(branch) = line.trim().strip_prefix("HEAD branch: ") {
                return Ok(branch.trim().to_string());
            }
        }

        Err(GitError::MissingDefaultBranch {
            remote: remote_name.to_string(),
            path: repo_path.to_path_buf(),
        })
    }

    fn resolve_remote_head(
        &self,
        repo_path: &Path,
        remote_name: &str,
        branch: &str,
    ) -> Result<String, GitError> {
        let reference = format!("refs/remotes/{remote_name}/{branch}");
        self.run_git(repo_path, ["rev-parse", &reference])
    }

    fn local_branch_exists(&self, repo_path: &Path, branch: &str) -> Result<bool, GitError> {
        let reference = format!("refs/heads/{branch}");
        let command = render_command(
            repo_path,
            &[
                OsStr::new("show-ref"),
                OsStr::new("--verify"),
                OsStr::new("--quiet"),
                OsStr::new(&reference),
            ],
        );
        let status = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["show-ref", "--verify", "--quiet", &reference])
            .status()
            .map_err(|source| GitError::Io { command, source })?;

        Ok(status.success())
    }

    fn create_or_reset_branch(
        &self,
        repo_path: &Path,
        branch: &str,
        target: &str,
    ) -> Result<(), GitError> {
        self.run_git(repo_path, ["branch", "--force", branch, target])
            .map(|_| ())
    }

    fn commit_paths(
        &self,
        repo_path: &Path,
        paths: &[PathBuf],
        message: &str,
    ) -> Result<String, GitError> {
        let mut add_args = vec!["add".to_string(), "--".to_string()];
        add_args.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
        self.run_git(repo_path, add_args).map(|_| ())?;
        self.run_git(repo_path, ["commit", "-m", message])
            .map(|_| ())?;
        self.head_sha(repo_path)
    }

    fn push_branch(
        &self,
        repo_path: &Path,
        remote_name: &str,
        branch: &str,
    ) -> Result<(), GitError> {
        self.run_git(repo_path, ["push", remote_name, branch])
            .map(|_| ())
    }

    fn push_refspec(
        &self,
        repo_path: &Path,
        remote_name: &str,
        refspec: &str,
    ) -> Result<(), GitError> {
        self.run_git(repo_path, ["push", remote_name, refspec])
            .map(|_| ())
    }

    fn add_detached_worktree(
        &self,
        repo_path: &Path,
        worktree_path: &Path,
        target: &str,
    ) -> Result<(), GitError> {
        self.run_git(
            repo_path,
            [
                "worktree",
                "add",
                "--detach",
                worktree_path.to_string_lossy().as_ref(),
                target,
            ],
        )
        .map(|_| ())
    }

    fn remove_worktree(&self, repo_path: &Path, worktree_path: &Path) -> Result<(), GitError> {
        self.run_git(
            repo_path,
            [
                "worktree",
                "remove",
                "--force",
                worktree_path.to_string_lossy().as_ref(),
            ],
        )
        .map(|_| ())
    }

    fn delete_branch(&self, repo_path: &Path, branch: &str) -> Result<(), GitError> {
        if !self.local_branch_exists(repo_path, branch)? {
            return Ok(());
        }

        self.run_git(repo_path, ["branch", "-D", branch])
            .map(|_| ())
    }

    fn derive_patch_commits(
        &self,
        request: &PatchDerivationRequest,
    ) -> Result<Vec<PatchCommit>, GitError> {
        let range = format!("{}..{}", request.base_ref, request.patch_branch);
        let output = self.run_git(
            &request.repo_path,
            ["log", "--reverse", "--format=%H%x00%s", &range],
        )?;

        if output.is_empty() {
            return Ok(Vec::new());
        }

        Ok(output
            .lines()
            .filter_map(|line| {
                let (sha, summary) = line.split_once('\0')?;
                Some(PatchCommit {
                    sha: sha.to_string(),
                    summary: summary.to_string(),
                })
            })
            .collect())
    }

    fn replay_patch_stack(&self, request: &ReplayRequest) -> Result<ReplayResult, GitError> {
        self.checkout(&request.repo_path, &request.candidate_branch)?;

        let mut applied_commits = Vec::new();

        for patch in &request.patch_commits {
            let command = render_command(
                &request.repo_path,
                &[
                    OsStr::new("cherry-pick"),
                    OsStr::new("--allow-empty"),
                    OsStr::new(&patch.sha),
                ],
            );
            let output = Command::new("git")
                .arg("-C")
                .arg(&request.repo_path)
                .args(["cherry-pick", "--allow-empty", &patch.sha])
                .output()
                .map_err(|source| GitError::Io { command, source })?;

            if !output.status.success() {
                let _ = self.run_git_allow_failure(&request.repo_path, ["cherry-pick", "--abort"]);
                return Ok(ReplayResult {
                    status: ReplayStatus::Conflict,
                    applied_commits,
                    conflict_commit: Some(patch.sha.clone()),
                    head_sha: self.head_sha(&request.repo_path).ok(),
                });
            }

            applied_commits.push(patch.sha.clone());
        }

        Ok(ReplayResult {
            status: ReplayStatus::Clean,
            applied_commits,
            conflict_commit: None,
            head_sha: Some(self.head_sha(&request.repo_path)?),
        })
    }
}

fn render_command<S: AsRef<OsStr>>(repo_path: &Path, args: &[S]) -> String {
    let rendered_args = args
        .iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    format!("git -C {} {}", repo_path.display(), rendered_args)
}
