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

## User Flow

The primary user journey in v1 should be optimized for a fork-first, almost-no-config setup.

### Happy path: real user journey

1. A user is browsing GitHub and finds a repo they want to fork.
2. They click Fork on GitHub.
3. They clone their fork locally and open it in their editor or agent environment.
4. In the repo root, they run `forksync init`.
5. ForkSync attempts zero-config setup using defaults:
   - detect the upstream parent repo
   - detect the upstream default branch
   - assume output branch `main`
   - assume patch branch `forksync/patches`
   - assume live branch `forksync/live`
   - assume validation mode `none`
   - assume GitHub workflow installation is desired
6. ForkSync writes `.forksync.yml`.
7. ForkSync writes a GitHub Actions workflow file under `.github/workflows/`.
8. ForkSync creates or updates the local branches needed for management:
   - `forksync/patches`
   - `forksync/live`
9. ForkSync optionally performs an initial local sync preview or initial sync, depending on the command mode.
10. The user reviews the generated config and workflow.
11. The user commits the generated files to their fork and pushes.
12. From that point on, GitHub Actions keeps the fork current on schedule and via manual dispatch.

### No-config goal

The no-config experience should be:

- clone fork
- run `forksync init`
- review generated files
- commit
- push

That is the UX bar to optimize for.

### What `forksync init` must do in v1

`forksync init` is the product entrypoint. It should:

- verify the current directory is a Git repo
- inspect `origin` and infer that the current repo is a fork when possible
- detect upstream repo and default branch when possible
- fall back to explicit flags only when detection fails
- generate a complete `.forksync.yml` from typed defaults
- generate the GitHub workflow file
- create local management branches if missing
- offer or perform an initial sync path
- print the exact next steps for the user

### What the user should see after `forksync init`

The user should be able to understand ForkSync from the created artifacts alone:

- `.forksync.yml` explains what branches and policies are in play
- `.github/workflows/forksync.yml` shows when sync runs
- `forksync/patches` is where the user keeps their custom changes
- `forksync/live` is the machine-generated result
- `main` stays current automatically unless configured otherwise

### Local user experience before pushing

Before trusting GitHub Actions, a user should be able to test ForkSync locally:

1. Clone their fork.
2. Run `forksync init`.
3. Make a custom change on `forksync/patches`.
4. Simulate upstream movement locally or point to a real upstream remote.
5. Run `forksync sync --trigger local-debug`.
6. Inspect:
   - resulting branch tips
   - generated state files
   - validation behavior
   - success or failure summaries

This local debug flow is the first dogfood milestone. GitHub Actions comes after the local experience is understandable.

## Path to First Local Dogfood Experience

The implementation plan should optimize for the earliest moment when you can personally act like a user on your laptop and see the whole system behave.

### Milestone 1: zero-config setup in a real fork clone

The first local user-visible milestone is:

- open a forked repo locally
- run `forksync init`
- see `.forksync.yml`
- see `.github/workflows/forksync.yml`
- see `forksync/patches`
- see `forksync/live`

This milestone does not require full sync behavior yet. It proves the setup UX.

### Milestone 2: local sync on synthetic repos

The next milestone is:

- generate temp upstream and fork repos in tests
- create patch commits
- simulate upstream movement
- run `forksync sync --trigger local-debug`
- assert `forksync/live`, output branch, and state behavior

This proves the deterministic engine before GitHub is involved.

### Milestone 3: local manual dogfood on a real fork

The next milestone is:

- use a real fork clone in `sandbox/repos/`
- run `forksync init`
- create a real patch on `forksync/patches`
- point at a real or simulated upstream
- run local sync repeatedly
- inspect the result as a user would

This is the milestone where the product becomes understandable in practice.

### Milestone 4: workflow smoke test

Only after the above should we validate:

- generated workflow wiring with `act`
- workflow environment behavior
- permissions assumptions

This keeps GitHub Actions validation downstream of the real engine and setup experience.

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
  - [ ] Integration tests prove the local-debug user flow

### PR 3: Init Flow and Branch Bootstrap

- [ ] Implement `forksync init`
  - [ ] Upstream detection hooks
  - [ ] Config generation
  - [ ] zero-config default path from a forked repo
  - [ ] Branch creation for `forksync/patches`
  - [ ] Branch creation for `forksync/live`
  - [ ] GitHub workflow file generation and installation
  - [ ] user-facing next-step output
- [ ] TDD scope
  - [ ] Unit tests for init defaults
  - [ ] Integration tests for branch bootstrap in synthetic repos
  - [ ] Integration tests for zero-config init from a simulated fork clone
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

## TDD Plan to Reach a Real Local Demo

To reach the first local user-testable experience, implementation should proceed in this order:

1. Write failing tests for `forksync init` default config generation from a simulated fork clone.
2. Implement only enough config and detection logic to generate `.forksync.yml`.
3. Write failing tests for workflow file generation.
4. Implement only enough workflow generation to emit `.github/workflows/forksync.yml`.
5. Write failing tests for creating `forksync/patches` and `forksync/live`.
6. Implement only enough branch bootstrap logic to make those tests pass.
7. Run the setup flow manually in a sandbox clone and document the observed UX gaps.
8. Write failing tests for local patch replay sync using temp repos.
9. Implement deterministic sync behavior until local-debug sync works.
10. Add `act` only after the local CLI flow is understandable end to end.

The critical rule is that each visible user step must be backed by a failing test before we add the behavior.

## First Local Demo Script

Once the setup and local sync paths exist, the first manual demo should look like this:

1. Fork a public GitHub repo.
2. Clone the fork into `sandbox/repos/<name>`.
3. Run `forksync init`.
4. Review `.forksync.yml` and `.github/workflows/forksync.yml`.
5. Commit the generated files.
6. Create a change on `forksync/patches`.
7. Simulate an upstream change.
8. Run `forksync sync --trigger local-debug`.
9. Inspect `forksync/live`, `main`, and `.forksync/state`.
10. Repeat with a conflict scenario.

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
- [ ] support for GitLab and other forge providers beyond GitHub
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
