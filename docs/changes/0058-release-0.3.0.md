# 0058: Prepare release 0.3.0

- **Date:** 2026-11-29
- **Branch / PR:** `chore/release-0.3.0`, #48
- **Plan chunks:** none (release of the Phase 5 work since 0.2.0; see [plan](../planning/plan.md))
- **Commits:** filled in below before merging

## What changed
- `Cargo.toml`: `version = "0.3.0"` under `[workspace.package]`; `Cargo.lock` updated by `cargo check`.
- `CHANGELOG.md`: **Unreleased** became **0.3.0 (2026-11-29)**, with a new empty **Unreleased** section above it and
  a short summary (Housekeeper can now install what you download). Every entry was already under **Added**; none
  changed what 0.2.0 does.

## Why
Phase 5 is done (0051, 0054–0057): `inspect`, `install`, AppImage integration, and the refusal of archives and scripts.
These are new features, so this is a minor version under [Semantic Versioning](https://semver.org/) before 1.0.
`list --json` is unchanged (`schema` 2). `inspect --json` only gained the `sha256` field, and the step renamed in 0055
(`InstallPackageFiles`) was never released. The desktop app (Phase 6) is long, so releasing now publishes the install
work on its own. The steps follow [releasing](../development/releasing.md). Per 0052, the tag goes on this pull
request's merge commit, not on the tip of `main`.

## How it was verified
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` and
  `cargo test --workspace --all-features`: all passed.
- `cargo run --bin housekeeper -- --version` printed `housekeeper 0.3.0`.
- This pull request's CI builds the static packages and tests them on every supported distribution. The Release
  workflow runs the same jobs before it attaches anything.

## Known limits and follow-ups
- Publishing the GitHub release from tag `v0.3.0` attaches the packages and `SHA256SUMS`. A follow-up record checks the
  attached assets, as 0052 did.
- Packages are still not signed ([releasing](../development/releasing.md#not-yet-automated)).
