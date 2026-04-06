#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "==> Running Rust-side tla_connect replay checks"
cargo test -p forksync-engine --test formal_model -- --nocapture

if command -v tlc >/dev/null 2>&1; then
  echo "==> Running TLC (default output-update mode)"
  tlc -config formal/tla/ForkSyncCore.cfg formal/tla/ForkSyncCore.tla
  echo "==> Running TLC (live-only mode)"
  tlc -config formal/tla/ForkSyncCore.live_only.cfg formal/tla/ForkSyncCore.tla
elif [[ -n "${TLA2TOOLS_JAR:-}" ]]; then
  echo "==> Running TLC via java (default output-update mode)"
  java -cp "$TLA2TOOLS_JAR" tlc2.TLC -config formal/tla/ForkSyncCore.cfg formal/tla/ForkSyncCore.tla
  echo "==> Running TLC via java (live-only mode)"
  java -cp "$TLA2TOOLS_JAR" tlc2.TLC -config formal/tla/ForkSyncCore.live_only.cfg formal/tla/ForkSyncCore.tla
else
  echo "==> Skipping TLC: set TLA2TOOLS_JAR or install a tlc binary"
fi

if command -v apalache-mc >/dev/null 2>&1; then
  echo "==> Running Apalache (default output-update mode)"
  apalache-mc check --cinit=ConstInit --inv=TypeInvariant --length=5 formal/tla/ForkSyncCore.tla
  echo "==> Running Apalache (live-only mode)"
  apalache-mc check --cinit=ConstInitLiveOnly --inv=TypeInvariant --length=5 formal/tla/ForkSyncCore.tla
else
  echo "==> Skipping Apalache: install apalache-mc to enable pure spec checks"
fi
