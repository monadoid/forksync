#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  cat <<EOF
Usage:
  ./scripts/make_test_repos.sh
  ./scripts/make_test_repos.sh <name>
  ./scripts/make_test_repos.sh <absolute-or-relative-path>

Behavior:
  - no argument: recreates $ROOT_DIR/sandbox/repos/demo
  - bare name: recreates $ROOT_DIR/sandbox/repos/<name>
  - path: recreates the exact directory you pass
EOF
}

if [[ "${1:-}" == "--help" ]]; then
  usage
  exit 0
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
git clone --bare "$UPSTREAM_WORKING" "$FORK_REMOTE" >/dev/null
git clone "$FORK_REMOTE" "$USER_REPO" >/dev/null
git -C "$USER_REPO" config user.name "ForkSync Demo"
git -C "$USER_REPO" config user.email "forksync-demo@example.com"
git -C "$USER_REPO" remote add upstream "$UPSTREAM_REMOTE"
git -C "$USER_REPO" fetch upstream >/dev/null

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
  3. git switch forksync/patches
  4. git show HEAD:.forksync.yml
  5. echo "local patch" > PATCH.txt
  6. git add PATCH.txt && git commit -m "Add local patch"
  7. echo "upstream change" > "$UPSTREAM_WORKING/UPSTREAM.txt"
  8. git -C "$UPSTREAM_WORKING" add UPSTREAM.txt
  9. git -C "$UPSTREAM_WORKING" commit -m "Add upstream change"
 10. git -C "$UPSTREAM_WORKING" push "$UPSTREAM_REMOTE" main
 11. forksync sync --trigger local-debug --no-agent
 12. git show main:PATCH.txt
 13. git show main:UPSTREAM.txt

Notes:
  - You can omit the argument entirely to recreate the default demo at:
      $ROOT_DIR/sandbox/repos/demo
  - Under the current ForkSync model, make your own code changes on:
      forksync/patches
    The output branch (main by default) is machine-managed.
EOF
