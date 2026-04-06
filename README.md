# ForkSync

ForkSync is a Git-first tool for keeping a fork current with upstream while preserving a small layer of custom commits.

The intended user flow is simple:

1. Fork a repo on GitHub.
2. Clone your fork locally.
3. Run `forksync init`.
4. Keep working on `main`.
5. Let ForkSync replay your `main` commits onto new upstream changes automatically.


## How It Works

ForkSync tracks three branches:

- `main`: your normal authoring branch and default output branch
- `forksync/live`: the machine-generated synced result
- `forksync/patches`: an internal snapshot/debug branch

At sync time, ForkSync:

1. Fetches upstream and your fork.
2. Finds the commits you made on `main` since the last generated base.
3. Builds a fresh candidate branch from the latest upstream `HEAD`.
4. Reapplies ForkSync-managed files.
5. Replays your commit stack in order.
6. Uses the agent when there are conflicts that must be resolved.
7. Publishes `forksync/live` and, by default, force-updates `main`.

The main design rule is:

- upstream is the base truth
- your fork changes are a patch layer
- agent repair is the exception path, not the primary sync path

## Quick Start

Inside a forked repo:

```bash
forksync init
```

That will:

- detect the upstream remote when possible
- generate `.forksync.yml`
- generate `.github/workflows/forksync.yml`
- create the bootstrap commit in a detached temporary worktree
- create/update the management branches
- try to push the managed refs for you

After that, keep working on `main`.

To test a local sync run manually:

```bash
forksync sync --trigger local-debug
```

## Safe Local Demo Commands

If you want to see the real `forksync init` flow without touching a real fork:

```bash
forksync dev init --prepare-only
```

That creates a disposable sandbox repo and prints the exact command to run next.

If you want ForkSync to create the sandbox and immediately launch the real interactive `init` flow:

```bash
forksync dev init
```

If you want workflow smoke testing through `act`:

```bash
forksync dev act
```

## Configuration Notes

Important defaults:

- output branch: `main`
- live branch: `forksync/live`
- internal patch/debug branch: `forksync/patches`
- validation mode: `none` unless you provide commands
- default agent provider: `OpenCode`
- default model: `opencode/gpt-5-nano`

Validation commands can already be set during `init` with flags such as:

```bash
forksync init --build-command "cargo build --workspace" --test-command "cargo test --workspace"
```

## Observability

ForkSync uses structured `tracing` internally.

By default, normal CLI output is intentionally quiet.

Use:

- `--verbose` for human-readable logs
- `--json-logs` for structured log output
- `OTEL_EXPORTER_OTLP_ENDPOINT=...` to export telemetry

## TODOs

- [ ] Publish ForkSync as a versioned GitHub Action release with immutable tags, a moving major tag, and upgrade guidance.
- [ ] Prove GitHub-hosted runner packaging end to end with prebuilt binaries instead of source-build fallback.
- [ ] Decide whether the GitHub Action should bundle the OpenCode runtime or continue installing it dynamically.
- [ ] Implement real GitHub failure PR reuse/upsert on the standing conflict branch.
- [ ] Add a protected-branch fallback for `forksync init`, such as opening a PR instead of requiring a direct push.
- [ ] Add validation timeout handling.
- [ ] Add an interactive validation wizard for collecting build and test commands.
- [ ] Make replay of commits that mix managed files and normal files path-aware.
- [ ] Expand GitHub-side auth-failure and infra-failure coverage.
- [ ] Add example config, troubleshooting, and end-user install/debug docs.
- [ ] Publish easier install paths such as `pnpx`, Homebrew, and other non-Rust-first entrypoints.
- [ ] Decide the long-term fallback plan if OpenCode free models or runtime assumptions change materially.
- [ ] Either wire additional agent providers fully or narrow the exposed provider choices until they are real.
- [ ] Add backup/recovery-anchor behavior and fuller live-only/output-branch safety coverage.
- [ ] Add snapshot/golden coverage for generated config and failure summaries.
- [ ] Add conflict-memory and more advanced auto-resolution behavior.
- [ ] Decide the long-term hosted story, if any, for non-open-source orchestration features.
- [ ] Decide whether future public fork sharing should look like a directory, registry, or patch-stacking model.

## Contributing

Contributor rules for this repository:

- always follow TDD
- always write strongly typed schemas and think schema-first
- use explicit state machines where they clarify lifecycle transitions or side-effect boundaries
- keep the CLI and workflow wrappers thin over reusable library APIs
- add or update tests whenever behavior changes
- prefer deterministic engine behavior over implicit magic
- preserve the swappable agent abstraction even while OpenCode is the only fully wired provider
- update this README when the product shape or TODO list changes

Definition of done for a feature:

1. A failing test exists for the intended behavior.
2. Minimal implementation makes the test pass.
3. Refactor preserves passing tests.
4. Docs are updated if user-facing behavior changed.
