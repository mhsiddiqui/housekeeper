# 0022: Register sources with `register!` instead of scanning their text

- **Date:** 2026-11-23
- **Branch / PR:** `refactor/source-registration`
- **Plan chunks:** none (architecture review finding; no chunk status change)
- **Commits:** f6aee17, f20811f, a4c22f0

## What changed
- `crates/linux/build.rs` no longer reads source files. It lists the `*.rs` files in `src/sources/` (except `mod.rs`),
  declares one module per file and puts each file's `PROVIDER` into `PROVIDERS`. Each entry has a generated comment
  saying the file must contain `register!(<Type>);`. A file name that is not a valid Rust identifier stops the build
  with a clear message.
- `src/provider.rs` gains the `register!` macro, which defines `PROVIDER` for a source type. Every file in
  `src/sources/` calls it next to its `impl LinuxSource`.
- Sources no longer import from each other. `is_appimage`, `resolve_symlinks`, `scope_of` and `looks_like_version`
  moved from `sources/appimage.rs` to the new `src/files.rs`; `site_packages`, `split_dist_info` and `normalize_name`
  (with their test) moved from `sources/pip.rs` to the new `src/python.rs`. `version_line` stays in `sources/mod.rs`.
- D20 in `docs/architecture/README.md` is amended in place; the architecture guide, `AGENTS.md`, `CONTRIBUTING.md`
  and the module docs describe the new registration.

## Why
Finding `impl LinuxSource for ` in the text broke on a comment, string or test mock containing it, missed
`impl<T> LinuxSource for`, and the compiler could not check it. Now a file without `register!` fails to compile with
`cannot find value PROVIDER in module <file>`, next to the generated comment that says what to add. Imports between
sources (`manual.rs` from `appimage.rs`, `uv.rs` from `pip.rs`) meant deleting one source broke another.

## How it was verified
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features` (including
  `every_registered_non_native_source_is_present_exactly_once_in_display_order`) and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`: all pass.
- Removed `register!(NpmSource);` locally: `cargo check` failed with `cannot find value PROVIDER in module npm`.
- `cargo deny check` was not run: `cargo-deny` is not installed on this machine. No dependencies changed.

## Known limits and follow-ups
- File names in `src/sources/` must be valid Rust identifiers.
