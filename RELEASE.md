# Release Runbook

This repository uses a single release orchestrator workflow:

- `.github/workflows/release.yml` (`Release`)

The workflow handles release branch creation, version bump, crate publish, tag creation, and GitHub release asset upload in one run.

## Prerequisites

1. You have write access to run workflows.
2. crates.io trusted publisher is configured for this repository/package.
3. The workflow file exists on the default branch (`master`).

## Workflow Roles

1. `.github/workflows/release.yml` (`Release`) is the canonical release workflow:
- validates release inputs
- bumps `Cargo.toml` and `Cargo.lock`
- publishes crate (real mode)
- creates tag and GitHub release assets (real mode)

2. `.github/workflows/build.yml` (`Build Release Artifacts`) is a manual utility:
- builds Linux/macOS binaries for any input ref
- uploads build artifacts
- does not bump version
- does not publish to crates.io
- does not create tag or GitHub release

## Normal Release Procedure

1. Open **Actions** in GitHub.
2. Select **Release** workflow.
3. Click **Run workflow** on `master`.
4. Enter version as `X.Y.Z` (example: `0.4.14`).
5. Choose `dry_run`:
- `true` for safe validation (recommended first run)
- `false` for real publish/tag/release
6. Start run.

## What the Workflow Does

1. Validates:
- trigger branch is default branch
- version format is semver (`X.Y.Z`)
- target tag does not already exist
- crate version is not already published

2. Prepares release commit:
- bumps `Cargo.toml` + `Cargo.lock`
- runs `cargo check --locked`
- when `dry_run=false`: pushes `release/vX.Y.Z` branch and opens/reuses release PR

3. Builds artifacts (Linux + macOS) with `fail-fast: false`.

4. Publishes crate:
- `cargo publish --dry-run --locked`
- when `dry_run=false`: `cargo publish --locked`

5. Finalizes release:
- when `dry_run=false`: tags release commit (`vX.Y.Z`), pushes tag, and creates GitHub release with binaries + checksums

## Post-Release Step

1. Merge the release PR (`release/vX.Y.Z`) into `master`.
2. (Not applicable for `dry_run=true`, because no PR is created.)

## Failure Recovery

1. **Fails in `dry_run=true` mode**:
- fix workflow/build issues and rerun `dry_run=true` until green

2. **Fails before `Publish Crate` in real mode**:
- fix issue
- re-run workflow with same version

3. **Fails during `Publish Crate` in real mode**:
- if publish failed, fix issue and rerun same version
- if publish succeeded but later steps failed, do not rerun `Release` with the same version (the precheck will block already-published versions)

4. **Crate published but tag/release failed**:
- manually create the missing tag and GitHub release from the `release/vX.Y.Z` branch commit
- merge the existing release PR after tag/release is in place
- do not publish a new crate version unless you intentionally want a new release
