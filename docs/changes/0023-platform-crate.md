# 0023: Build core on any OS and pick the platform in one crate

- **Date:** 2026-11-23
- **Branch / PR:** `refactor/platform-crate`
- **Plan chunks:** none (architecture review)
- **Commits:** 23899a7, 33650f9, 9fd9046

## What changed
- `crates/core/src/host.rs`: the execute-bit check is one function, `is_executable`, behind `cfg(unix)`. On other
  systems a file counts as executable when it is a file. `command.rs` uses the same function when it looks a program
  up on `PATH`. Linux behaviour is unchanged.
- `crates/core/src/model.rs`: the doc comments of `SystemInfo::family` and `SystemInfo::image_based` no longer assume
  Linux. Field names and the scan JSON (schema 1) are unchanged.
- New crate `crates/platform` (`housekeeper-platform`): `detect(&host) -> Result<Box<dyn Platform>, Unsupported>`.
  It depends on `housekeeper-linux` only on Linux (`[target.'cfg(target_os = "linux")'.dependencies]`); elsewhere
  `detect` returns `Unsupported` ("Housekeeper supports Linux only for now").
- `crates/cli`: calls `housekeeper_platform::detect`. Its two `cfg(target_os)` `platform` functions and its direct
  `housekeeper-linux` dependency are gone.
- Docs: decision D27; the layers in `docs/architecture/README.md` and `guide.html` (diagram, cards, composition root,
  "Supporting another operating system", where things live); the crate list and code rules in `AGENTS.md`;
  "D1–D21" is now "D1–D27". `scripts/package.sh`, `packaging/nfpm.yaml` and the workflows name no crate other than
  `housekeeper-cli`, so they did not change.

## Why
The architecture said core is OS-independent and that a new OS needs no change in core, but core used
`std::os::unix::fs::PermissionsExt` and did not build for Windows. The choice of platform was a `cfg(target_os)` pair
in the CLI that the desktop app would have had to copy. Now core builds anywhere and every front end picks the
platform through one function ([D27](../architecture/README.md#decisions)). The new crate has no new third-party
dependency (`thiserror` is already used by core).

## How it was verified
Run in the worktree with `CC=/usr/bin/cc CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/cc`:
- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: no warnings.
- `cargo test --workspace --all-features`: all passed (4, 10 and 97 tests in the three suites with tests), none
  failed.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`: no warnings.
- `cargo check -p housekeeper-core --target x86_64-pc-windows-gnu` and
  `cargo check -p housekeeper-platform --target x86_64-pc-windows-gnu`: both pass.
- `cargo run -p housekeeper-cli -- sources` on Ubuntu 26.04.1: same table as before (dpkg, Flatpak, Snap, … listed).
- `cargo deny check` was not run: `cargo-deny` is not installed on this machine. CI runs it.

## Known limits and follow-ups
- Only core and `housekeeper-platform` build for Windows. The CLI does too in principle, but it was not checked, and it
  would only print "Housekeeper supports Linux only for now".
- The non-Unix executable rule (every file counts) is deliberately coarse; a Windows platform would decide what is a
  program from the file extension when it exists.
