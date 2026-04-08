# ForkSync Release Notes

ForkSync ships as three coordinated artifacts:

- the Rust CLI and engine
- the GitHub Action
- the `forksync` npm launcher

## Binary-first packaging

The public action and npm launcher both prefer prebuilt release binaries.

Resolution order:

1. explicit binary path
2. cached release binary
3. downloaded release binary
4. dev-only source-build fallback, only when explicitly enabled

The action verifies release downloads against a sibling `.sha256` file when available.

## npm launcher

The npm package name is `@tabslabs/forksync`.

Typical usage:

```bash
pnpx @tabslabs/forksync init
```

The launcher accepts pass-through CLI arguments and wrapper-only overrides:

- `--binary-path`
- `--binary-version`
- `--repository`
- `--allow-build-fallback`

## GitHub Action

Public workflows should pin to:

```yaml
uses: monadoid/forksync@v1
```

The action defaults to downloading the latest compatible release binary.
Use `binary-path` only for local development or test harnesses.

## Manual release process

Release a new version by pushing a semver tag:

```bash
git tag v1.2.3
git push origin v1.2.3
```

The release workflow will:

- build platform binaries
- generate checksums
- publish a GitHub release
- move the floating `v1` tag
- publish the npm package

## Dev escape hatch

Source-build fallback is intentionally off by default.

Enable it only when you are working locally and need to build from source:

```bash
forksync dev act --docker
```

or by setting the relevant launcher/action fallback flag explicitly.
