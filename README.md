# ForkSync

ForkSync keeps a fork current with upstream while preserving a small layer of custom commits.

The intended user flow is:

1. Fork a repo on GitHub.
2. Clone your fork locally.
3. Run `forksync init`.
4. Keep working on your output branch, which defaults to `main`.
5. Let ForkSync replay your fork changes onto new upstream updates automatically.

## Quick Start

Public install target:

```bash
pnpx forksync init
```

Current repo-local dogfood flow:

```bash
cargo run --bin forksync -- init
```

`forksync init` will:

- detect the upstream remote when possible
- generate `.forksync.yml`
- generate `.github/workflows/forksync.yml`
- create/update `forksync/live` and `forksync/patches`
- try to publish the managed refs for you
- offer a minimal interactive setup for:
  - direct publication to the output branch when it looks safe
  - `OpenCode` or `No AI`
  - optional public registry opt-in for GitHub-hosted forks

After that, keep working on your output branch and let the generated GitHub Action run ForkSync.

## How It Works

ForkSync tracks three primary branches:

- `main` or your configured output branch: your normal authoring branch
- `forksync/live`: the generated synced result
- `forksync/patches`: an internal snapshot/debug branch

At sync time, ForkSync:

1. Fetches upstream and origin.
2. Builds a fresh candidate branch from the latest upstream `HEAD`.
3. Rewrites ForkSync-managed files from the current config.
4. Replays imported public source commits, if configured.
5. Replays your local authored commits last.
6. Uses the agent only when replay conflicts require repair.
7. Publishes `forksync/live` and, by default, force-updates the output branch with explicit `--force-with-lease` protection.

The design rule is simple:

- upstream is the base truth
- your fork changes are a patch layer
- imported public sources are replayed before your local commits
- ForkSync automatically resolves what it can first, then uses the agent for conflicts that still need repair

## Public Sources

ForkSync now has a public-source model for sharing reusable fork layers.

- A source is a Git repo plus tracked branch.
- Configured sources live in `sources[]` in `.forksync.yml`.
- `forksync sync` fetches each enabled source, derives its patch layer from the merge-base with upstream, and replays those commits before your own local commits.
- Source ordering is deterministic and internal. It is not a user-facing knob.

You can configure sources during init:

```bash
forksync init --source owner/repo#main
```

Or later:

```bash
forksync registry add owner/repo#main
forksync registry list
forksync registry remove owner/repo#main
```

The repository also contains a Cloudflare Worker + D1 scaffold under `registry/` for a public browse/search/select experience.

Current registry URLs:

- production: `https://forksync-registry-prod.prosammer.workers.dev`
- staging: `https://forksync-registry-staging.prosammer.workers.dev`

## GitHub Action

Generated workflows use:

```yaml
uses: monadoid/forksync@v1
```

The action is a JavaScript launcher that prefers prebuilt release binaries and only falls back to source builds as a dev escape hatch.

Important runtime behavior:

- OpenCode install is automatic only when OpenCode is selected.
- normal CLI output is intentionally quiet
- leased pushes are the correctness guard for remote ref publication
- GitHub workflow `concurrency` remains the scheduler guard for hosted runs

## Defaults

Important defaults:

- output branch: detected default branch, usually `main`
- live branch: `forksync/live`
- patch/debug branch: `forksync/patches`
- default action ref: `monadoid/forksync@v1`
- validation mode: `none` unless you provide commands
- public agent choices: `OpenCode` and `No AI`
- default OpenCode model: `opencode/gpt-5-nano`

Validation commands can already be set during init:

```bash
forksync init --build-command "cargo build --workspace" --test-command "cargo test --workspace"
```

## Local Demo

Safe local demo commands:

```bash
forksync dev init --prepare-only
forksync dev init
forksync dev act
```

Use `forksync dev act` to smoke-test the real action path locally.

## Observability

ForkSync uses structured `tracing` internally.

Use:

- `--verbose` for human-readable logs
- `--json-logs` for structured logs
- `OTEL_EXPORTER_OTLP_ENDPOINT=...` to export telemetry

## TODOs

- [ ] Publish the first real versioned GitHub Action release and move `@v1` onto an immutable semver release flow.
- [ ] Publish the npm package and other non-Rust-first install paths for public users.
- [ ] Finish the bootstrap protected-branch fallback by auto-opening the PR, not just publishing the fallback branch.
- [ ] Finish standing conflict PR reuse end to end on GitHub-hosted runs.
- [ ] Expand GitHub-side auth and infra failure coverage.
- [ ] Add an interactive validation wizard instead of only flag-based validation setup.
- [ ] Finish public registry publish/update/unpublish from the Rust CLI against the deployed Worker with stronger end-to-end coverage.
- [ ] Add a custom registry domain and document its long-term operating model.
- [ ] Add example configs, troubleshooting docs, and end-user release/update guidance.
- [ ] Decide the long-term fallback plan if OpenCode runtime or free-model assumptions change materially.
- [ ] Decide whether future public sharing should stay registry-style or evolve toward richer patch stacking.
- [ ] Add conflict-memory and stronger automatic repair behavior.

## Contributing

Contributor rules for this repository:

- always follow TDD
- always write strongly typed schemas and think schema-first
- use explicit state machines where they clarify lifecycle transitions or side-effect boundaries
- keep the CLI and workflow wrappers thin over reusable library APIs
- add or update tests whenever behavior changes
- prefer deterministic engine behavior over implicit magic
- preserve the swappable agent abstraction even while OpenCode is the only fully wired public provider
- update this README when the product shape or TODO list changes

Definition of done for a feature:

1. A failing test exists for the intended behavior.
2. Minimal implementation makes the test pass.
3. Refactor preserves passing tests.
4. Docs are updated if user-facing behavior changed.
