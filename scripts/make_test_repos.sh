#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR="${1:-$ROOT_DIR/sandbox/repos/demo}"

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
  4. git add .forksync.yml .github/workflows/forksync.yml .gitignore
  5. git commit -m "Add ForkSync bootstrap"
  6. echo "local patch" > PATCH.txt
  7. git add PATCH.txt && git commit -m "Add local patch"
  8. echo "upstream change" > "$UPSTREAM_WORKING/UPSTREAM.txt"
  9. git -C "$UPSTREAM_WORKING" add UPSTREAM.txt
 10. git -C "$UPSTREAM_WORKING" commit -m "Add upstream change"
 11. git -C "$UPSTREAM_WORKING" push "$UPSTREAM_REMOTE" main
 12. forksync sync --trigger local-debug --no-agent
 13. git show main:PATCH.txt
 14. git show main:UPSTREAM.txt
EOF
