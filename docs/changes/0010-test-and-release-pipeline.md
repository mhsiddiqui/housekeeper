# 0010: Full test pipeline and release packages

- **Date:** 2026-11-23
- **Branch / PR:** `ci/test-and-release-pipeline`, #1
- **Plan chunks:** 0.3, 0.4
- **Commits:** `1fae06a`, `5dddc9b`, `8242db8`, `7bba07c`, `c4c7fe2`, `c0ca133`

## What changed
- `scripts/package.sh` + `packaging/nfpm.yaml`: static musl binary; `.deb`, `.rpm`, Arch `.pkg.tar.zst` and tarball for
  x86_64 or aarch64, with `SHA256SUMS`.
- `scripts/test-package.sh`: installs the right package with the native manager (without contacting any repository),
  checks the native source against the real database, checks Housekeeper lists its own package and that no source
  fails, then removes it; tarball mode on image-based systems.
- `ci.yml`: lint, tests, MSRV 1.85, rustdoc `-D warnings`, `cargo deny` (`deny.toml`), package matrix.
- `packages.yml` (reusable): builds on native x86_64 and arm64 runners; tests on Debian 12/13, Ubuntu 22.04/24.04/latest,
  Fedora, AlmaLinux 9/10, openSUSE Leap and Tumbleweed (both architectures), Arch (x86_64) and the Fedora Silverblue 45
  image.
- `release.yml`: on a published release, checks the tag against `Cargo.toml`, builds, tests and attaches the packages.
- Docs: `CONTRIBUTING.md`, releasing guide, `CHANGELOG.md`, README install table, decisions D16, D18, D19.

## Why
Requested: a complete test pipeline, packages on every release, and tests on every supported OS (decision D19).

## How it was verified
- Locally: packages built; install/run/remove passed in Debian 12 and 13, Ubuntu 22.04, 24.04 and 26.04, Fedora 45,
  AlmaLinux 9 and 10, openSUSE Leap 16.1 and Tumbleweed, Arch; image-based mode in Fedora 45; MSRV 1.85 test run.
- CI: first run failed. apt read the relative `dist/x.deb` as a package name; the Silverblue image had 257 layers,
  more than Docker can stack; openSUSE Leap 15.6 is end of life with an expired key. All three fixed. The final run
  (Actions run 45169779352) passed 29/29 jobs.

## Known limits and follow-ups
- The release upload step runs for the first time at release v0.1.0.
- Packages are not signed; nothing is published to the AUR, COPR or an apt repository.
