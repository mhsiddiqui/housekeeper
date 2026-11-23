# 0015: Homebrew on Linux, and providers that are one file

- **Date:** 2026-11-23
- **Branch / PR:** `feat/homebrew-source` (stacked on `docs/move-design`)
- **Plan chunks:** 1.9; 2.3 partly (the `OwnerLookup` capability)
- **Commits:** see the pull request

## What changed
- **Homebrew source** (`crates/linux/src/sources/homebrew.rs`): finds prefixes (`$HOMEBREW_PREFIX`,
  `/home/linuxbrew/.linuxbrew`, `~/.linuxbrew`) by their `bin/brew`; formulae from `Cellar/<name>/<version>` with
  the version `opt/<name>` links to (else the highest), install reason from `INSTALL_RECEIPT.json`
  (`installed_on_request` / `installed_as_dependency`), install time, tap as origin, `bin/` entries as commands;
  casks from `Caskroom` (skipping `.metadata`). Reads files only; `brew` is never run.
- **One-file providers** (decision D20):
  - `crates/linux/src/provider.rs`: the `LinuxSource` trait (`create`, optional `NATIVE` with `Family` and the
    `os-release` IDs it serves), `Context` (distribution and the chosen native source), `Provider`.
  - `crates/linux/build.rs`: finds `impl LinuxSource for <Type>` in every file of `src/sources/` and generates the
    module list and `PROVIDERS`; a file without exactly one such impl fails the build with a message.
  - `LinuxPlatform` picks the native source from `ID`, then `ID_LIKE`, against the declared families, creates it
    first, passes it to the others through `Context`, and sorts sources by kind. It names no source.
  - Removed the `Family` enum from `distro.rs` (it now only parses `os-release`) and the `NativeManager` enum.
- **`OwnerLookup` capability** in core: `Source::owner_lookup()` (default `None`); dpkg, rpm and pacman implement it
  themselves; `owner.rs` keeps only the shared batched-query plumbing; the manual source asks the native source it
  was given.
- `Platform::sources()` returns `Arc<dyn Source>` so the native source can be shared with the manual source.
- CI: new `test (homebrew, x86_64)` job in Homebrew's official image (`scripts/test-homebrew.sh`): installs `jq`
  with brew and checks `jq` is explicit and `oniguruma` a dependency.
- Docs: architecture overview and guide (new "Adding a provider: one file" section, shorter Alpine walkthrough, limits
  section), plan, AGENTS.md, CONTRIBUTING, README, CHANGELOG, package description.

## Why
Requested: Homebrew is supported on Linux, and adding another provider should mean implementing one type, nothing
else. Before this change a provider needed edits in `sources/mod.rs` and `lib.rs`, and a new distribution also in the
`Family` and `NativeManager` enums.

## How it was verified
- 111 tests pass on stable and on Rust 1.85; clippy `-D warnings` clean; rustfmt.
- Registration proven on a real build: a throwaway `sources/zz_example.rs` with one type and the two impls appeared in
  `housekeeper sources` with no other edit; a file without `impl LinuxSource for` stopped the build with
  `zz_example.rs: a file in src/sources/ must contain exactly one impl LinuxSource for <Type> (found 0)`.
- Homebrew unit tests: receipts in Homebrew 4 format, linked vs highest version, taps, casks, two prefixes with
  `HOMEBREW_PREFIX`, probe, broken receipt.
- On this machine (no Homebrew): `homebrew` reports "not available: Homebrew is not installed".
- Real Homebrew, locally in `homebrew/brew:latest` (Ubuntu 22.04): `brew install jq` installed jq 1.8.2 and its
  dependency oniguruma 6.9.10; Housekeeper listed jq as `explicit` / `app` and oniguruma as `dependency`
  (`scripts/test-homebrew.sh`). The first version of the script checked the JSON with the jq brew had just
  installed, which could not run on the image's older glibc; it now uses `grep`.

## Known limits and follow-ups
- Formula descriptions and sizes are not read (not in the receipt; a directory walk would be slow).
- The build script recognises the impl only when written as `impl LinuxSource for <Type>`.
- `housekeeper who` (rest of chunk 2.3) is still to do.
