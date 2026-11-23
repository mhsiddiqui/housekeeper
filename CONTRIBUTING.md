# Contributing

## One branch per task
Every chunk in the [build plan](docs/PLAN.md) is done on its own branch and merged through a pull request, so each
change can be tracked and reviewed on its own.

1. Start from an up-to-date `main`: `git switch main && git pull`.
2. Create a branch named after the task: `<type>/<short-description>`, for example
   `feat/desktop-entries`, `fix/snap-timeout`, `ci/release-pipeline`, `docs/install-guide`.
   Types: `feat`, `fix`, `refactor`, `test`, `ci`, `docs`, `chore`.
3. Commit in small steps with [Conventional Commits](https://www.conventionalcommits.org/) subjects
   (`feat(linux): …`, `ci: …`).
4. Update the documentation the change affects, in the same branch:
   - [docs/PLAN.md](docs/PLAN.md): chunk status.
   - [CHANGELOG.md](CHANGELOG.md): an entry under **Unreleased**.
   - [README.md](README.md): when usage or installation changes.
   - [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): when a design decision is made or changed.
5. Push the branch and open a pull request against `main`. Merge once CI is green.

## Checks
CI runs all of these on every pull request ([ci.yml](.github/workflows/ci.yml)). To run them locally:

```sh
cargo fmt --all --check
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
- Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) first. New ways of installing software are new `Source`
  implementations; nothing else needs to change.
- Sources read the machine only through `Host` (logical paths, injected command runner), so every source is tested
  with fixture files and recorded command output.
- Tests should catch bugs a user would notice: wrong install reason, missed packages, crashes on unusual input.
