# 0031: Release 0.1.0 published and checked

- **Date:** 2026-11-24
- **Branch / PR:** `docs/release-0.1.0-done`
- **Plan chunks:** 0.4 (releases), which closes Phase 0
- **Commits:** 7d8c3a4

## What changed
- [Plan](../planning/plan.md): chunk 0.4 and Phase 0 marked ✅, with a link to the release.

## Why
The owner published [v0.1.0](https://github.com/mhsiddiqui/housekeeper/releases/tag/v0.1.0) (tag on `main`, commit
`013cd8b`). That ran the one step of the release pipeline that pull requests never run: attaching the packages to the
release. It worked, so the chunk is done.

## How it was verified
- The Release workflow run for `v0.1.0`
  ([45400471088](https://github.com/mhsiddiqui/housekeeper/actions/runs/45400471088)) completed with success.
- The release has nine assets: `.deb` (amd64, arm64), `.rpm` (x86_64, aarch64), Arch `.pkg.tar.zst` (x86_64,
  aarch64), tarballs (x86_64, aarch64) and `SHA256SUMS`.
- All eight packages were downloaded and `sha256sum -c SHA256SUMS` reported OK for each.
- The x86_64 tarball contains `housekeeper`, `LICENSE` and `README.md`; the binary is a static-pie x86-64 ELF,
  prints `housekeeper 0.1.0`, and `housekeeper list` ran on this Ubuntu 26.04 machine.
- `housekeeper_0.1.0-1_amd64.deb` is package `housekeeper`, version `0.1.0-1`, architecture `amd64`, and installs
  `/usr/bin/housekeeper`, the README under `/usr/share/doc/housekeeper/` and the licence.
- The file names match the install commands in the README.

## Known limits and follow-ups
- The aarch64 Arch package is attached but not tested: Arch Linux has no official ARM image (Arch Linux ARM is a
  separate project), so CI excludes it.
- Packages are not signed yet ([releasing](../development/releasing.md#not-yet-automated)).
