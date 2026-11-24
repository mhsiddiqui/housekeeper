# 0030: Prepare release 0.1.0

- **Date:** 2026-11-24
- **Branch / PR:** `chore/release-0.1.0`
- **Plan chunks:** 0.4 (releases)
- **Commits:** 9dd2b7a

## What changed
- `CHANGELOG.md`: the **Unreleased** section became **0.1.0 (2026-11-24)**, with a new empty **Unreleased** above it.
  Because nothing was released before, the section is written as a first release: one **Added** list, with a short
  summary at the top. The entries that compared against unreleased states ("instead of MIT", the two "Fixed" items
  about the manual source) are folded into that list, and the CI entry is left out because users do not see it.
- The version in `Cargo.toml` was already `0.1.0`, so the release tag is `v0.1.0`; no code or dependency changed.

## Why
Plan chunk 0.4: the release pipeline builds and tests packages on every pull request, but its last step (attaching
them to a GitHub release) only runs when a release is published. This prepares `main` for that first release, as
described in [releasing](../development/releasing.md).

## How it was verified
- `version = "0.1.0"` under `[workspace.package]` in `Cargo.toml`, which `release.yml` compares with the tag.
- The CI run of this pull request builds the static packages and tests them on every supported distribution (the same
  jobs the release runs before attaching anything).

## Known limits and follow-ups
- The owner publishes the GitHub release (tag `v0.1.0` on `main`); the Release workflow then attaches the packages
  and `SHA256SUMS`.
- Chunk 0.4 is marked done only after the assets are attached, in a follow-up change.
