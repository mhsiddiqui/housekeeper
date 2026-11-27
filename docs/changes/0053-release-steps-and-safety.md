# 0053: Tag the release commit; describe what Housekeeper changes

- **Date:** 2026-11-28
- **Branch / PR:** `docs/release-steps-and-safety`, #43
- **Plan chunks:** none (follow-ups of 0052)
- **Commits:** (filled in below)

## What changed
- [releasing](../development/releasing.md):
  - Note the release preparation's merge commit.
  - Tag that commit with `git tag -a` and push the tag, then draft the release from the existing tag. Never let GitHub
    create the tag on the tip of `main`.
  - Check the assets afterwards and record the release in a change record.
  - New paragraph: how to withdraw and republish a wrong release.
- README, "Privacy and safety": replaces "Read-only. This version never changes the system" and "No root … never asks
  for a password", both untrue since `housekeeper remove` (0049). It now says:
  - which commands only read, and that `remove` is the one that changes the system (plan, question, check again);
  - that root is used only through the helper and `pkexec`, with a password each time, and what the helper may do;
  - that Housekeeper deletes nothing itself: packages go through their own tool, leftovers and AppImages to the Trash;
  - which files Housekeeper writes for itself (`~/.local/state/housekeeper`);
  - that nothing leaves the machine, unchanged.

## Why
Both were left open by [0052](0052-release-0.2.0-published.md). 0.2.0 was first tagged on the tip of `main`, which
already held unreleased work. The README's safety section described 0.1.0.

## How it was verified
- Each README statement was checked against the code and records it describes: only `remove` changes the system
  (0049); the helper's operations (D40); leftovers and AppImages trashed (D43, 0036); state files (D42).
- The release steps are the ones followed for v0.2.0's second attempt (0052).
- Docs only; no build or test affected.

## Known limits and follow-ups
- None.
