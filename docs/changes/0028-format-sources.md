# 0028: Format and check the source files cargo fmt never reached

- **Date:** 2026-11-24
- **Branch / PR:** `ci/format-sources`
- **Plan chunks:** none
- **Commits:** 66fb03d, a76b305

## What changed
- All of `crates/linux/src/sources/*.rs` formatted with rustfmt. The most visible fix: the `inventory` functions
  changed in 0020 had their first statement on the signature line and their bodies under-indented.
- `rustfmt.toml` sets `edition = "2024"`, so plain `rustfmt` formats these files the way `cargo fmt` formats the rest.
- CI (`lint` job) runs `rustfmt --check crates/linux/src/sources/*.rs` after `cargo fmt --all --check`; the same
  command is in the check lists of AGENTS.md and CONTRIBUTING.md.

## Why
`build.rs` declares the source modules in a generated file that `src/sources/mod.rs` pulls in with `include!`.
`cargo fmt` only follows `mod` declarations it can see in the source tree, so it never formatted or checked any file
in `src/sources/`, and `cargo fmt --all --check` passed while those files were unformatted. Found while writing the
README (0027).

## How it was verified
- Before the change, `rustfmt --check crates/linux/src/sources/uv.rs` reported a diff while `cargo fmt --all --check`
  passed. After it, `rustfmt --check crates/linux/src/sources/*.rs` and `cargo fmt --all --check` both pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo test --workspace --all-features`
  pass; formatting only, no behaviour change.

## Known limits and follow-ups
- A new file under `src/sources/` is covered by the glob automatically; a new folder of generated modules (such as the
  package tools of D24) needs the same line in CI.
