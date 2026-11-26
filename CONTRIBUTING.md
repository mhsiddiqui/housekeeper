# Contributing

## One branch per task
Every chunk in the [build plan](docs/planning/plan.md) is done on its own branch and merged through a pull request, so each
change can be tracked and reviewed on its own.

1. Start from an up-to-date `main`: `git switch main && git pull`.
2. Create a branch named after the task: `<type>/<short-description>`, for example
   `feat/desktop-entries`, `fix/snap-timeout`, `ci/release-pipeline`, `docs/install-guide`.
   Types: `feat`, `fix`, `refactor`, `test`, `ci`, `docs`, `chore`.
3. Commit in small steps with [Conventional Commits](https://www.conventionalcommits.org/) subjects
   (`feat(linux): …`, `ci: …`).
4. Update the documentation the change affects, in the same branch (the full table is in
   [docs/README.md](docs/README.md#which-document-to-update)):
   - [docs/changes/](docs/changes/README.md): a new change record. **Every branch has one.**
   - [docs/planning/plan.md](docs/planning/plan.md): chunk status.
   - [CHANGELOG.md](CHANGELOG.md): an entry under **Unreleased** when users would notice.
   - [README.md](README.md): when usage or installation changes.
   - [docs/architecture/README.md](docs/architecture/README.md): when a design decision is made or changed.
5. Push the branch and open a pull request **against `main`**, never against another branch. CI runs only for pull
   requests into `main` and pushes to `main`; a pull request into another branch is merged untested and does not reach
   `main`. Merging is blocked until the `ci-ok` check is green, which requires every CI job to pass
   ([repository settings](docs/development/repository-settings.md)). If your work needs another branch that is not
   merged yet, wait for it to land in `main` and start from there.

AI coding agents follow the same rules, summarised in [AGENTS.md](AGENTS.md).

## Licence of contributions
Housekeeper is licensed under [GPL-3.0-or-later](LICENSE). By submitting a contribution you agree that it is
licensed under the same terms. Do not copy code from projects under licences that are incompatible with the GPL.

## Checks
CI runs all of these on every pull request into `main` ([ci.yml](.github/workflows/ci.yml)). To run them locally:

```sh
cargo fmt --all --check
# cargo fmt cannot see these files (build.rs declares them):
rustfmt --check crates/platform/linux/src/sources/*.rs crates/platform/linux/src/tools/*.rs
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo deny check                       # https://github.com/EmbarkStudios/cargo-deny
```

### Packages on real distributions
CI builds the packages ([packages.yml](.github/workflows/packages.yml)) and, inside a container per distribution and
architecture, installs them with the native package manager, checks Housekeeper against the real package database,
and removes them again ([scripts/test-package.sh](scripts/test-package.sh)). To reproduce one locally:

```sh
rustup target add x86_64-unknown-linux-musl   # once
scripts/package.sh                             # needs nfpm: https://nfpm.goreleaser.com
docker run --rm -v "$PWD/dist:/dist:ro" -v "$PWD/scripts:/scripts:ro" fedora:latest sh /scripts/test-package.sh /dist
```

## Code
- Read [docs/architecture/README.md](docs/architecture/README.md) first. A new way of installing software is one new
  file in `crates/platform/linux/src/sources/` implementing `Source` and `LinuxSource` and calling
  `register!(<Type>);`; it registers itself, nothing else changes ([how](docs/architecture/guide.html#new-provider)).
  Sources never import from each other; code they share lives outside `sources/`.
- Sources read the machine only through `Host` (logical paths, injected command runner), so every source is tested
  with fixture files and recorded command output.
- Tests should catch bugs a user would notice: wrong install reason, missed packages, crashes on unusual input.
