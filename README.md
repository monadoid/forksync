# ForkSync

ForkSync is a Git-first system for keeping small feature forks alive: fork early, patch locally, stay current automatically, and only bother a human when deterministic automation or bounded repair cannot finish the job.

This repository README is the coordination file for the project. It captures the v1 implementation plan, repository structure, TDD rules, delivery plan, and progress tracking.

## Status

- [x] Repository coordination scaffold started
  - [x] Root implementation plan captured in this README
  - [x] Initial repository structure chosen
  - [x] Local testing strategy defined
  - [x] PR breakdown defined
  - [x] `.gitignore` added
  - [x] `AGENTS.md` symlinked to `README.md`
  - [x] Sandbox and fixture directories created
  - [x] Developer script entrypoints stubbed
  - [x] Git repository initialized
  - [x] Rust workspace scaffold created
  - [x] First crate manifests created
  - [x] First schema-first tests written

## Product Statement

ForkSync v1 is an open-source Rust CLI packaged as a GitHub Action that keeps forks synced with upstream automatically.

The v1 product posture is:

- Action-only polling mode
- Git-first deterministic sync engine
- Patch replay from upstream HEAD
- Agentic repair only when replay or optional validation repair breaks
- Direct push on green by default
- Single reused failure PR on red
- Public upstreams first, private upstreams via PAT
- Strongly typed schema-first configuration
- OpenCode as the initial agent provider behind a swappable agent interface

## Locked Design Decisions

- [x] v1 mode is action-only polling
- [x] Future hosted evented mode remains an architectural seam
- [x] Default branches are `forksync/patches`, `forksync/live`, and `main`
- [x] `forksync/live` is the authoritative generated branch
- [x] `main` updates by default for simple UX
- [x] Patch derivation is based on commits since recorded patch base
- [x] Validation defaults to `none` when the user does not specify commands
- [x] Reckless mode is the default posture
- [x] Output branch force updates are allowed by default
- [x] Failure handling reuses one PR
- [x] Private upstream auth is PAT-first in v1
- [x] OpenCode is the default v1 agent provider
- [x] Agent integration must remain swappable behind a stable abstraction seam
- [x] TDD is mandatory for implementation work
- [x] Local engine tests are the primary harness
- [x] `act` is supplementary workflow validation, not the core harness

## Guiding Principles

- upstream is the base truth
- local customization is a patch layer
- sync should be deterministic first
- agentic repair should only happen when deterministic replay breaks
- green automation should push by default
- human review is the exception path
- everything user-facing is strongly typed and schema-driven
- branch semantics must stay clean enough for future patch sharing and stacking

## Branch Model

Primary branches:

- `forksync/patches`: the user-maintained local customization layer
- `forksync/live`: the machine-generated continuously synced result
- `main`: the default user-facing output branch

Update semantics:

1. Always rebuild `forksync/live`.
2. Update `main` from `forksync/live` when `sync.update_output_branch = true`.
3. Default to force-updating the output branch because the product is optimized for "keep me current" over "fail safely on local drift".

## Sync Model

Canonical strategy: patch replay from latest upstream HEAD.

Runtime flow:

1. Load config and effective defaults.
2. Acquire concurrency lock.
3. Fetch fork and upstream remotes.
4. Resolve latest upstream SHA.
5. Exit early with `NoChange` if the SHA is already processed and the run is not forced.
6. Create a fresh candidate branch from upstream HEAD.
7. Derive active patch commits from `forksync/patches` since recorded patch base.
8. Replay the patch stack in stable order.
9. If replay conflicts, invoke the agent repair step.
10. Run validation if configured.
11. On success, update `forksync/live`, then the configured output branch.
12. On failure, open or update one standing failure PR.
13. Persist state and run history.

## Validation Model

Supported v1 modes:

- `none`
- `build_only`
- `build_and_tests`
- `custom`

Wizard default:

- If the user does not provide build or test commands, default `validation.mode` to `none`.

Out of scope for v1:

- Automatic build/test/install command detection

## Agent Model

ForkSync v1 will wire `OpenCode` first, but the design must not hard-code the engine to one coding agent forever.

Agent defaults:

- enabled
- OpenCode provider by default
- reckless prompt profile
- bounded retries
- bounded runtime
- file edits allowed
- new commits allowed
- command execution allowed

Agent responsibility:

- repair patch replay conflicts
- optionally repair validation failures when enabled
- never replace the deterministic engine as the primary sync path

Agent design rules:

- the engine depends on an agent trait, not a concrete provider
- `OpenCode` is the first concrete adapter we wire up
- provider selection belongs in typed config, even if v1 only fully exercises one provider
- adding a second provider later should not require redesigning the engine pipeline

## Authentication Model

Public upstreams:

- anonymous fetch by default

Private upstreams:

- PAT first in v1
- GitHub App later

## Local-First Development Strategy

ForkSync must be fully developable locally.

### Layer A: engine-level local integration tests

Primary harness:

- create upstream repos in temp directories
- create fork repos in temp directories
- create patch branches and commits
- simulate upstream movement
- invoke the Rust library or CLI locally
- assert branch tips, SHAs, state files, failure payload inputs, and sync outcomes

This layer is the default path for TDD because it is fast, deterministic, and debuggable.

### Layer B: workflow-level local tests

Secondary harness:

- validate GitHub Actions wiring with `act`
- check environment passing, permissions, and shell integration
- avoid treating `act` as a perfect reproduction of GitHub-hosted runners

## Test Harness Structure

Tracked test assets:

- `tests/integration/`: integration tests
- `tests/fixtures/repo_templates/`: template directories used to generate synthetic repos for tests
- `tests/fixtures/scenarios/`: scenario-specific metadata or expected-output fixtures when needed

Ephemeral test repos:

- generated in OS temp directories during tests
- never committed to the repository
- owned by the integration harness

Manual local sandboxes:

- `sandbox/repos/`: manual scratch repos for interactive debugging
- `sandbox/act/`: optional local `act` scratch space and captured outputs
- ignored by Git by default

Scripts:

- `scripts/run_act.sh`: local workflow runner wrapper
- `scripts/make_test_repos.sh`: optional helper for manual sandbox generation

### Design Rule for Testability

The CLI must remain a thin shell over a reusable library API. The core engine must be callable from:

- CLI commands
- integration tests
- GitHub Action wrappers
- future hosted workers

## Proposed Repository Layout

```text
forksync/
  .github/
    workflows/
  crates/
    forksync-cli/
    forksync-config/
    forksync-engine/
    forksync-git/
    forksync-agent/
    forksync-github/
    forksync-state/
  tests/
    integration/
    fixtures/
      repo_templates/
      scenarios/
  sandbox/
    repos/
    act/
  scripts/
  README.md
  AGENTS.md -> README.md
  .gitignore
```

## TDD Rules

Every implementation PR must follow test-first development.

- [x] Coordination PR may establish structure and planning without production code
- [ ] For crate work, start with failing tests before production logic
- [ ] Prefer unit tests for config/defaults/schema behavior
- [ ] Prefer integration tests for git orchestration and sync behavior
- [ ] Add regression tests for every bug fixed
- [ ] Add workflow-level checks only after engine coverage exists
- [ ] Do not merge production behavior without a corresponding test or explicit justification

Definition of done for a feature:

1. A failing test exists for the intended behavior.
2. Minimal implementation makes the test pass.
3. Refactor preserves passing tests.
4. Docs and checklist state are updated.

## Implementation Roadmap

### PR 0: Repo Bootstrap and Coordination

- [x] Create root README as the canonical plan
- [x] Define repository layout
- [x] Define temp-repo and sandbox strategy
- [x] Define PR breakdown
- [x] Add `.gitignore`
- [x] Add `AGENTS.md` symlink to `README.md`
- [x] Add developer scripts stubs
- [ ] Create Rust workspace manifest
- [ ] Add placeholder crate manifests

### PR 1: Workspace and Typed Config Skeleton

- [x] Create Cargo workspace
  - [x] Add workspace members for all planned crates
  - [x] Add shared lint/test formatting configuration
- [x] Implement `forksync-config`
  - [x] Typed config structs
  - [x] Serde serialization and deserialization
  - [x] Defaults in Rust
  - [x] Versioned config model
  - [x] YAML read and write helpers
- [x] Implement CLI skeleton
  - [x] `clap` command tree
  - [x] `print-config`
  - [x] `validate --config-only`
- [x] TDD scope
  - [x] Unit tests for defaults
  - [x] Unit tests for enum parsing
  - [x] Unit tests for config round-tripping
  - [ ] Snapshot or golden tests for generated default config

### PR 2: Local Git Harness and Repo Factories

- [ ] Implement `forksync-git` foundations
  - [ ] Thin wrappers around Git command orchestration
  - [ ] Repository discovery and remote helpers
  - [ ] Branch creation and reset helpers
- [ ] Build test harness support
  - [ ] Temp repo factory utilities
  - [ ] Commit helper APIs
  - [ ] Remote wiring helpers
  - [ ] Branch assertion helpers
- [ ] Add initial integration scenarios
  - [ ] no-conflict sync fixture template
  - [ ] textual conflict fixture template
  - [ ] no-validation fixture template
- [ ] TDD scope
  - [ ] Integration tests drive all Git orchestration APIs

### PR 3: Init Flow and Branch Bootstrap

- [ ] Implement `forksync init`
  - [ ] Upstream detection hooks
  - [ ] Config generation
  - [ ] Branch creation for `forksync/patches`
  - [ ] Branch creation for `forksync/live`
  - [ ] Optional workflow installation hooks
- [ ] TDD scope
  - [ ] Unit tests for init defaults
  - [ ] Integration tests for branch bootstrap in synthetic repos
  - [ ] Failure tests for missing upstream data

### PR 4: State Persistence and Run History

- [ ] Implement `forksync-state`
  - [ ] State directory layout
  - [ ] Last processed upstream SHA
  - [ ] Last good sync SHA
  - [ ] Patch base SHA
  - [ ] Run history with max-entry trimming
- [ ] TDD scope
  - [ ] Unit tests for serialization
  - [ ] Unit tests for trimming and overwrite semantics
  - [ ] Integration tests for state persistence across sync runs

### PR 5: Patch Derivation

- [ ] Implement patch derivation from recorded patch base
  - [ ] Commit range calculation
  - [ ] Stable ordering rules
  - [ ] Merge-base fallback behaviors where explicitly needed
- [ ] TDD scope
  - [ ] Integration tests for commit selection
  - [ ] Regression tests for patch-only replay vs full branch history
  - [ ] Multi-upstream-change replay scenarios

### PR 6: Deterministic Sync Engine

- [ ] Implement `forksync-engine` sync pipeline
  - [ ] Fetch
  - [ ] Upstream SHA resolution
  - [ ] candidate branch creation
  - [ ] patch replay
  - [ ] success and failure outcomes
- [ ] Implement `SyncOutcome`
- [ ] TDD scope
  - [ ] no-change scenario
  - [ ] clean replay scenario
  - [ ] replay conflict scenario before agent handoff
  - [ ] auth failure scenario
  - [ ] infra failure scenario

### PR 7: Validation Execution

- [ ] Implement validation runner
  - [ ] `none`
  - [ ] `build_only`
  - [ ] `build_and_tests`
  - [ ] `custom`
  - [ ] timeout handling
- [ ] TDD scope
  - [ ] Unit tests for mode resolution
  - [ ] Integration tests for success and failure execution paths
  - [ ] Validation-disabled scenarios

### PR 8: Output Branch Update and Safety Controls

- [ ] Update `forksync/live`
- [ ] Force-update configured output branch when enabled
- [ ] Implement backup-before-update behavior
- [ ] TDD scope
  - [ ] Integration tests for live-only mode
  - [ ] Integration tests for output branch force updates
  - [ ] Recovery-anchor tests for backup creation

### PR 9: GitHub Failure Surfaces

- [ ] Implement `forksync-github` payload generation
  - [ ] standing failure PR metadata
  - [ ] summary body rendering
  - [ ] labels, mentions, assignments, requested reviews
- [ ] TDD scope
  - [ ] Unit tests for rendered PR bodies
  - [ ] Unit tests for reuse behavior inputs
  - [ ] Snapshot tests for summaries

### PR 10: Workflow Generator and GitHub Action Wiring

- [ ] Implement workflow generation
  - [ ] schedule trigger
  - [ ] workflow_dispatch trigger
  - [ ] permissions block
  - [ ] concurrency group
  - [ ] CLI invocation
- [ ] Add `.github/workflows/` templates or generated outputs
- [ ] Add `scripts/run_act.sh`
- [ ] TDD scope
  - [ ] Golden tests for workflow YAML generation
  - [ ] Local `act` smoke validation

### PR 11: Agent Abstraction and Stub Provider

- [ ] Implement `forksync-agent`
  - [ ] repair trait
  - [ ] bounded-attempt runtime contract
  - [ ] `OpenCode` default provider stub
  - [ ] swappable provider factory seam
  - [ ] structured repair result reporting
- [ ] TDD scope
  - [ ] Unit tests for config gating
  - [ ] Integration tests for agent-invocation decision points
  - [ ] Failure propagation tests

### PR 12: Documentation and Hardening

- [ ] Expand README usage docs
- [ ] Add architecture notes
- [ ] Add example `.forksync.yml`
- [ ] Add troubleshooting guidance
- [ ] TDD scope
  - [ ] Validate docs examples against actual CLI behavior where feasible

## Detailed Build Checklist

- [ ] Workspace foundation
  - [ ] Root `Cargo.toml`
  - [ ] Common toolchain settings
  - [ ] Common lint settings
  - [ ] Common test entrypoints
- [ ] Crates
  - [ ] `forksync-cli`
  - [ ] `forksync-config`
  - [ ] `forksync-engine`
  - [ ] `forksync-git`
  - [ ] `forksync-agent`
  - [ ] `forksync-github`
  - [ ] `forksync-state`
- [ ] Commands
  - [ ] `init`
  - [ ] `sync`
  - [ ] `validate`
  - [ ] `print-config`
  - [ ] `generate-workflow`
  - [ ] `status`
  - [ ] `rollback`
  - [ ] `registry` placeholders
- [ ] Sync behavior
  - [ ] effective default resolution
  - [ ] concurrency lock
  - [ ] upstream fetch
  - [ ] dedupe by upstream SHA
  - [ ] candidate branch creation
  - [ ] patch derivation from recorded patch base
  - [ ] patch replay
  - [ ] agent repair path
  - [ ] validation path
  - [ ] live branch update
  - [ ] output branch update
  - [ ] state persistence
- [ ] Failure handling
  - [ ] standing failure branch policy
  - [ ] standing failure PR reuse
  - [ ] structured summary generation
  - [ ] artifact/log hooks
- [ ] Test coverage
  - [ ] unit tests for config
  - [ ] unit tests for state
  - [ ] integration tests for Git flows
  - [ ] integration tests for conflict handling
  - [ ] integration tests for validation failure
  - [ ] integration tests for auth failure
  - [ ] local `act` workflow checks

## Non-MVP / Planned but Not in v1

- [ ] hosted event-driven sync mode via GitHub App or relay
- [ ] deterministic auto-detection of build, test, and install commands
- [ ] richer validation profiles
- [ ] patch registry for publishing reusable patch layers
- [ ] patch stacking and composition from multiple sources
- [ ] conflict fingerprint memory and auto-resolution learning
- [ ] org-grade auth and GitHub App installs for private repos
- [ ] multiple output branch strategies
- [ ] advanced reviewer routing and CODEOWNERS integration
- [ ] semantic merge assistance and structural diffing
- [ ] observability dashboards and hosted metrics

## Initial Files to Add Next

These are expected in the next implementation PR:

- first integration harness support under `tests/integration/`
- initial workflow generation tests
- first temp-repo factory helpers
- first state persistence tests

## Notes for Contributors and Builder Agents

- Treat this README as the coordination source of truth.
- Update checkboxes as work lands.
- Keep implementation scoped to the current PR slice.
- Preserve the architectural seam between engine, CLI, Action wiring, and future hosted orchestration.
- Default to tests first.

## Contributing

Contributing rules for this repository:

- always follow TDD
- always write strongly typed schemas and think schema-first
- keep the CLI and workflow wrappers thin over reusable library APIs
- add or update tests whenever behavior changes
- prefer deterministic engine behavior over implicit magic
- preserve the swappable agent abstraction even while `OpenCode` is the only fully wired provider
- update this README checklist when a PR lands
