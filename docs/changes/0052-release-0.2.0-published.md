# 0052: Release 0.2.0 published and checked

- **Date:** 2026-11-28
- **Branch / PR:** `docs/release-0.2.0-done`, #42
- **Plan chunks:** none (release of the work prepared in 0050)
- **Commits:** (filled in below)

## What changed
- This record only; no code or other document changed.

## Why
The owner published [v0.2.0](https://github.com/mhsiddiqui/housekeeper/releases/tag/v0.2.0), the release 0050
prepared. It took two attempts:

- **First attempt (2026-11-27 18:47 UTC).** The tag was created on the tip of `main`, `767661b`. That commit already
  held chunk 5.1 (`housekeeper inspect`, change 0051), which `CHANGELOG.md` lists under **Unreleased**, not 0.2.0. The
  Release workflow ([46052005708](https://github.com/mhsiddiqui/housekeeper/actions/runs/46052005708)) succeeded, so
  the attached binaries had a command the release notes did not mention.
- **Fix.** The owner chose to release again from the commit 0050 prepared, rather than move `inspect` into 0.2.0's
  notes. The `v0.2.0` tag was deleted and created again on `64bcd05` (the merge of #40). The first release, which
  GitHub had turned into a draft when its tag went away, was deleted with its assets.
- **Second attempt (2026-11-27 18:55 UTC).** Published from the existing tag on `64bcd05`.

Anyone who downloaded a 0.2.0 package during the roughly eight minutes the first release was public has a build that
also has `housekeeper inspect`. It is the same code, tested on every distribution in #41's CI, and it will be in the
next release.

[releasing](../development/releasing.md) already says the tag is created on `main`. It does not say which commit:
the tag must go on the release preparation's merge commit when later work has been merged since.

## How it was verified
- The tag `v0.2.0` points to `64bcd05`, where `Cargo.toml` says `version = "0.2.0"`. The release is the latest, not a
  draft or pre-release, and its notes do not mention `inspect`. No other `v0.2.0` release exists.
- The Release workflow run for it
  ([46053020905](https://github.com/mhsiddiqui/housekeeper/actions/runs/46053020905), commit `64bcd05`) completed
  with success.
- The release has nine assets: `.deb` (amd64, arm64), `.rpm` (x86_64, aarch64), Arch `.pkg.tar.zst` (x86_64,
  aarch64), tarballs (x86_64, aarch64) and `SHA256SUMS`. All eight packages were downloaded and
  `sha256sum -c SHA256SUMS` reported OK for each.
- The x86_64 tarball holds `housekeeper`, `housekeeper-helper`, the polkit policy, `LICENSE` and `README.md`. The
  binary is a static-pie x86-64 ELF, prints `housekeeper 0.2.0`, rejects `inspect` as an unknown subcommand, and
  `housekeeper list` ran on this Ubuntu 26.04 machine. The aarch64 tarball's binary is a static ARM aarch64 ELF.
- `housekeeper_0.2.0-1_amd64.deb` is package `housekeeper`, version `0.2.0-1`, architecture `amd64`. It installs
  `/usr/bin/housekeeper`, `/usr/lib/housekeeper/housekeeper-helper`, the polkit action, the README and the licence.

## Known limits and follow-ups
- [releasing](../development/releasing.md) step 3 should say to tag the release preparation's merge commit, not
  whatever `main` is at publishing time, and to create the tag with `git tag` before drafting the release. That is a
  small docs change.
- Packages are still not signed ([releasing](../development/releasing.md#not-yet-automated)).
- The README's "Privacy and safety" section still says Housekeeper never changes the system, which has not been true
  since `housekeeper remove` (0049).
