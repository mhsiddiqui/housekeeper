# 0001: Add product spec, architecture, build plan and mockup

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main` (before the branch-per-task rule)
- **Plan chunks:** 0.1 (documents)
- **Commits:** `5cffc1a`

## What changed
- Product spec (now [product/spec.md](../product/spec.md)), renamed from "Linux Manager" to Housekeeper; uninstall
  recipes specified as TOML.
- Architecture document with decisions D1–D17 (now [architecture/README.md](../architecture/README.md)).
- Build plan with phases 0–12 split into chunks (now [planning/plan.md](../planning/plan.md)).
- Interactive HTML mockup and Flathub/Snap store snapshot under `design/mockup/`.

## Why
Agree on scope, the composition-based architecture and the order of work before writing code. Sign-in was removed
from v1 (D15) after discussing why tools like Atuin need accounts.

## How it was verified
Mockup opened in headless Chromium: every view rendered, no console errors.

## Known limits and follow-ups
- The mockup still says "Linux Manager" in its UI text.
