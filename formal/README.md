# ForkSync Formal Checks

This directory holds the first formal-modeling slice for ForkSync.

Scope:

- `formal/tla/ForkSyncCore.tla`: abstract TLA+ model of the current single-source sync flow
- `formal/tla/ForkSyncCore.cfg`: TLC config for bounded local model checking
- `formal/tla/ForkSyncCore.live_only.cfg`: TLC config for live-only output mode
- `crates/forksync-engine/tests/formal_model.rs`: Rust-side `tla_connect` replay harness

The intent is layered:

1. Keep normal Rust tests for the real implementation.
2. Model the sync protocol separately in TLA+.
3. Replay spec-shaped traces against a reduced Rust driver so spec and implementation logic stay aligned.

## What Runs Today

Always runnable:

- `cargo test -p forksync-engine --test formal_model`

This runs two inline ITF trace replays and, if `apalache-mc` is installed, also replays generated traces from the TLA+ spec.

Best-effort external checks:

- `scripts/run_formal_checks.sh`

That wrapper:

- runs the Rust-side `tla_connect` replay test
- runs TLC if `tlc` or `TLA2TOOLS_JAR` is available
- runs Apalache if `apalache-mc` is available

## External Tooling

Required for pure TLA+ checks:

- Java plus TLC (`tlc` on `PATH` or `TLA2TOOLS_JAR` pointing to `tla2tools.jar`)
- `apalache-mc` on `PATH`

Not required for the Rust replay tests:

- TLC
- Apalache

## Why Both Layers Exist

Normal Rust tests still matter because they cover:

- real `git` behavior
- file IO and worktrees
- CLI parsing
- YAML/state serialization
- GitHub workflow wiring

The formal layer checks the protocol-level rules:

- success updates generated state coherently
- live-only mode updates `forksync/live` without rewriting the output branch
- no-change does not mutate generated branches
- lock state matches whether a sync is actively running
- success and failure append run history, while no-change does not
- failed agent repair does not advance good sync state
- future concurrency/stacking changes have an explicit state machine to evolve
