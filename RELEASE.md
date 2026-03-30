# Release Runbook

This repository uses a single release orchestrator workflow:

- `.github/workflows/release.yml` (`Release`)

The workflow handles release branch creation, version bump, artifact build, tag/release finalization, and crates.io publish in one run.

## Prerequisites

1. You have write access to run workflows.
2. crates.io trusted publisher is configured for this repository/package.
3. The workflow file exists on the default branch (`master`).

## Workflow Roles

1. `.github/workflows/release.yml` (`Release`) is the canonical release workflow:
- validates release inputs
- bumps `Cargo.toml` and `Cargo.lock`
- validates crate publishability with `cargo publish --dry-run` (normal mode)
- creates/reuses tag and GitHub release assets (real mode)
- publishes to crates.io as the final irreversible step (normal mode)

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
- records whether the target tag already exists (for safe reruns)
- when `skip_publish_check=false`: crate version is not already published
- when `skip_publish_check=true`: crate version is already published

2. Prepares release commit:
- when `skip_publish_check=false`: bumps `Cargo.toml` + `Cargo.lock` and runs `cargo check --locked`
- when `dry_run=false` and `skip_publish_check=false` and tag does not exist: pushes `release/vX.Y.Z` branch and opens/reuses release PR
- when `skip_publish_check=false` and tag already exists: reuses the tagged release commit for reruns
- when `skip_publish_check=true`: reuses existing `release/vX.Y.Z` branch commit without rewriting it

3. Builds artifacts (Linux + macOS) with `fail-fast: false`.

4. Publishes crate:
- when `skip_publish_check=false`: runs `cargo publish --dry-run --locked` as a preflight validation
- when `skip_publish_check=true`: skips dry-run publish validation

5. Finalizes release:
- when `dry_run=false`: creates/reuses tag (`vX.Y.Z`), creates GitHub release with binaries + checksums, then publishes to crates.io
- crates.io publish is the final step in normal mode
- when `skip_publish_check=true`: skips crates.io publish because version is already published
- when `dry_run=true`: `finalize_release` job is skipped

## Post-Release Step

1. Merge the release PR (`release/vX.Y.Z`) into `master`.
2. (Not applicable for `dry_run=true`, because no PR is created.)

## Failure Recovery

1. **Fails in `dry_run=true` mode**:
- fix workflow/build issues and rerun `dry_run=true` until green

2. **Fails before finalization in real mode**:
- fix issue
- re-run workflow with same version

3. **Fails during finalization before crates publish**:
- rerun workflow with the same version
- existing tag/commit state is reused where possible, and tag push is idempotent when it already points to the expected commit

4. **Crates publish step fails in real mode**:
- fix the publish error and rerun workflow with the same version
- if tag/release already exists from the previous attempt, rerun will reuse it and continue to the publish step

5. **Crate already published but tag/release missing**:
- rerun `Release` with the same version and `dry_run=false`, `skip_publish_check=true`
- this reuses the existing published crate version and continues with tag/release creation
- recovery mode requires `release/vX.Y.Z` to already exist on origin
- if the tag already exists but GitHub release is missing, create the GitHub release manually for that existing tag
- merge the existing release PR after tag/release is in place
- do not publish a new crate version unless you intentionally want a new release
