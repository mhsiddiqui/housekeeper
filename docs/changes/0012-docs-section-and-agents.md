# 0012: Documentation section, change records and AGENTS.md

- **Date:** 2026-11-23
- **Branch / PR:** `docs/docs-section` (stacked on #1)
- **Plan chunks:** none (documentation and workflow)
- **Commits:** fill in before merging

## What changed
- All documentation moved under `docs/`, grouped by section, with an index ([docs/README.md](../README.md)):
  - `docs/SPEC.md` → `docs/product/spec.md`
  - `docs/ARCHITECTURE.md` → `docs/architecture/README.md`
  - `docs/architecture.html` → `docs/architecture/guide.html`
  - `docs/PLAN.md` → `docs/planning/plan.md`
  - `docs/RELEASING.md` → `docs/development/releasing.md`
- Root keeps the files tools look for there: `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `AGENTS.md`, `LICENSE`.
- New `docs/changes/`: a template, an index and records 0001–0012 covering every change so far.
- New `AGENTS.md`: how AI agents work in this repository, based on the docs section; every branch must add a change
  record.
- Links updated in README, CONTRIBUTING, the plan, spec, releasing guide, architecture guide, `release.yml` and the
  core crate docs.

## Why
Requested: one documentation section, and an agent guide that keeps a track record of each change.

## How it was verified
Every relative link in the Markdown and HTML documents resolves to an existing file (checked with a script). Code
still builds and tests pass (only a doc comment changed in code).

## Known limits and follow-ups
- Records 0001–0009 describe work committed directly to `main` before the branch-per-task rule.
