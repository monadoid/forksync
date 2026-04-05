use forksync_config::from_yaml_str;
use forksync_state::{FileStateStore, StateStore};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct LocalForkFixture {
    _temp: TempDir,
    upstream_working: PathBuf,
    upstream_remote: PathBuf,
    fork_remote: PathBuf,
    user_repo: PathBuf,
}

#[test]
fn init_bootstraps_main_for_direct_authoring() {
    let fixture = create_local_fork_fixture();

    let output = run_cli(&fixture.user_repo, ["init"]);
    assert!(
        output.status.success(),
        "init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let state_path = fixture.user_repo.join(".forksync/state/state.yml");
    assert!(state_path.exists(), "expected state file to exist");

    let main_config = git_output(&fixture.user_repo, ["show", "main:.forksync.yml"]);
    let config = from_yaml_str(&main_config).expect("parse generated config from main branch");
    assert_eq!(config.upstream.remote_name, "upstream");
    assert_eq!(config.upstream.branch, "main");
    assert_eq!(config.branches.patch, "forksync/patches");
    assert_eq!(config.branches.live, "forksync/live");
    assert_eq!(config.branches.output, "main");

    let current_branch = git_output(&fixture.user_repo, ["branch", "--show-current"]);
    assert_eq!(current_branch, "main");
    assert_eq!(git_output(&fixture.user_repo, ["status", "--short"]), "");
    assert!(local_branch_exists(&fixture.user_repo, "forksync/live"));
    assert!(local_branch_exists(&fixture.user_repo, "forksync/patches"));
    assert!(
        git_output(&fixture.user_repo, ["show", "main:.forksync.yml"]).contains("forksync/patches")
    );
    assert!(
        git_output(&fixture.user_repo, ["show", "forksync/live:.forksync.yml"])
            .contains("forksync/patches")
    );
    assert!(
        git_output_git_dir(&fixture.fork_remote, ["show", "main:.forksync.yml"])
            .contains("forksync/patches")
    );
    assert!(remote_branch_exists(&fixture.fork_remote, "main"));
    assert!(remote_branch_exists(
        &fixture.fork_remote,
        "forksync/patches"
    ));
    assert!(remote_branch_exists(&fixture.fork_remote, "forksync/live"));

    let state = FileStateStore::new(state_path)
        .load()
        .expect("load persisted state");
    assert!(
        state.author_base_sha.is_some(),
        "expected author base sha to be recorded"
    );
}

#[test]
fn init_is_idempotent_when_repo_is_already_bootstrapped() {
    let fixture = create_local_fork_fixture();

    let first = run_cli(&fixture.user_repo, ["init"]);
    assert!(
        first.status.success(),
        "first init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let head_before = git_output(&fixture.user_repo, ["rev-parse", "HEAD"]);

    let second = run_cli(&fixture.user_repo, ["init"]);
    assert!(
        second.status.success(),
        "second init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("already initialized"));
    assert_eq!(
        git_output(&fixture.user_repo, ["rev-parse", "HEAD"]),
        head_before
    );
}

#[test]
fn init_force_succeeds_when_repo_is_bootstrapped_and_main_has_unrelated_changes() {
    let fixture = create_local_fork_fixture();

    let first = run_cli(&fixture.user_repo, ["init"]);
    assert!(
        first.status.success(),
        "first init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    fs::write(fixture.user_repo.join("README.md"), "user local edit\n")
        .expect("write unrelated local change");
    let head_before = git_output(&fixture.user_repo, ["rev-parse", "HEAD"]);

    let forced = run_cli(&fixture.user_repo, ["init", "--force"]);
    assert!(
        forced.status.success(),
        "forced init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&forced.stdout),
        String::from_utf8_lossy(&forced.stderr)
    );

    assert_eq!(
        git_output(&fixture.user_repo, ["branch", "--show-current"]),
        "main"
    );
    assert!(git_output(&fixture.user_repo, ["status", "--short"]).contains("README.md"));
    assert_eq!(
        git_output(&fixture.user_repo, ["rev-parse", "HEAD"]),
        head_before
    );
}

#[test]
fn init_keeps_dirty_feature_branch_checked_out() {
    let fixture = create_local_fork_fixture();

    git(&fixture.user_repo, ["switch", "-c", "feature/wip"]);
    fs::write(fixture.user_repo.join("WIP.txt"), "leave me alone\n").expect("write dirty file");

    let output = run_cli(&fixture.user_repo, ["init"]);
    assert!(
        output.status.success(),
        "init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        git_output(&fixture.user_repo, ["branch", "--show-current"]),
        "feature/wip"
    );
    assert_eq!(
        git_output(&fixture.user_repo, ["status", "--short"]),
        "?? WIP.txt"
    );
    assert!(
        git_output(&fixture.user_repo, ["show", "main:.forksync.yml"]).contains("forksync/patches")
    );
    assert!(
        git_output_git_dir(&fixture.fork_remote, ["show", "main:.forksync.yml"])
            .contains("forksync/patches")
    );
    assert!(remote_branch_exists(
        &fixture.fork_remote,
        "forksync/patches"
    ));
    assert!(remote_branch_exists(&fixture.fork_remote, "forksync/live"));
}

#[test]
fn sync_replays_main_commits_onto_updated_upstream() {
    let fixture = create_local_fork_fixture();

    let init_output = run_cli(&fixture.user_repo, ["init"]);
    assert!(
        init_output.status.success(),
        "init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init_output.stdout),
        String::from_utf8_lossy(&init_output.stderr)
    );

    fs::write(fixture.user_repo.join("PATCH.txt"), "local patch\n").expect("write patch file");
    git(&fixture.user_repo, ["add", "PATCH.txt"]);
    git(&fixture.user_repo, ["commit", "-m", "Add local patch"]);

    fs::write(
        fixture.upstream_working.join("UPSTREAM.txt"),
        "upstream change\n",
    )
    .expect("write upstream file");
    git(&fixture.upstream_working, ["add", "UPSTREAM.txt"]);
    git(
        &fixture.upstream_working,
        ["commit", "-m", "Add upstream change"],
    );
    git(
        &fixture.upstream_working,
        [
            "push",
            fixture.upstream_remote.to_str().expect("utf-8 path"),
            "main",
        ],
    );

    let sync_output = run_cli(
        &fixture.user_repo,
        ["sync", "--trigger", "local-debug", "--no-agent"],
    );
    assert!(
        sync_output.status.success(),
        "sync failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&sync_output.stdout),
        String::from_utf8_lossy(&sync_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&sync_output.stdout);
    assert!(stdout.contains("SyncedDeterministic"));

    let main_patch = git_output(&fixture.user_repo, ["show", "main:PATCH.txt"]);
    let main_upstream = git_output(&fixture.user_repo, ["show", "main:UPSTREAM.txt"]);
    let live_patch = git_output(&fixture.user_repo, ["show", "forksync/live:PATCH.txt"]);
    let live_upstream = git_output(&fixture.user_repo, ["show", "forksync/live:UPSTREAM.txt"]);
    let main_config = git_output(&fixture.user_repo, ["show", "main:.forksync.yml"]);

    assert_eq!(main_patch, "local patch");
    assert_eq!(main_upstream, "upstream change");
    assert_eq!(live_patch, "local patch");
    assert_eq!(live_upstream, "upstream change");
    assert!(main_config.contains("forksync/patches"));

    let state = FileStateStore::new(fixture.user_repo.join(".forksync/state/state.yml"))
        .load()
        .expect("load state after sync");
    assert!(
        state.last_processed_upstream_sha.is_some(),
        "expected last processed upstream sha to be populated"
    );
    assert!(
        state.last_good_sync_sha.is_some(),
        "expected last good sync sha to be populated"
    );
    assert_eq!(
        state.author_base_sha.as_deref(),
        state.last_good_sync_sha.as_deref(),
        "author base should advance to the latest generated main/live state after sync"
    );
}

#[test]
fn sync_conflict_reports_failed_agent_instead_of_human_review() {
    let fixture = create_local_fork_fixture();

    let init_output = run_cli(&fixture.user_repo, ["init"]);
    assert!(
        init_output.status.success(),
        "init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init_output.stdout),
        String::from_utf8_lossy(&init_output.stderr)
    );

    fs::write(
        fixture.user_repo.join("README.md"),
        "seed repo\nlocal change\n",
    )
    .expect("write local readme change");
    git(&fixture.user_repo, ["add", "README.md"]);
    git(&fixture.user_repo, ["commit", "-m", "Local readme change"]);

    fs::write(
        fixture.upstream_working.join("README.md"),
        "seed repo\nupstream change\n",
    )
    .expect("write upstream readme change");
    git(&fixture.upstream_working, ["add", "README.md"]);
    git(
        &fixture.upstream_working,
        ["commit", "-m", "Upstream readme change"],
    );
    git(
        &fixture.upstream_working,
        [
            "push",
            fixture.upstream_remote.to_str().expect("utf-8 path"),
            "main",
        ],
    );

    let sync_output = run_cli(
        &fixture.user_repo,
        ["sync", "--trigger", "local-debug", "--no-agent"],
    );
    assert!(
        sync_output.status.success(),
        "sync failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&sync_output.stdout),
        String::from_utf8_lossy(&sync_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&sync_output.stdout);
    assert!(stdout.contains("FailedAgent"));
    assert!(stdout.contains("agent repair is disabled"));

    let state = FileStateStore::new(fixture.user_repo.join(".forksync/state/state.yml"))
        .load()
        .expect("load state after failed agent path");
    assert_eq!(
        state.history.last().map(|record| record.outcome),
        Some(forksync_state::RecordedOutcome::FailedAgent)
    );
}

#[test]
fn sync_from_uninitialized_directory_shows_init_hint() {
    let temp = TempDir::new().expect("create tempdir");

    let output = run_cli(
        temp.path(),
        ["sync", "--trigger", "local-debug", "--no-agent"],
    );
    assert!(
        !output.status.success(),
        "sync unexpectedly succeeded:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no ForkSync config found at"));
    assert!(stderr.contains("Run `forksync init` from the fork repo root first"));
}

fn create_local_fork_fixture() -> LocalForkFixture {
    let temp = TempDir::new().expect("create tempdir");
    let upstream_working = temp.path().join("upstream-working");
    let upstream_remote = temp.path().join("upstream-remote.git");
    let fork_remote = temp.path().join("fork-remote.git");
    let user_repo = temp.path().join("user-repo");

    fs::create_dir_all(&upstream_working).expect("create upstream working dir");
    git(&upstream_working, ["init", "-b", "main"]);
    git(&upstream_working, ["config", "user.name", "ForkSync Test"]);
    git(
        &upstream_working,
        ["config", "user.email", "forksync-test@example.com"],
    );
    fs::write(upstream_working.join("README.md"), "seed repo\n").expect("write seed readme");
    git(&upstream_working, ["add", "README.md"]);
    git(
        &upstream_working,
        ["commit", "-m", "Initial upstream commit"],
    );

    git(
        temp.path(),
        [
            "clone",
            "--bare",
            upstream_working.to_str().expect("utf-8 path"),
            upstream_remote.to_str().expect("utf-8 path"),
        ],
    );
    git(
        temp.path(),
        [
            "clone",
            "--bare",
            upstream_working.to_str().expect("utf-8 path"),
            fork_remote.to_str().expect("utf-8 path"),
        ],
    );
    git(
        temp.path(),
        [
            "clone",
            fork_remote.to_str().expect("utf-8 path"),
            user_repo.to_str().expect("utf-8 path"),
        ],
    );
    git(&user_repo, ["config", "user.name", "ForkSync Test"]);
    git(
        &user_repo,
        ["config", "user.email", "forksync-test@example.com"],
    );
    git(
        &user_repo,
        [
            "remote",
            "add",
            "upstream",
            upstream_remote.to_str().expect("utf-8 path"),
        ],
    );
    git(&user_repo, ["fetch", "upstream"]);

    LocalForkFixture {
        _temp: temp,
        upstream_working,
        upstream_remote,
        fork_remote,
        user_repo,
    }
}

fn run_cli<const N: usize>(cwd: &Path, args: [&str; N]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_forksync"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run forksync cli")
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

fn local_branch_exists(cwd: &Path, branch: &str) -> bool {
    Command::new("git")
        .current_dir(cwd)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()
        .expect("run git show-ref")
        .success()
}

fn remote_branch_exists(git_dir: &Path, branch: &str) -> bool {
    Command::new("git")
        .args([
            "--git-dir",
            git_dir.to_str().expect("utf-8 path"),
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()
        .expect("run git show-ref for bare repo")
        .success()
}

fn git_output_git_dir<const N: usize>(git_dir: &Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .args(["--git-dir", git_dir.to_str().expect("utf-8 path")])
        .args(args)
        .output()
        .expect("run git command against bare repo");
    assert!(
        output.status.success(),
        "git command failed in bare repo {}\nargs: {:?}\nstdout:\n{}\nstderr:\n{}",
        git_dir.display(),
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
