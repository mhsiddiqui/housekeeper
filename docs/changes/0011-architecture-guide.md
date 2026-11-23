# 0011: Illustrated architecture guide

- **Date:** 2026-11-23
- **Branch / PR:** written on `docs/architecture-explainer`, carried into `docs/docs-section` (see [0012](0012-docs-section-and-agents.md))
- **Plan chunks:** none (documentation)
- **Commits:** `b1c8a29`

## What changed
- [architecture/guide.html](../architecture/guide.html): layers, composition instead of inheritance, the
  `Source`/`Platform`/`Host` parts, how `LinuxPlatform` assembles sources, the scan flow, optional capabilities,
  supporting a new distribution (relatives, new package managers with an Alpine walkthrough, NixOS-like systems) and
  a new operating system, testing, and current limits.

## Why
Explain, with diagrams, how the composition-based design lets Housekeeper add distributions and operating systems
without changing working code.

## How it was verified
Rendered in headless Chromium in light and dark themes: no console errors, diagrams laid out correctly.

## Known limits and follow-ups
- States the `NativeManager` limit and its planned fix in chunk 2.3.
