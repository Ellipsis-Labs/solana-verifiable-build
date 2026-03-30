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
6. Choose `skip_publish_check`:
- `false` for normal releases (default)
- `true` only for recovery when the crate version is already published and you need to finish tag/release
7. Start run.

## What the Workflow Does

1. Validates:
- trigger branch is default branch
- version format is semver (`X.Y.Z`)
- `skip_publish_check` is a valid boolean and only used with `dry_run=false`
- target tag does not already exist
- when `skip_publish_check=false`: crate version is not already published
- when `skip_publish_check=true`: crate version is already published

2. Prepares release commit:
- when `skip_publish_check=false`: bumps `Cargo.toml` + `Cargo.lock` and runs `cargo check --locked`
- when `dry_run=false` and `skip_publish_check=false`: pushes `release/vX.Y.Z` branch and opens/reuses release PR
- when `skip_publish_check=true`: reuses existing `release/vX.Y.Z` branch commit without rewriting it

3. Builds artifacts (Linux + macOS) with `fail-fast: false`.

4. Publishes crate:
- when `skip_publish_check=false`: `cargo publish --dry-run --locked`
- when `dry_run=false` and `skip_publish_check=false`: `cargo publish --locked`
- when `skip_publish_check=true`: skips crates.io publish checks and publish steps

5. Finalizes release:
- when `dry_run=false`: tags release commit (`vX.Y.Z`), pushes tag, and creates GitHub release with binaries + checksums
- when `dry_run=true`: `finalize_release` job is skipped

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
- if publish succeeded but later steps failed, rerun `Release` with `skip_publish_check=true` (do not rerun default mode)

4. **Crate published but tag/release failed**:
- rerun `Release` with the same version and `dry_run=false`, `skip_publish_check=true`
- this reuses the existing published crate version and continues with tag/release creation
- recovery mode requires `release/vX.Y.Z` to already exist on origin
- if the tag already exists but GitHub release is missing, create the GitHub release manually for that existing tag
- merge the existing release PR after tag/release is in place
- do not publish a new crate version unless you intentionally want a new release
