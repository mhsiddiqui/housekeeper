# 0040: Move the Linux crate into the platform folder

- **Date:** 2026-11-26
- **Branch / PR:** `refactor/platform-layout`, PR not opened yet
- **Plan chunks:** none (layout only)
- **Commits:** 6962212, 5f92021

## What changed
- **Move**: `crates/linux` is now `crates/platform/linux`, unchanged. The crate is still `housekeeper-linux`. Git
  records the 44 files as renames, and only the two doc comments below changed.
- **The layout now shows the structure**: the selector crate (`crates/platform`, `housekeeper-platform`) holds every
  operating-system crate it picks from. A future macOS crate goes in `crates/platform/macos`.
- **Workspace**:
  - The root `Cargo.toml` lists `crates/platform/linux` as a member, with a comment explaining why.
  - The `housekeeper-linux` dependency path now points to the new location.
  - `Cargo.lock` is unchanged; every check ran with `--locked`.
- **CI**: the `rustfmt --check` step uses the new path. It now also checks `src/tools/*.rs`: like the sources, these
  are declared by `build.rs`, so `cargo fmt` never saw them and CI had not been checking them. `rustfmt.toml`'s
  comment says the same.
- **Code comments**: `recipe.rs` and `manual.rs` now refer to "this crate's `recipes/`" rather than a full path, so
  they survive future moves.
- **Docs**: paths updated in AGENTS.md (overview, composition rule, test reference, checks), CONTRIBUTING.md,
  CHANGELOG (the Unreleased recipe entry) and plan 1.9.
  - Architecture: the layer diagram and crate list, the "new OS" paragraph, the provider, tool and recipe sections,
    and D20, D24 and D34.
  - The guide: code captions, the "Supporting another operating system" section (now also says to add the workspace
    member) and the file table.
  - New decision D35; the decision range is now D1–D35.
  - The documented `rustfmt` command now uses two globs instead of `{sources,tools}`, so it works in POSIX `sh`.
- **Not changed**: older change records keep the old paths, because they describe history.

## Why
The user asked for it: the selector should visibly hold the platforms it chooses between, so the place for another
operating system is obvious. D35 records two choices:

- **Each OS stays a separate crate rather than a module of the selector.** This keeps D27's target-specific
  dependencies, keeps one build script per OS, and keeps core free of OS knowledge.
- **The workspace lists nested OS crates by name.** A `crates/platform/*` pattern would also match `src/`.

## How it was verified
- **Workspace**: `cargo metadata` lists `housekeeper-linux` at `crates/platform/linux/Cargo.toml`.
- **Local checks**, all passed:
  - `cargo fmt --all --check`
  - `rustfmt --check crates/platform/linux/src/sources/*.rs crates/platform/linux/src/tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (7 + 28 + 147, the same counts as before)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check --workspace --all-targets --all-features --locked`
  - `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu`
  - `cargo check -p housekeeper-core -p housekeeper-platform -p housekeeper-cli --target aarch64-apple-darwin`
- **Leftover paths**: `grep -rn crates/linux`, excluding older change records, finds only D35, which names the old
  layout on purpose.
- **Packaging**: `scripts/package.sh` and `scripts/test-package.sh dist` passed in Debian stable, Ubuntu 24.04,
  Fedora, AlmaLinux 9, openSUSE Leap, openSUSE Tumbleweed and Arch Linux (x86_64).
- **Not run**: `cargo deny`, which is not installed. No dependency changed.

## Known limits and follow-ups
- The cross-platform sources (Homebrew, npm, pipx, uv, pip, cargo) still live in the Linux crate. A macOS platform
  will need them moved to a shared crate first; the guide's "Supporting another operating system" section says so.
