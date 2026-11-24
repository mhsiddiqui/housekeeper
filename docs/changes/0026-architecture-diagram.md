# 0026: Redraw the architecture diagrams in the guide

- **Date:** 2026-11-24
- **Branch / PR:** `docs/architecture-diagram`
- **Plan chunks:** none
- **Commits:** eef607f

## What changed
- `docs/architecture/guide.html`, section 2 ("Four layers" → "The whole picture"): one diagram of the whole system as it
  is on `main` after changes 0019–0025: front ends, `housekeeper-platform`, the Linux crate (platform assembly,
  `Distro`, generated `PROVIDERS`, the thirteen sources by kind, shared code), core (`Housekeeper`, the traits,
  `ScanContext`, model, `Host`, `CommandRunner`) and the machine. Decided but unbuilt parts are dashed: the desktop
  app, the root helper (D25), package tools (D24), recipe-based installers (D26), other operating systems.
- Section 6 ("What happens during a scan"): the sequence now starts at `housekeeper_platform::detect`, uses
  `scan_with`, passes `ScanContext` to `inventory`, shows the manual source consulting the others and `on_report`
  firing as each source finishes.
- Four small SVG style classes in the guide's stylesheet (`head`, `tiny`, `platform`, `lane`).

## Why
The previous diagrams predated `ScanContext`, `register!`, the platform crate and decisions D22–D27, so the guide's
pictures no longer matched the code.

## How it was verified
- Rendered the page with headless Chromium and checked both diagrams in the screenshots; fixed overlapping labels and
  text running past box edges.
- Reviewed in a browser by the owner before merging.

## Known limits and follow-ups
- The Layers diagram in `docs/architecture/README.md` already matches (crate level only) and was not changed.
