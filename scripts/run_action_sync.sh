#!/usr/bin/env bash
set -euo pipefail

action_path="${GITHUB_ACTION_PATH:?}"
workspace="${GITHUB_WORKSPACE:?}"
working_directory="${FORKSYNC_WORKING_DIRECTORY:-.}"
trigger="${FORKSYNC_TRIGGER:-schedule}"
config_path="${FORKSYNC_CONFIG_PATH:-.forksync.yml}"

cd "${workspace}/${working_directory}"

cmd=("${action_path}/target/release/forksync")
if [[ "${FORKSYNC_VERBOSE:-false}" == "true" ]]; then
  cmd+=("--verbose")
fi
if [[ "${FORKSYNC_JSON_LOGS:-false}" == "true" ]]; then
  cmd+=("--json-logs")
fi
cmd+=("--config" "${config_path}" "sync" "--trigger" "${trigger}")

"${cmd[@]}"
