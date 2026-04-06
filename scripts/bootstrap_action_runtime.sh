#!/usr/bin/env bash
set -euo pipefail

cd "${GITHUB_ACTION_PATH:?}"

cargo --version
git --version

if [[ "${FORKSYNC_INSTALL_OPENCODE:-true}" == "true" ]]; then
  if ! command -v opencode >/dev/null 2>&1; then
    curl -fsSL https://opencode.ai/install | bash
  fi
  opencode --version
fi

cargo build --release --bin forksync --locked
