# 0027: Complete the README for the first release

- **Date:** 2026-11-24
- **Branch / PR:** `docs/readme`
- **Plan chunks:** 0.4 (first release)
- **Commits:** 5966086

## What changed
`README.md` now covers what a first-time user needs before v0.1.0 is published:
- **Why** Housekeeper exists and **what it finds**: every source by kind, and how each is read.
- **Install:** per-distribution commands, ARM file names, checksum check, the tarball steps (extract and
  `install -Dm755` into `~/.local/bin`), and uninstalling.
- **Use:** every option of `housekeeper list`, real output of `housekeeper sources` and `housekeeper list`, what the
  default filter hides, the JSON document and its `schema` field.
- **When a source is missing or fails:** what "not available" and the two kinds of warning mean.
- **Privacy and safety:** read-only, no root, no network or telemetry.
- **Supported distributions** as tested in CI, and how derivatives and unknown distributions are handled.
- **Build from source**, a short **roadmap**, and how to report a problem.

## Why
The README is the first thing a user of the first release sees (it is also shipped in every package under
`/usr/share/doc/housekeeper/`). It described installation and the commands only briefly and left out what is
detected, what the output means, what to do when a source fails, and what the program does and does not touch.

## How it was verified
- Every statement checked against the code on `main`: the sources and the folders they read, the default filter
  (`crates/cli/src/filter.rs`), the 60-second command timeout, the JSON fields, and the "not available" reasons.
- The `sources` and `list` examples are output of `housekeeper` 0.1.0 on this Ubuntu 26.04 machine.
- The tarball steps were run on a tarball laid out like `scripts/package.sh` makes it; the installed binary printed
  `housekeeper 0.1.0`.
- The distribution list matches the test matrix in `.github/workflows/packages.yml` (Arch and Silverblue x86_64 only).

## Known limits and follow-ups
- Release package file names in the install table are globs; they match the names in
  [releasing](../development/releasing.md) but were not checked against a published release, since there is none yet.
