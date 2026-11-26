# 0043: Measure leftovers only in `remove --dry-run`, within a budget

- **Date:** 2026-11-26
- **Branch / PR:** `fix/remove-dry-run`, #33
- **Plan chunks:** 3.6 (b), done; 3.6 and Phase 3 done
- **Commits:** 08c958c, f3ce7c6, 9796160

## What changed
The second half of the review of [0041](0041-leftovers.md), after [0042](0042-leftover-matching.md).

- **`show` stops measuring leftovers.**
  - `Housekeeper::show` finds leftover folders but no longer walks them. `Leftover::size_bytes` is now
    `Option<u64>`: `None` (JSON `null`) until measured. Unmeasured folders are sorted by path.
  - The CLI's `Leftovers:` line counts folders and shell lines without a size, for example "2 folders (`housekeeper
    remove --dry-run` measures and lists them)".
- **`remove --dry-run` measures them, within a budget.**
  - New `Housekeeper::measure_leftovers(&mut Leftovers, budget)`, which sorts the folders largest first. Its budget is
    the number of files and folders looked at per folder; the CLI passes the new `MEASURE_BUDGET` (50,000).
  - New `Leftover::size_complete`, false when measuring stopped at the budget. The CLI then prints "at least 648.0 MB:
    too many files to count them all".
  - The budget counts entries rather than time, so results and tests are repeatable.
- **Exit code.** `remove` without `--dry-run` exits with 3 instead of 2, which already meant "nothing matches" (and is
  clap's code for a usage error). `scripts/test-package.sh` now checks for 3 exactly.
- **Docs:** README (the `show` Leftovers bullet, exit code 3, the size budget), CHANGELOG (Changed), the architecture's
  removal paragraph, and the plan (3.6 ✅, Phase 3 ✅).

## Why
The review's findings 1 and 6.

- **Finding 1, speed.** Every `show` walked every leftover folder in full. For the uv binary, `~/.cache/uv` holds
  167,294 entries, and `show` took 1.1 s in a release build (7.7 s in a debug build). A game library under
  `~/.var/app` would take longer.
- **Finding 6, exit codes.** Scripts could not tell "not available yet" from "no such item".

## How it was verified
- **New and changed tests:**
  - core: `show` leaves sizes unmeasured, and `measure_leftovers` gives exact sizes, largest first;
  - core: with a budget of 2 entries, a folder of 3 files reports 3 bytes and incomplete, and with 3 reports 6 bytes
    and complete;
  - CLI: the `show` line has no size, and the dry run prints "at least … too many files to count them all".
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (8 + 35 + 148)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - `cargo check --target x86_64-pc-windows-gnu -p housekeeper-core -p housekeeper-platform`
  - `cargo deny` was not run (not installed); no dependency was added.
- **Release builds on this Ubuntu 26.04 machine, for `~/.local/bin/uv`, two runs each:**

  | Command | `main` at 50f98ab (before 0042) | This branch |
  |---|---|---|
  | `show` | 1.13 s, 1.15 s | 0.26 s, 0.29 s |
  | `remove --dry-run` | 1.21 s, 1.26 s | 0.56 s, 0.56 s |

  - The dry run printed `~/.cache/uv (cache, at least 648.0 MB: too many files to count them all)` and
    `~/.config/uv (settings, 327 B; …)`. `show` printed `Leftovers:  2 folders (…)`.
  - `remove` without `--dry-run` exited with 3.
- **Packaging:** `sh -n scripts/test-package.sh` passed. The package test itself was not run locally (nfpm is not
  installed); CI runs it on every distribution.

## Known limits and follow-ups
- **Partial sizes can be far off.** The partial size of a big folder can be far below the real one: 648 MB counted,
  where the full walk gave 1.8 GB. The text says "at least".
- **Two lookups per file.** Measuring still calls `stat` per file through `Host::metadata` after `read_dir`'s own
  lookup. Folding the size into `Entry` would halve that; it was left out to keep `Entry` unchanged.
- **Cached sizes.** Caching sizes for a storage view is Phase 10, under the decision taken in the next change (D39).
