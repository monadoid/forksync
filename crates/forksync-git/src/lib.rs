use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::{debug, instrument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSpec {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchDerivationRequest {
    pub repo_path: PathBuf,
    pub patch_branch: String,
    pub base_ref: String,
    pub ignored_paths: Vec<PathBuf>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasedRefUpdate {
    pub remote_ref: String,
    pub expected_old_sha: Option<String>,
    pub new_sha: String,
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
    #[error(
        "git push lease was rejected for remote `{remote}` refs {refs:?}: expected remote values no longer matched"
    )]
    LeaseRejected {
        remote: String,
        refs: Vec<String>,
        stderr: String,
    },
}

pub trait GitBackend: Send + Sync {
    fn ensure_repo(&self, repo_path: &Path) -> Result<(), GitError>;
    fn git_dir(&self, repo_path: &Path) -> Result<PathBuf, GitError>;
    fn worktree_clean(&self, repo_path: &Path) -> Result<bool, GitError>;
    fn paths_clean(&self, repo_path: &Path, paths: &[PathBuf]) -> Result<bool, GitError>;
    fn current_ref(&self, repo_path: &Path) -> Result<String, GitError>;
    fn checkout(&self, repo_path: &Path, reference: &str) -> Result<(), GitError>;
    fn hard_reset(&self, repo_path: &Path, target: &str) -> Result<(), GitError>;
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
    fn resolve_remote_branch_tip(
        &self,
        repo_path: &Path,
        remote_name: &str,
        branch: &str,
    ) -> Result<Option<String>, GitError>;
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
    fn push_leased_ref_updates(
        &self,
        repo_path: &Path,
        remote_name: &str,
        updates: &[LeasedRefUpdate],
    ) -> Result<(), GitError>;
    fn add_detached_worktree(
        &self,
        repo_path: &Path,
        worktree_path: &Path,
        target: &str,
    ) -> Result<(), GitError>;
    fn remove_worktree(&self, repo_path: &Path, worktree_path: &Path) -> Result<(), GitError>;
    fn delete_branch(&self, repo_path: &Path, branch: &str) -> Result<(), GitError>;
    fn abort_cherry_pick(&self, repo_path: &Path) -> Result<(), GitError>;
    fn derive_patch_commits(
        &self,
        request: &PatchDerivationRequest,
    ) -> Result<Vec<PatchCommit>, GitError>;
    fn replay_patch_stack(&self, request: &ReplayRequest) -> Result<ReplayResult, GitError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGitBackend;

impl SystemGitBackend {
    #[instrument(skip_all, fields(repo_path = %repo_path.display()))]
    fn run_git<I, S>(&self, repo_path: &Path, args: I) -> Result<String, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args_vec: Vec<_> = args.into_iter().collect();
        let command = render_command(repo_path, &args_vec);
        debug!(command = %command, "running git command");
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

        debug!(
            status = output.status.code().unwrap_or_default(),
            "git command completed"
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[instrument(skip_all, fields(repo_path = %repo_path.display(), remote = %remote_name, refspec = %refspec))]
    pub fn dry_run_push_refspec(
        &self,
        repo_path: &Path,
        remote_name: &str,
        refspec: &str,
    ) -> Result<(), GitError> {
        self.run_git(repo_path, ["push", "--dry-run", remote_name, refspec])
            .map(|_| ())
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

    fn git_dir(&self, repo_path: &Path) -> Result<PathBuf, GitError> {
        let git_dir = PathBuf::from(self.run_git(repo_path, ["rev-parse", "--git-dir"])?);
        if git_dir.is_absolute() {
            Ok(git_dir)
        } else {
            Ok(repo_path.join(git_dir))
        }
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

    fn hard_reset(&self, repo_path: &Path, target: &str) -> Result<(), GitError> {
        self.run_git(repo_path, ["reset", "--hard", target])
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

    fn resolve_remote_branch_tip(
        &self,
        repo_path: &Path,
        remote_name: &str,
        branch: &str,
    ) -> Result<Option<String>, GitError> {
        let command = render_command(
            repo_path,
            &[
                OsStr::new("ls-remote"),
                OsStr::new("--heads"),
                OsStr::new(remote_name),
                OsStr::new(branch),
            ],
        );
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["ls-remote", "--heads", remote_name, branch])
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

        let stdout = String::from_utf8_lossy(&output.stdout);
        let sha = stdout
            .split_whitespace()
            .next()
            .map(|value| value.to_string());
        Ok(sha)
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

    fn push_leased_ref_updates(
        &self,
        repo_path: &Path,
        remote_name: &str,
        updates: &[LeasedRefUpdate],
    ) -> Result<(), GitError> {
        if updates.is_empty() {
            return Ok(());
        }

        let mut args = vec![
            "push".to_string(),
            "--atomic".to_string(),
            remote_name.to_string(),
        ];
        for update in updates {
            let expected = update.expected_old_sha.as_deref().unwrap_or("");
            args.push(format!(
                "--force-with-lease={}:{}",
                update.remote_ref, expected
            ));
        }
        for update in updates {
            args.push(format!("{}:{}", update.new_sha, update.remote_ref));
        }

        let command = render_command(repo_path, &args.iter().map(OsStr::new).collect::<Vec<_>>());
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(args.iter().map(|arg| arg.as_str()))
            .output()
            .map_err(|source| GitError::Io {
                command: command.clone(),
                source,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("stale info")
                || stderr.contains("remote ref updated since checkout")
                || stderr.contains("atomic push failed")
            {
                return Err(GitError::LeaseRejected {
                    remote: remote_name.to_string(),
                    refs: updates
                        .iter()
                        .map(|update| update.remote_ref.clone())
                        .collect(),
                    stderr,
                });
            }

            return Err(GitError::CommandFailed {
                command,
                status: output.status.code().unwrap_or(-1),
                stderr,
            });
        }

        Ok(())
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

    fn abort_cherry_pick(&self, repo_path: &Path) -> Result<(), GitError> {
        self.run_git(repo_path, ["cherry-pick", "--abort"])
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
                if commit_changes_only_ignored_paths(
                    self,
                    &request.repo_path,
                    sha,
                    &request.ignored_paths,
                )
                .ok()?
                {
                    return None;
                }
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

fn commit_changes_only_ignored_paths(
    git: &SystemGitBackend,
    repo_path: &Path,
    sha: &str,
    ignored_paths: &[PathBuf],
) -> Result<bool, GitError> {
    if ignored_paths.is_empty() {
        return Ok(false);
    }

    let output = git.run_git(repo_path, ["show", "--format=", "--name-only", sha])?;
    let changed_paths = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if changed_paths.is_empty() {
        return Ok(false);
    }

    Ok(changed_paths.iter().all(|changed| {
        ignored_paths
            .iter()
            .any(|ignored| ignored.to_string_lossy().as_ref() == *changed)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn leased_push_rejects_stale_remote_updates() {
        let temp = TempDir::new().expect("create tempdir");
        let remote = temp.path().join("remote.git");
        let clone_a = temp.path().join("clone-a");
        let clone_b = temp.path().join("clone-b");

        run_git(
            temp.path(),
            ["init", "--bare", remote.to_str().expect("utf-8 path")],
        );
        run_git(
            temp.path(),
            [
                "clone",
                remote.to_str().expect("utf-8 path"),
                clone_a.to_str().expect("utf-8 path"),
            ],
        );
        configure_repo(&clone_a);
        fs::write(clone_a.join("README.md"), "seed\n").expect("write seed file");
        run_git(&clone_a, ["add", "README.md"]);
        run_git(&clone_a, ["commit", "-m", "Initial commit"]);
        run_git(&clone_a, ["push", "origin", "HEAD:main"]);

        run_git(
            temp.path(),
            [
                "clone",
                remote.to_str().expect("utf-8 path"),
                clone_b.to_str().expect("utf-8 path"),
            ],
        );
        configure_repo(&clone_b);

        let git = SystemGitBackend;
        let expected_main = git
            .resolve_remote_branch_tip(&clone_b, "origin", "main")
            .expect("resolve initial remote main");

        fs::write(clone_a.join("A.txt"), "from a\n").expect("write a change");
        run_git(&clone_a, ["add", "A.txt"]);
        run_git(&clone_a, ["commit", "-m", "A change"]);
        let a_head = git.head_sha(&clone_a).expect("resolve a head");
        git.push_leased_ref_updates(
            &clone_a,
            "origin",
            &[LeasedRefUpdate {
                remote_ref: "refs/heads/main".to_string(),
                expected_old_sha: expected_main.clone(),
                new_sha: a_head,
            }],
        )
        .expect("push a update with lease");

        fs::write(clone_b.join("B.txt"), "from b\n").expect("write b change");
        run_git(&clone_b, ["add", "B.txt"]);
        run_git(&clone_b, ["commit", "-m", "B change"]);
        let b_head = git.head_sha(&clone_b).expect("resolve b head");
        let error = git
            .push_leased_ref_updates(
                &clone_b,
                "origin",
                &[LeasedRefUpdate {
                    remote_ref: "refs/heads/main".to_string(),
                    expected_old_sha: expected_main,
                    new_sha: b_head,
                }],
            )
            .expect_err("stale lease should be rejected");

        assert!(matches!(error, GitError::LeaseRejected { .. }));
    }

    fn configure_repo(repo_path: &Path) {
        run_git(repo_path, ["config", "user.name", "ForkSync Test"]);
        run_git(
            repo_path,
            ["config", "user.email", "forksync-test@example.com"],
        );
    }

    fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .status()
            .expect("run git command");
        assert!(status.success(), "git command failed: {:?}", args);
    }
}
