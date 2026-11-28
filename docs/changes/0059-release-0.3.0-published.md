# 0059: Release 0.3.0 published and checked

- **Date:** 2026-11-29
- **Branch / PR:** `docs/release-0.3.0-done`, #49
- **Plan chunks:** none (release of the work prepared in 0058)
- **Commits:** bbc18ff

## What changed
- This record only; no code or other document changed.

## Why
The owner published [v0.3.0](https://github.com/mhsiddiqui/housekeeper/releases/tag/v0.3.0) (2026-11-28 19:45 UTC),
which 0058 prepared. This time it took one attempt. As 0052 asked, the annotated tag `v0.3.0` was created on the merge
of #48 (`4883765`) and pushed before the release was drafted, and the release was published from that existing tag.

## How it was verified
- The release is not a draft or a prerelease, and its tag resolves to `4883765`, whose `Cargo.toml` says
  `version = "0.3.0"`.
- The Release workflow ([46182861250](https://github.com/mhsiddiqui/housekeeper/actions/runs/46182861250)) ran on
  `4883765` and succeeded. Every job passed: the tag check, both builds, the package tests on every supported
  distribution and processor, and the upload.
- Nine assets are attached:
  - `.deb`, `.rpm`, Arch `.pkg.tar.zst` and the tarball, each for x86_64 and aarch64;
  - `SHA256SUMS`.
- All of them were downloaded. `sha256sum -c SHA256SUMS` reported OK for all eight packages.
- The x86_64 tarball's `housekeeper`:
  - is a static-pie x86-64 ELF;
  - `--version` printed `housekeeper 0.3.0`;
  - `install --help` describes the new command.
- `scripts/test-package.sh` on the downloaded files in `debian:stable` (x86_64) printed
  `OK: housekeeper_0.3.0-1_amd64.deb on Debian GNU/Linux 13 (trixie)`.

## Known limits and follow-ups
- Packages are still not signed ([releasing](../development/releasing.md#not-yet-automated)).
- An aarch64 Arch package is attached. Arch Linux is supported on x86_64 only (README), and the workflow tests Arch
  on x86_64 only, as in 0.2.0.
