# 0013: Block merging until every CI job is green

- **Date:** 2026-11-23
- **Branch / PR:** `ci/required-checks`
- **Plan chunks:** 0.5
- **Commits:** `b0aaabf`, `bf01f72`, plus the commit that added these hashes

## What changed
- New final CI job `ci-ok` in [ci.yml](../../.github/workflows/ci.yml): runs even when earlier jobs fail
  (`if: always()`) and succeeds only if lint, test, msrv, docs, dependencies and the whole `packages` workflow (both
  builds and every distribution test) succeeded. Failed, cancelled or skipped jobs make it fail.
- [development/repository-settings.md](../development/repository-settings.md): the `main` ruleset that requires
  `ci-ok`, with both the GitHub UI steps and a `gh api` command.
- CONTRIBUTING, AGENTS.md and the docs index describe the merge gate; plan chunk 0.5 added.

## Why
Requested: merging must be blocked unless every pipeline job is green. Requiring one aggregate check instead of the 29
matrix job names keeps the rule valid when distributions are added or removed.

## How it was verified
- Workflow YAML parses; `ci-ok` needs exactly the six top-level jobs.
- The gate's shell logic run locally: all `success` → green; any `failure`, `cancelled` or `skipped` → red.

## Known limits and follow-ups
- The ruleset is a GitHub setting, not code: a repository admin must create it (steps in
  [repository-settings.md](../development/repository-settings.md)) after this branch's CI has run `ci-ok` once.
- Chunk 0.5 is done once the ruleset is active.
