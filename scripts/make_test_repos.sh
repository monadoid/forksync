#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  cat <<EOF
Usage:
  ./scripts/make_test_repos.sh
  ./scripts/make_test_repos.sh --auto
  ./scripts/make_test_repos.sh --auto <name>
  ./scripts/make_test_repos.sh <name>
  ./scripts/make_test_repos.sh <absolute-or-relative-path>

Behavior:
  - no argument: recreates $ROOT_DIR/sandbox/repos/demo
  - --auto: recreates the demo repo, then runs a narrated end-to-end conflict demo
  - bare name: recreates $ROOT_DIR/sandbox/repos/<name>
  - path: recreates the exact directory you pass
EOF
}

if [[ "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

AUTO_MODE=false
if [[ "${1:-}" == "--auto" ]]; then
  AUTO_MODE=true
  shift
fi

RAW_DEST="${1:-demo}"
if [[ "$RAW_DEST" == /* || "$RAW_DEST" == .* ]]; then
  DEST_DIR="$RAW_DEST"
else
  DEST_DIR="$ROOT_DIR/sandbox/repos/$RAW_DEST"
fi

UPSTREAM_WORKING="$DEST_DIR/upstream-working"
UPSTREAM_REMOTE="$DEST_DIR/upstream-remote.git"
FORK_REMOTE="$DEST_DIR/fork-remote.git"
USER_REPO="$DEST_DIR/user-repo"

rm -rf "$DEST_DIR"
mkdir -p "$DEST_DIR"

git init -b main "$UPSTREAM_WORKING" >/dev/null
git -C "$UPSTREAM_WORKING" config user.name "ForkSync Demo"
git -C "$UPSTREAM_WORKING" config user.email "forksync-demo@example.com"
printf "seed repo\n" >"$UPSTREAM_WORKING/README.md"
git -C "$UPSTREAM_WORKING" add README.md
git -C "$UPSTREAM_WORKING" commit -m "Initial upstream commit" >/dev/null

git clone --bare "$UPSTREAM_WORKING" "$UPSTREAM_REMOTE" >/dev/null
git -C "$UPSTREAM_WORKING" remote add origin "$UPSTREAM_REMOTE"
git -C "$UPSTREAM_WORKING" fetch origin >/dev/null
git -C "$UPSTREAM_WORKING" branch --set-upstream-to=origin/main main >/dev/null
git clone --bare "$UPSTREAM_WORKING" "$FORK_REMOTE" >/dev/null
git clone "$FORK_REMOTE" "$USER_REPO" >/dev/null
git -C "$USER_REPO" config user.name "ForkSync Demo"
git -C "$USER_REPO" config user.email "forksync-demo@example.com"
git -C "$USER_REPO" remote add upstream "$UPSTREAM_REMOTE"
git -C "$USER_REPO" fetch upstream >/dev/null

run_auto_demo() {
  local sleep_seconds="1"

  narrate() {
    printf '\n[demo] %s\n' "$1"
  }

  pause() {
    sleep "$sleep_seconds"
  }

  narrate "Entering the user fork clone at $USER_REPO."
  pause
  narrate "Running 'forksync init' to bootstrap ForkSync with defaults."
  (
    cd "$USER_REPO"
    /Users/samfinton/.local/bin/forksync init
  ) >/tmp/forksync-auto-init.log 2>&1
  cat /tmp/forksync-auto-init.log
  pause

  narrate 'Editing the README in the user repo with "local change".'
  printf 'seed repo\nlocal change\n' >"$USER_REPO/README.md"
  pause
  narrate 'Committing the user-side change on main as "Local readme change".'
  git -C "$USER_REPO" add README.md
  git -C "$USER_REPO" commit -m "Local readme change" >/dev/null
  pause

  narrate 'Editing the README in the upstream working repo with "upstream change".'
  printf 'seed repo\nupstream change\n' >"$UPSTREAM_WORKING/README.md"
  pause
  narrate 'Committing and pushing the upstream-side change as "Upstream readme change".'
  git -C "$UPSTREAM_WORKING" add README.md
  git -C "$UPSTREAM_WORKING" commit -m "Upstream readme change" >/dev/null
  git -C "$UPSTREAM_WORKING" push >/tmp/forksync-auto-push.log 2>&1
  cat /tmp/forksync-auto-push.log
  pause

  narrate "Running 'forksync sync --trigger local-debug' so ForkSync can replay the local change on top of the updated upstream."
  (
    cd "$USER_REPO"
    /Users/samfinton/.local/bin/forksync sync --trigger local-debug
  ) >/tmp/forksync-auto-sync.log 2>&1
  cat /tmp/forksync-auto-sync.log
  pause

  narrate "Showing the final README on main."
  git -C "$USER_REPO" show main:README.md
  pause

  narrate "Showing the final README on forksync/live."
  git -C "$USER_REPO" show forksync/live:README.md
}

cat <<EOF
Created local ForkSync demo repos under:
  $DEST_DIR

Repo layout:
  upstream working repo: $UPSTREAM_WORKING
  upstream bare remote:  $UPSTREAM_REMOTE
  fork bare remote:      $FORK_REMOTE
  user clone:            $USER_REPO

Suggested local dogfood flow:
  1. cd "$USER_REPO"
  2. forksync init
  3. git show main:.forksync.yml
  4. echo "local patch" > PATCH.txt
  5. git add PATCH.txt && git commit -m "Add local patch"
  6. echo "upstream change" > "$UPSTREAM_WORKING/UPSTREAM.txt"
  7. git -C "$UPSTREAM_WORKING" add UPSTREAM.txt
  8. git -C "$UPSTREAM_WORKING" commit -m "Add upstream change"
  9. git -C "$UPSTREAM_WORKING" push
 10. forksync sync --trigger local-debug --no-agent
 11. git show main:PATCH.txt
 12. git show main:UPSTREAM.txt

Notes:
  - You can omit the argument entirely to recreate the default demo at:
      $ROOT_DIR/sandbox/repos/demo
  - Re-running this script deletes and recreates that demo directory from scratch.
    If you were already inside the old demo tree, `cd` back into the new one after it finishes.
  - Under the current ForkSync model, make your own code changes on:
      main
    ForkSync derives your local fork layer from commits on main since the last generated base.
  - ForkSync keeps the generated recovery/debug branch at:
      forksync/live
  - Run forksync commands only from:
      $USER_REPO
EOF

if [[ "$AUTO_MODE" == true ]]; then
  run_auto_demo
fi
