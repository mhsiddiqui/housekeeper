# 0057: Refuse archives, scripts and installer programs, saying why

- **Date:** 2026-11-29
- **Branch / PR:** `feat/refuse-archives-scripts`, #47
- **Plan chunks:** 5.4 (see [plan](../planning/plan.md)); completes Phase 5
- **Commits:** filled in below before merging

## What changed
- **Scope, agreed with the owner:** the plan's choice for 5.4 is to run these files in the tracked mode (Phase 11) or
  refuse them with an explanation. The tracked terminal does not exist yet, so they are refused. Extracting archives
  into `~/.local/opt` with a manifest (the spec's table) was set aside: recording what an install changes is the
  tracked terminal's job.
- **Decision D49** (new); the architecture's installer-files section and the `FileInstaller` row say so too.
- **Linux:** the manual source (whose formats these are) offers a `FileInstaller`. Its plan has no steps and one blocker
  that explains why each format is not installed and what to do instead:
  - archives: unpack them yourself; what you put in `/opt`, `/usr/local/bin` or `~/.local/bin` is listed as installed
    by hand;
  - scripts: read them first; the tools Housekeeper's recipes know are named, since what such a script installs is
    found and removed by its recipe;
  - `.run`/`.bin` programs: look for their own uninstaller.
  - Every explanation ends with the tracked terminal (Phase 11).
- **Core:**
  - `InstallPlan::blocked`.
  - `Housekeeper::inspect` asks the source for its plan first. It scans the source and simulates only when the plan has
    steps (`install::plan` is now `install::check`, which takes the plan).
- **`scripts/test-package.sh`:** `housekeeper install --yes` of a script exits 3 with the explanation, and the script
  does not run.
- **Docs:**
  - architecture (D49, installer files, `FileInstaller` row) and D1–D49 in AGENTS.md, docs/README.md and the guide;
  - plan 5.4 ✅ and Phase 5 ✅;
  - CHANGELOG, and the README (status, "What is this file?", "Installing a downloaded file", roadmap).

## Why
Plan chunk 5.4. Unpacking an archive or running a script cannot be undone the way it was done: nothing records what
it changed (spec, removal tiers 5 and 6). Before this change, `inspect` said only "no install plan yet". Now it says
why these formats are not installed and what to do instead.

## How it was verified
- `cargo fmt --all --check`, `rustfmt --check` on sources and tools, and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --workspace --all-features`: all pass (core 69, Linux 176, CLI 8, helper 1). The new test inspects a
  `.tar.gz`, a script and a `.run` program on Ubuntu. Each gets a plan with no steps and a refusal; the script's names
  `rustup`, and every refusal mentions the tracked terminal. Only the file's own format is read: no command is run
  for these files.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps` and `cargo deny check`: clean.
- Packages built with `scripts/package.sh` (nfpm from its container image). `scripts/test-package.sh dist` passed in
  `debian:stable`, `fedora:latest` and `archlinux:latest`, including the new script refusal.
- By hand: `housekeeper inspect` of a two-line script printed the explanation under `Install:` and exited 1.

## Known limits and follow-ups
- Nothing installs archives, scripts or installer programs yet. The tracked terminal (Phase 11) is where they will be
  run or unpacked with what they change recorded.
- A script that installs a tool a recipe knows is not matched to that recipe before it runs. Its explanation names
  every tool that has a recipe.
