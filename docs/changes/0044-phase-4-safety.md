# 0044: Decide how Phase 4 checks a plan before carrying it out

- **Date:** 2026-11-26
- **Branch / PR:** `docs/phase-4-safety`, #34
- **Plan chunks:** 4.3, 4.4, 4.5, 6.1 and Phase 10 reworded; nothing built
- **Commits:** 113e401, 5ae9d72

## What changed
This change adds decisions only. It closes the last findings of the review that led to [0042](0042-leftover-matching.md)
and [0043](0043-leftover-sizes.md).

- **New decisions in the [architecture](../architecture/README.md#decisions):**
  - **D37: nothing from a reviewed plan is trusted when it is carried out.**
    - Each leftover records, when found, the identity of its folder (device and inode) and its owner.
    - Before trashing it, Housekeeper opens the path without following symlinks and checks that the folder is still a
      direct child of where it was found, with the same identity and owner.
    - Packages are checked again: the same version, still not protected, and simulated again.
    - The root helper repeats its own checks.
  - **D38: what is preselected.** Only caches with no `also_used_by` and no `unseen_copies`. Everything else is
    unticked, and shell lines are never edited before Phase 8.
  - **D39: scan cache.**
    - None before the desktop app, since a full CLI scan is under a second.
    - From Phase 6, the last scan may be shown and then refreshed, using per-source database fingerprints to tell
      which parts are stale.
    - Stored scans are display-only: plans, simulations, the helper and removals read the live system.
    - JSON and JSONL files under the XDG cache and state folders, not a database.
- **Plan updates:**
  - 4.3 also keeps a record of every item seen installed.
  - 4.4 and 4.5 cite D37 and D38.
  - 6.1 opens on the last scan (D39).
  - Phase 10 names leftovers of apps that are no longer installed, from that record.
- **Decision range:** D1–D39 in AGENTS.md, docs/README.md and the guide.

## Why
- **Review finding 5:** the plan is reviewed now and acted on later, so the folder or package can change in between.
- **Review finding 8:** leftovers of apps removed outside Housekeeper cannot be named, because nothing remembers what
  was installed.
- **The question of a local cache:** a cache would not fix any of the matching findings, and stale data would make
  finding 5 worse. It earns its place only for the desktop app's start-up, for display.

Deciding these before 4.1 keeps the helper and the Trash code from being written against assumptions that would need
undoing.

## How it was verified
- Docs only: no code changed, so the Rust checks were not re-run for this change.
- Checked that every decision cited in the new rows exists in the architecture table (D7, D25, D32, D33), and that
  the plan rows cite D37–D39.

## Known limits and follow-ups
- **Design only.** The checks are built in 4.4 (Trash) and 4.5 (removal); the items-seen record in 4.3; the cached
  scan in 6.1.
- **Rename or move between plan and execution.** D37 skips a folder that was renamed or moved after the plan was made,
  rather than following it.
