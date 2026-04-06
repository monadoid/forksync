# ForkSync

ForkSync is a Git-first system for keeping small feature forks alive: fork early, patch locally, stay current automatically, and rely on deterministic automation plus bounded agent repair instead of a manual review lane.

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
  - [x] Developer script entrypoints created
  - [x] Git repository initialized
  - [x] Rust workspace scaffold created
  - [x] First crate manifests created
  - [x] First schema-first tests written
  - [x] First local dogfood slice works end to end
  - [x] `init` now bootstraps through a detached temporary worktree
  - [x] `init` auto-commits and attempts to push managed refs
  - [x] Main-authoring workflow is implemented locally
  - [x] Agentic conflict repair works locally through OpenCode
  - [x] Narrated local `--auto` demo script exists for repeatable dogfooding
  - [x] Initial TLA+ core spec and `tla_connect` replay harness added for sync-model checks

## What Works Today

- [x] `forksync init` bootstraps a forked repo with typed defaults
- [x] `.forksync.yml` and `.github/workflows/forksync.yml` are generated from Rust-owned config
- [x] `main` is the user authoring branch
- [x] `forksync/live` is updated as the machine-generated recovery/debug branch
- [x] local deterministic sync works on clean replay paths
- [x] same-file replay conflicts can be repaired locally through the OpenCode adapter
- [x] local state persists author-base and run-history data
- [x] repo-scoped sync locking prevents overlapping local sync runs in the same checkout
- [x] remote branch publication now uses explicit `--force-with-lease=<ref>:<expect>` plus atomic push semantics
- [x] `scripts/make_test_repos.sh --auto` demonstrates the end-to-end local conflict flow
- [x] abstract sync protocol now has a first formal model and Rust-side replay checks

## Current Gaps and Risks

- [ ] validation modes beyond `none` are not implemented yet
- [ ] failure PR reuse and GitHub-side failure surfaces are not implemented yet
- [ ] the generated workflow is still a local-development placeholder, not a published GitHub Action release
- [ ] local sync success is proven; GitHub-hosted runner packaging and dependency bootstrapping are not
- [ ] auth-failure and infra-failure coverage are still thin
- [ ] changing managed config on `main` currently replays as a user patch and can conflict with refreshed managed files
- [ ] logging is still mostly ad hoc prints; move CLI, engine, and Action paths onto `tracing` with structured logs and an OpenTelemetry-friendly sink model before release

## Current Dependency Assumptions

- [x] ForkSync currently uses OpenCode as the default agent backend
- [x] the default model is `opencode/gpt-5-nano`
- [x] the current local implementation expects an `opencode` binary to be available either on `PATH` or at `~/.opencode/bin/opencode`
- [ ] ForkSync does not yet install OpenCode automatically on GitHub runners
- [ ] ForkSync does not yet bundle an agent runtime inside the GitHub Action package
- [ ] we have not yet locked the long-term fallback plan if OpenCode free built-in models disappear or change materially

## Next Steps

1. Harden the local proof of concept with the missing validation, auth-failure, no-change, and infra-failure coverage.
2. Decide and implement the runner dependency strategy for OpenCode on GitHub Actions.
3. Package and publish ForkSync as a versioned GitHub Action with explicit release-tag policy.
4. Implement GitHub-side failure surfaces: standing failure branch, standing PR reuse, and structured summaries.
5. Expand the public-facing docs from "coordination plan" into "install, run, and debug" documentation.

## Open Product Decisions

- [ ] keep OpenCode built-in free models as the default zero-config path, or require explicit credentials by default
- [ ] decide whether GitHub Action setup should install OpenCode dynamically on the runner or use a packaged/bundled runtime strategy
- [ ] decide how much of the long-term product stays fully open source versus optional hosted services
- [ ] decide whether public fork sharing should look like a directory, registry, or stackable patch-source model
- [ ] decide what version-upgrade story users get for the GitHub Action without introducing an unsafe auto-update channel

## Draft GitHub Action Release Policy

- [ ] publish immutable release tags for exact versions such as `v1.0.0`
- [ ] maintain a moving major tag such as `v1` only for users who explicitly want convenience over maximum immutability
- [ ] document SHA pinning as the most secure consumption option
- [ ] provide explicit upgrade notes instead of any silent auto-update behavior

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

The repository is currently at "strong local proof of concept" stage:

- local setup and sync UX is real
- local conflict repair is real
- GitHub Action publishing, dependency bootstrapping, and GitHub-native failure handling are still ahead

## Locked Design Decisions

- [x] v1 mode is action-only polling
- [x] Future hosted evented mode remains an architectural seam
- [x] Default branches are `main` for authoring/output, `forksync/live` for generated recovery, and `forksync/patches` as an internal snapshot/debug branch
- [x] `forksync/live` is the authoritative generated branch
- [x] `main` is the user authoring branch and also the default output branch
- [x] User patch derivation is based on commits on `main` since the recorded generated author base
- [x] Validation defaults to `none` when the user does not specify commands
- [x] Reckless mode is the default posture
- [x] Output branch force updates are allowed by default
- [x] Failure handling reuses one PR
- [x] Private upstream auth is PAT-first in v1
- [x] OpenCode is the default v1 agent provider
- [x] `opencode/gpt-5-nano` is the default v1 model
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
- agent failure is the exception path
- everything user-facing is strongly typed and schema-driven
- branch semantics must stay clean enough for future patch sharing and stacking

## Branch Model

Primary branches:

- `main`: the user-maintained fork branch and default output branch
- `forksync/live`: the machine-generated continuously synced result
- `forksync/patches`: an internal snapshot/debug branch that preserves the pre-sync authored state when helpful

Current authoring rule in the implemented v1 slice:

- human-authored fork changes belong on `main`
- ForkSync derives the user patch layer from commits on `main` since the last generated author base
- `forksync/live` is the machine-generated recovery/debug branch
- `forksync/patches` is internal and should not be required knowledge for normal users

Update semantics:

1. Always rebuild `forksync/live`.
2. Derive user commits from `main` since the recorded author base.
3. Force-update `main` from the regenerated candidate when `sync.update_output_branch = true`.
4. Default to force-updating the output branch because the product is optimized for "keep me current" over "fail safely on local drift".

## Sync Model

Canonical strategy: replay user commits from `main` onto latest upstream HEAD.

Concurrency posture:

- GitHub Actions `concurrency` is a scheduler-level optimization and should reduce duplicate work, but it is not the sole correctness guard.
- The local `fs4` lock only protects one checkout on one machine and is primarily for local or self-hosted reuse scenarios.
- Remote correctness should rely on explicit leased pushes so a stale run fails cleanly if another run already updated the managed refs.

Runtime flow:

1. Load config and effective defaults.
2. Acquire the local checkout lock when running in a shared checkout.
3. Fetch fork and upstream remotes.
4. Resolve latest upstream SHA.
5. Derive user commits from `main` since the recorded generated author base.
6. Exit early with `NoChange` only if the upstream SHA is already processed and there are no new user commits on `main`.
7. Snapshot the authored `main` state into the internal patch/debug branch when configured.
8. Create a fresh candidate branch from upstream HEAD.
9. Reapply ForkSync-managed files.
10. Replay the user commit stack in stable order.
11. If replay conflicts, invoke the agent repair step.
12. Run validation if configured.
13. On success, publish `forksync/live` and optionally `main` with explicit leased atomic pushes so stale runs fail instead of overwriting newer remote refs.
14. Record the new generated author base.
15. On failure, open or update one standing failure PR.
16. Persist state and run history.

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
- agent repair can be explicitly disabled per repo with `agent.enabled: false` or per run with `forksync sync --no-agent`
- `opencode/gpt-5-nano` by default
- built-in no-login OpenCode path by default when available
- reckless prompt profile
- bounded retries
- bounded runtime
- file edits allowed
- new commits allowed
- command execution allowed

Agent responsibility:

- repair replay conflicts when user commits from `main` stop applying cleanly
- optionally repair validation failures when enabled
- never replace the deterministic engine as the primary sync path

Agent design rules:

- the engine depends on an agent trait, not a concrete provider
- `OpenCode` is the first concrete adapter we wire up
- provider selection belongs in typed config, even if v1 only fully exercises one provider
- adding a second provider later should not require redesigning the engine pipeline
- future hosted ForkSync agent mode should fit behind the same typed provider seam
- the current repair loop owns a typed `edit_file` / `finish` tool protocol in Rust so providers stay swappable

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
- create commits on `main` and simulate upstream movement
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
   - assume live branch `forksync/live`
   - assume internal patch/debug branch `forksync/patches`
   - assume validation mode `none`
   - assume GitHub workflow installation is desired
6. ForkSync prepares `.forksync.yml` and `.github/workflows/forksync.yml` inside a bootstrap commit.
7. ForkSync creates that bootstrap commit in a detached temporary worktree so the generated setup can be applied without hijacking the user's current checkout.
9. ForkSync creates or updates the local branches needed for management:
   - `main`
   - `forksync/live`
   - `forksync/patches` for internal snapshot/debug use
10. ForkSync attempts to push the bootstrap refs to `origin` automatically so the user does not have to remember to push the managed branches by hand.
11. ForkSync leaves the user with `main` as the normal authoring branch.
12. From that point on, GitHub Actions keeps the fork current on schedule and via manual dispatch.

### No-config goal

The no-config experience should be:

- clone fork
- run `forksync init`
- let ForkSync create and try to push the bootstrap commit automatically
- keep working on `main`
- let ForkSync track the generated base in the background
- inspect `forksync/live` only when debugging or recovering

That is the UX bar to optimize for.

### What `forksync init` must do in v1

`forksync init` is the product entrypoint. It should:

- verify the current directory is a Git repo
- inspect `origin` and infer that the current repo is a fork when possible
- detect upstream repo and default branch when possible
- fall back to explicit flags only when detection fails
- generate a complete `.forksync.yml` from typed defaults
- generate the GitHub workflow file
- create the bootstrap commit inside a detached temporary worktree
- create local management branches if missing
- update local `main` to the bootstrap commit when safe to do so
- attempt to push the managed refs to `origin`
- be safe to rerun: no-op by default when already initialized, regenerate only with `--force`
- tell the user to keep working on `main`
- print the exact next steps for the user

### What the user should see after `forksync init`

The user should be able to understand ForkSync from the created artifacts alone:

- `.forksync.yml` explains what branches and policies are in play on `main`
- `.github/workflows/forksync.yml` shows when sync runs on `main`
- `main` is where the user keeps their custom changes
- `forksync/live` is the machine-generated result
- `forksync/patches` exists only for internal snapshot/debug use
- the configured output branch stays current automatically unless configured otherwise

### Local user experience before pushing

Before trusting GitHub Actions, a user should be able to test ForkSync locally:

1. Clone their fork.
2. Run `forksync init`.
3. Inspect the bootstrap commit on `main`.
4. Make a custom change on `main`.
5. Simulate upstream movement locally or point to a real upstream remote.
6. Run `forksync sync --trigger local-debug`.
7. Inspect:
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
- let ForkSync create the bootstrap commit and managed refs automatically
- inspect `.forksync.yml` and `.github/workflows/forksync.yml` on `main`
- keep working on `main`
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
- create a real patch on `main`
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
- [x] Create Rust workspace manifest
- [x] Add placeholder crate manifests

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

- [x] Implement `forksync-git` foundations
  - [x] Thin wrappers around Git command orchestration
  - [x] Repository discovery and remote helpers
  - [x] Branch creation and reset helpers
- [x] Build test harness support
  - [x] Temp repo factory utilities
  - [x] Commit helper APIs
  - [x] Remote wiring helpers
  - [x] Branch assertion helpers
- [ ] Add initial integration scenarios
  - [x] no-conflict sync fixture template
  - [ ] textual conflict fixture template
  - [x] no-validation fixture template
- [x] TDD scope
  - [x] Integration tests drive core Git orchestration APIs
  - [x] Integration tests prove the local-debug user flow

### PR 3: Init Flow and Branch Bootstrap

- [x] Implement `forksync init`
  - [x] Upstream detection hooks
  - [x] Config generation
  - [x] zero-config default path from a forked repo
  - [x] detached temp-worktree bootstrap flow
  - [x] automatic bootstrap commit creation
  - [x] Branch creation for internal `forksync/patches` snapshot/debug use
  - [x] Branch creation for `forksync/live`
  - [x] automatic push attempts for managed refs
  - [x] GitHub workflow file generation and installation
  - [x] user-facing next-step output
- [ ] TDD scope
  - [ ] Unit tests for init defaults
  - [x] Integration tests for branch bootstrap in synthetic repos
  - [x] Integration tests for zero-config init from a simulated fork clone
  - [ ] Failure tests for missing upstream data

### PR 4: State Persistence and Run History

- [x] Implement `forksync-state`
  - [x] State directory layout
  - [x] Last processed upstream SHA
  - [x] Last good sync SHA
  - [x] Author base SHA
  - [x] Run history persistence
- [ ] TDD scope
  - [x] Unit tests for serialization
  - [ ] Unit tests for trimming and overwrite semantics
  - [x] Integration tests for state persistence across sync runs

### PR 5: Author Commit Derivation

- [x] Implement user-commit derivation from recorded author base
  - [x] Commit range calculation
  - [x] Stable ordering rules
  - [ ] Merge-base fallback behaviors where explicitly needed
- [ ] TDD scope
  - [x] Integration tests for commit selection
  - [ ] Regression tests for patch-only replay vs full branch history
  - [ ] Multi-upstream-change replay scenarios

### PR 6: Deterministic Sync Engine

- [x] Implement `forksync-engine` sync pipeline
  - [x] Fetch
  - [x] Upstream SHA resolution
  - [x] candidate branch creation
  - [x] replay user commits from `main`
  - [x] success and failure outcomes
- [x] Implement `SyncOutcome`
- [ ] TDD scope
  - [ ] no-change scenario
  - [x] clean replay scenario
  - [x] replay conflict scenario with agent repair
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

- [x] Implement workflow generation
  - [x] schedule trigger
  - [x] workflow_dispatch trigger
  - [x] permissions block
  - [x] concurrency group
  - [x] CLI invocation placeholder
- [x] Add generated workflow outputs
- [ ] Add `scripts/run_act.sh`
- [ ] TDD scope
  - [x] Workflow generation tests
  - [ ] Local `act` smoke validation

## TDD Plan to Reach a Real Local Demo

To reach the first local user-testable experience, implementation should proceed in this order:

1. Write failing tests for `forksync init` default config generation from a simulated fork clone.
2. Implement only enough config and detection logic to generate `.forksync.yml`.
3. Write failing tests for workflow file generation.
4. Implement only enough workflow generation to emit `.github/workflows/forksync.yml`.
5. Write failing tests for bootstrapping `main` and `forksync/live`.
6. Implement only enough branch bootstrap logic to make those tests pass.
7. Run the setup flow manually in a sandbox clone and document the observed UX gaps.
8. Write failing tests for local main-authoring replay sync using temp repos.
9. Implement deterministic sync behavior until local-debug sync works.
10. Add `act` only after the local CLI flow is understandable end to end.

The critical rule is that each visible user step must be backed by a failing test before we add the behavior.

## First Local Demo Script

Once the setup and local sync paths exist, the first manual demo should look like this:

1. Fork a public GitHub repo.
2. Clone the fork into `sandbox/repos/<name>`.
3. Run `forksync init`.
4. Review the already-committed `.forksync.yml` and `.github/workflows/forksync.yml` on `main`.
5. Create a change on `main`.
7. Simulate an upstream change.
8. Run `forksync sync --trigger local-debug`.
9. Inspect `forksync/live`, `main`, and `.forksync/state`.
10. Repeat with a conflict scenario.

### PR 11: Agent Abstraction and Provider Loop

- [x] Implement `forksync-agent`
  - [x] repair trait
  - [x] bounded-attempt runtime contract
  - [x] `OpenCode` default provider loop
  - [x] swappable provider factory seam
  - [x] structured repair result reporting
- [ ] TDD scope
  - [ ] Unit tests for config gating
  - [x] Integration tests for agent-invocation decision points
  - [x] Failure propagation tests

### PR 12: Documentation and Hardening

- [ ] Expand README usage docs
- [ ] Add architecture notes
- [ ] Add example `.forksync.yml`
- [ ] Add troubleshooting guidance
- [ ] TDD scope
  - [ ] Validate docs examples against actual CLI behavior where feasible

## Detailed Build Checklist

- [x] Workspace foundation
  - [x] Root `Cargo.toml`
  - [x] Common toolchain settings
  - [x] Common lint settings
  - [x] Common test entrypoints
- [ ] Crates
  - [x] `forksync-cli`
  - [x] `forksync-config`
  - [x] `forksync-engine`
  - [x] `forksync-git`
  - [x] `forksync-agent`
  - [x] `forksync-github`
  - [x] `forksync-state`
- [ ] Commands
  - [x] `init`
  - [x] `sync`
  - [x] `validate`
  - [x] `print-config`
  - [x] `generate-workflow`
  - [x] `status`
  - [x] `rollback` placeholders
  - [x] `registry` placeholders
- [ ] Sync behavior
  - [x] effective default resolution
  - [x] concurrency lock
  - [x] explicit leased remote publication
  - [x] upstream fetch
  - [x] dedupe by upstream SHA
  - [x] candidate branch creation
  - [x] user-commit derivation from recorded author base
  - [x] replay user commits from `main`
  - [x] agent repair path
  - [ ] validation path
  - [x] live branch update
  - [x] output branch update
  - [x] state persistence
- [ ] Failure handling
  - [ ] standing failure branch policy
  - [ ] standing failure PR reuse
  - [ ] structured summary generation
  - [ ] artifact/log hooks
- [ ] Test coverage
  - [x] unit tests for config
  - [x] unit tests for state
  - [x] integration tests for Git flows
  - [x] integration tests for conflict handling
  - [ ] integration tests for validation failure
  - [ ] integration tests for auth failure
  - [ ] local `act` workflow checks

## Non-MVP / Planned but Not in v1

- [ ] hosted event-driven sync mode via GitHub App or relay
- [ ] support for GitLab and other forge providers beyond GitHub
- [ ] user-friendly CLI distribution via npm, Homebrew, and cargo install flows
- [ ] bring-your-own OpenCode provider credentials and model selection
- [ ] hosted ForkSync agent mode with user login
- [ ] optional public fork directory or registry so users can opt in to sharing discoverable forks
- [ ] patch stacking and composition from multiple shared public fork sources
- [ ] restrict agent edits to conflict files or an explicit allowlist for stronger safety
- [ ] deterministic auto-detection of build, test, and install commands
- [ ] richer validation profiles
- [ ] patch registry for publishing reusable patch layers
- [ ] package or install runner dependencies explicitly instead of assuming preinstalled agent tooling
- [ ] explicit fallback strategy if OpenCode free built-in models disappear or change
- [ ] published, versioned GitHub Action release policy with pinned tags and upgrade guidance
- [ ] conflict fingerprint memory and auto-resolution learning
- [ ] org-grade auth and GitHub App installs for private repos
- [ ] multiple output branch strategies
- [ ] advanced reviewer routing and CODEOWNERS integration
- [ ] semantic merge assistance and structural diffing
- [ ] observability dashboards and hosted metrics

## Initial Files to Add Next

These are expected in the next implementation PR:

- validation runner beyond `validation.mode = none`
- local no-change sync coverage
- GitHub Action packaging and dependency bootstrapping
- improved workflow execution via `act`

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
- use explicit state machines where they clarify Rust lifecycle transitions or side-effect boundaries
- keep the CLI and workflow wrappers thin over reusable library APIs
- add or update tests whenever behavior changes
- prefer deterministic engine behavior over implicit magic
- preserve the swappable agent abstraction even while `OpenCode` is the only fully wired provider
- update this README checklist when a PR lands
