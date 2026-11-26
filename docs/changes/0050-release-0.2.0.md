# 0050: Prepare release 0.2.0

- **Date:** 2026-11-26
- **Branch / PR:** `chore/release-0.2.0`
- **Plan chunks:** none (release of Phases 1–4 work since 0.1.0; see [plan](../planning/plan.md))
- **Commits:** 66657f9

## What changed
- `Cargo.toml`: `version = "0.2.0"` under `[workspace.package]`; `Cargo.lock` updated by `cargo check`.
- `CHANGELOG.md`: the **Unreleased** section became **0.2.0 (2026-11-26)**, with a new empty **Unreleased** above it
  and a short summary at the top (Housekeeper can now remove what it lists).
  - Entries under **Changed** and **Fixed** that compared against states never released (`remove --dry-run`'s
    leftover choices and folder budget, `show` no longer measuring leftovers, dnf and zypper removals without online
    repositories, `show`'s "Needed by" on RPM systems, the `who` snap fix, the leftover hardening of 0042) were folded
    into the **Added** entries of the features they belong to, as 0030 did for 0.1.0.
  - Entries that change what 0.1.0 did stay under **Changed**: the default `list` hides components, `protected` is a
    reason string (JSON `schema` 2), and only the running kernel is protected.

## Why
Phase 4 is done (0049): Housekeeper now changes the machine, not only reads it. That is a minor version under
[Semantic Versioning](https://semver.org/) before 1.0, and the JSON `schema` changed too. Releasing before Phase 5
starts gives users the removal work on its own. Steps follow [releasing](../development/releasing.md).

## How it was verified
- `cargo fmt --all --check`, `rustfmt --check` on the sources and tools, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` and `cargo test --workspace --all-features`: all passed.
- `cargo run --bin housekeeper -- --version` printed `housekeeper 0.2.0`.
- The CI run of this pull request builds the static packages and tests them on every supported distribution, the same
  jobs the Release workflow runs before attaching anything.

## Known limits and follow-ups
- The owner publishes the GitHub release (tag `v0.2.0` on `main`); the Release workflow then attaches the packages
  and `SHA256SUMS`. A follow-up record checks the attached assets, as 0031 did.
- Packages are still not signed ([releasing](../development/releasing.md#not-yet-automated)).
