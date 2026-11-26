# 0047: Keep a history of changes and a record of items seen

- **Date:** 2026-11-26
- **Branch / PR:** `feat/history`, #37
- **Plan chunks:** 4.3, done
- **Commits:** 55aff98, ffa2cb9, 7d79637, e5dfde4

## What changed
- **New core module `history.rs`** (decisions D39, D42), working on two JSON Lines files in
  `$XDG_STATE_HOME/housekeeper`.
  - **`history.jsonl`** holds one `HistoryEntry` per change: when it ended, the `change` (`remove`), the item's key and
    title, every step with its `StepOutcome` (`done`, `failed` with a reason, `skipped` with a reason), and the overall
    `ChangeOutcome` (`done`, `failed`, `not-done`). `record` appends an entry and `history` reads them, skipping lines
    it cannot read.
  - **`seen.jsonl`** holds one `SeenItem` per item: key, source, name, title, AppStream IDs, launcher IDs, and
    `first_seen` and `last_seen`.
    - `remember(host, scan, now)` refreshes the items of sources the scan read without failure or warning.
    - Other items keep their dates. An item unseen for 365 days (`FORGET_AFTER_SECS`) is dropped.
    - The file is rewritten in one step.
  - `unix_now()` and `Housekeeper::history()` are new too.
- **`Housekeeper::scan` and `scan_with`** (used by `list` and `show`) update the record of items seen. A write that
  fails becomes a scan warning.
- **`Host` gains `state_home()`, `write_atomic()` and `append()`.** `write_atomic` writes a temporary file beside the
  target, then renames it; `append` adds the contents in one write. Both create the folder. The Linux leftover roots use
  `state_home()`.
- **`serde_json` is now a dependency of `housekeeper-core`.** It was already in the workspace (the CLI and the helper
  use it), so nothing new reaches `cargo deny`.
- **CLI `housekeeper history [--json]`.** It prints a table (UTC time, change, item, ID, outcome), or "Housekeeper has
  not changed anything on this computer yet." when the history is empty.
- **`scripts/test-package.sh`.**
  - It checks that a scan wrote `seen.jsonl` with the native packages, and that `history` is empty.
  - The leftover check now finds `~/.config/housekeeper` on any line: Housekeeper's own state folder is a leftover of
    the `housekeeper` package too, and may be the larger, listed first.
- **Docs:**
  - new decision D42, plus D1–D42 in AGENTS.md, docs/README.md and the guide;
  - AGENTS' "read-only until Phase 4" rule becomes "changes only through reviewed paths" (state files, or plan, queue
    and helper);
  - README (`history` in the usage block, a "What Housekeeper keeps" section), CHANGELOG (Added), and the plan
    (4.3 ✅).

## Why
Plan chunk 4.3.

- **A record of what was changed.** Removals (4.5) need a durable record of what was changed and how each step went:
  for the user, and for the desktop app's History view (6.2).
- **A record of what was installed.** Review finding 8 (0042): leftovers of an app removed outside Housekeeper cannot
  be named without remembering what was installed. Decision 0044 put that record in 4.3.
- **Why skip partial sources.** Only sources read completely refresh items, so a snapd outage or an unreadable
  database does not make every snap look removed.

## How it was verified
- **New tests:**
  - core `history.rs`:
    - a Flatpak app found on day 1 and gone on day 2 keeps `last_seen` day 1 with its AppStream and launcher IDs,
      while vim's moves to day 2;
    - the file is in the XDG state folder;
    - a failed source and a source with warnings leave their items' dates alone;
    - an item unseen for a year and a second is dropped;
    - the history reads back two entries around a broken line.
  - CLI: UTC formatting of the epoch, a 2026 date, a leap day and a negative time.
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (9 + 50 + 2 + 151)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - the Windows check of core and platform
  - `cargo deny` was not run (not installed); no new dependency entered the workspace.
- **On this Ubuntu 26.04 machine:**
  - `housekeeper history` printed the empty sentence, and `--json` printed `[]`.
  - After `housekeeper list`, `~/.local/state/housekeeper/seen.jsonl` had 2,179 lines (one per item found; 272 kB).
- **Packaging:** `scripts/package.sh` (nfpm from the `goreleaser/nfpm` image), then the updated
  `scripts/test-package.sh dist`, passed in Debian stable, Fedora latest, openSUSE Leap latest and Arch Linux (x86_64).

## Known limits and follow-ups
- **Nothing writes history entries yet.** `housekeeper remove` does in 4.5; `record` is ready for it.
- **Leftovers of removed apps.** The record of items seen is only written and read back; naming the leftovers of
  removed apps from it is Phase 10.
- **UTC times.** History times print in UTC: local time zones would need a time-zone database or a new dependency.
- **Concurrent scans.** Two scans finishing at the same moment both rewrite `seen.jsonl`; the later rename wins, and
  the next scan sets the dates right again.
