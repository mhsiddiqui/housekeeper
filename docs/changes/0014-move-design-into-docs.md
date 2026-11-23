# 0014: Move the design mockup into the docs section

- **Date:** 2026-11-23
- **Branch / PR:** `docs/move-design` (stacked on `ci/required-checks`)
- **Plan chunks:** none (documentation)
- **Commits:** see the pull request

## What changed
- `design/mockup/` → `docs/design/mockup/` (`index.html` and `store-snapshot.js` stay together).
- Links updated in README, the docs index and the plan. Record [0001](0001-product-spec-and-plan.md) keeps the old path
  because it describes the repository as it was then.

## Why
Requested: all documentation, including design material, lives in the docs section.

## How it was verified
The mockup has no relative links outside its folder. All relative links in the Markdown and HTML documents resolve
(checked with a script).

## Known limits and follow-ups
- None.
