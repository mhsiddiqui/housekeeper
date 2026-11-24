# 0029: CI status badge in the README

- **Date:** 2026-11-24
- **Branch / PR:** `docs/ci-badge`
- **Plan chunks:** none
- **Commits:** f795d2e

## What changed
- `README.md`: a badge under the title showing the status of the CI workflow (`.github/workflows/ci.yml`) on `main`,
  linking to its runs.

## Why
Shows at a glance whether `main` builds and passes its checks, before the first release is published.

## How it was verified
- The badge URL returns an SVG (HTTP 200, `image/svg+xml`) titled "CI - passing".

## Known limits and follow-ups
- The badge shows the latest CI run on the default branch; the Release workflow has no badge.
