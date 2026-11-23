# 0016: CI only for `main`, and bringing stacked work into `main`

- **Date:** 2026-11-23
- **Branch / PR:** `ci/main-only`
- **Plan chunks:** 0.5 (follow-up)
- **Commits:** see the pull request

## What changed
- `ci.yml` runs for pull requests into `main` and pushes to `main` only (plus manual runs); pull requests into other
  branches no longer run CI.
- AGENTS.md and CONTRIBUTING: every pull request targets `main`; branches are not stacked on unmerged branches.
- This branch carries the work that missed `main`:
  - [0014](0014-move-design-into-docs.md) (#4) and [0015](0015-homebrew-and-one-file-providers.md) (#5), which were
    merged into their base branches instead of `main`.
  - `main` merged in (commit `2a008eb`), so the branch is up to date.

## Why
#4 and #5 were opened against the branches they were stacked on. #3 reached `main` first, then #4 and #5 were merged
into their (already merged) base branches, so the design move and the Homebrew source never reached `main`. Requested:
one pull request that brings everything into `main`, and CI only for `main`.

## How it was verified
- `git diff origin/main` on this branch contains the design move, the Homebrew source, the one-file provider
  refactor and this change: 40 files before this record.
- Workflow YAML parses; CI itself runs on this branch's pull request into `main`.

## Known limits and follow-ups
- The `main` ruleset ([repository settings](../development/repository-settings.md)) is not active yet: GitHub
  reports no rulesets, which is why pull requests could be merged without CI. Chunk 0.5 is done once it is created.
