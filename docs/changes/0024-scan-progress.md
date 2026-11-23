# 0024: Report each source's result as soon as it finishes

- **Date:** 2026-11-23
- **Branch / PR:** `feat/scan-progress`
- **Plan chunks:** none (prepares 6.3, see [plan](../planning/plan.md))
- **Commits:** ba8cc0f, 3cf5011

## What changed
- `crates/core/src/inventory.rs`: new `Housekeeper::scan_with(selection, on_report)`. It runs the same parallel scan
  as `scan` and calls `on_report(&SourceReport, &[Package])` once per selected source, from that source's scanning
  thread, as soon as the source finishes. It is called for every outcome: ok, unavailable, failed, and a source that
  panicked (reported as failed). The returned `Scan` is the same as `scan` returns.
- `scan` is now `scan_with(selection, |_, _| {})`, so there is one code path.
- If the callback itself panics, that source is reported as failed with no packages, like a panicking source; the
  other sources are unaffected. The doc comment says so, and that calls come concurrently in completion order.
- One test: the callback sees each selected source exactly once (including a failing, a panicking and an unavailable
  one, and not an unselected one), with the same report and packages as the final scan.
- [Architecture](../architecture/README.md) D12 and the scan walkthrough in the
  [guide](../architecture/guide.html#scan) mention `scan_with`.

## Why
`scan` returns only when every source has finished. The desktop app (Phase 6) shows scan progress per source on the
first-run screen, so it needs each result as it arrives. No decision changed; D12 and D13 still hold (threads, no
async runtime in core).

## How it was verified
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features` (all pass, including the new test) and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`: all clean.
- `cargo deny check` was not run: `cargo-deny` is not installed on this machine. No dependency was added.
- CLI output unchanged: `housekeeper list` and `housekeeper list --json` (with `elapsed_ms` removed) on this machine
  gave identical output before and after the change.

## Known limits and follow-ups
- Nothing calls `scan_with` yet; the CLI keeps using `scan`. The desktop app's first-run screen (Phase 6) will use it.
- There is no "source started" event; the app can list the selected sources from `Housekeeper::sources` before the
  scan and mark each one done when its report arrives.
