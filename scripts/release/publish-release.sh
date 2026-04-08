#!/usr/bin/env bash
set -euo pipefail

TAG="${1:?release tag required}"
shift

if [[ $# -eq 0 ]]; then
  echo "usage: publish-release.sh <tag> <asset...>" >&2
  exit 1
fi

gh release create "$TAG" "$@" --title "$TAG" --generate-notes
