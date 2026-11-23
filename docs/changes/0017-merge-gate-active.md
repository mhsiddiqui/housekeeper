# 0017: Merge gate active on `main`

- **Date:** 2026-11-23
- **Branch / PR:** `docs/merge-gate-active`
- **Plan chunks:** 0.5 (done)
- **Commits:** see the pull request

## What changed
- The `main` ruleset was created on GitHub by the repository owner (a setting, not code).
- [repository-settings.md](../development/repository-settings.md) records that it is active and how it was checked;
  plan chunk 0.5 is marked done.

## Why
Requested: merging must be blocked unless every pipeline job is green ([0013](0013-merge-gate.md)).

## How it was verified
`GET /repos/mhsiddiqui/housekeeper/rules/branches/main` returns the rules `deletion`, `non_fast_forward`,
`pull_request` (0 approvals) and `required_status_checks` with `ci-ok` (integration 15368, GitHub Actions) and
`strict_required_status_checks_policy: true`. A first check found the status-check rule without any check listed; it
was completed and checked again.

## Known limits and follow-ups
- With 0 required approvals, the gate is CI only; reviews are not enforced.
