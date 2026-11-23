# 0002: Workspace, CI and the OS-independent core crate

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main`
- **Plan chunks:** 0.1, 0.2, 1.1
- **Commits:** `54c9456`

## What changed
- Cargo workspace (edition 2024, MSRV 1.85), `rustfmt.toml`, GitHub Actions running fmt, clippy `-D warnings` and tests.
- `housekeeper-core`:
  - model: `Package`, `SourceInfo`, `SourceKind`, `PackageKind`, `InstallReason`, `Scope`, `SystemInfo`;
  - traits: `Source` (probe + inventory) and `Platform`;
  - `Host`: logical paths under a root, home, environment, symlinks followed inside the root;
  - `CommandRunner` / `SystemRunner`: argv only, `LC_ALL=C`, timeout with kill;
  - `Housekeeper::scan`: one thread per source; failures and panics reported per source;
  - `testing`: `FakeRunner` and `Fixture` (feature `test-support`).

## Why
Everything OS-specific plugs in through these traits (composition, decisions D12–D14), so later platforms and
capabilities never change the core.

## How it was verified
10 unit tests (runner timeout/kill, symlink resolution under the root, failing and panicking sources, selection).
A fresh clone of the pushed commit passed fmt, clippy and tests.

## Known limits and follow-ups
- The committed `Cargo.lock` already listed the not-yet-committed Linux crate; fixed by the next commit.
