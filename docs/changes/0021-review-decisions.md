# 0021: Decisions for package tools, removal privileges and script-installer recipes

- **Date:** 2026-11-23
- **Branch / PR:** `docs/review-decisions`
- **Plan chunks:** 3.1, 3.4, 4.1 (scope only; nothing is built yet)
- **Commits:** bfc3231

## What changed
- New decisions in [the architecture](../architecture/README.md#decisions):
  - **D24:** the native source reads the package database; changing packages belongs to a separate *package tool*
    (apt, dnf, zypper, rpm-ostree, transactional-update, pacman, an AUR helper), one file each, chosen like the native
    source. The native source's `remover()` and later abilities come from the chosen tool.
  - **D25:** every removal step says who runs it. User-scope items are removed as the user; snapd and Flatpak's
    system helper authorise themselves; the root helper takes only native package tool operations and recipe actions
    on system paths, checks names against the package database itself, and never runs a scan.
  - **D26:** a script-installed tool is one TOML recipe holding both its detection and its removal; the manual
    source's `INSTALLERS` table moves into recipes in Phase 3.4.
- Amended **D3** (openSUSE Aeon/MicroOS use `transactional-update`, handled by their own package tool) and **D4**
  (the helper checks names against the package database, not "the inventory", see D25).
- [Plan](../planning/plan.md): chunks 3.1, 3.4 and 4.1 name these decisions.
- Decision count D1–D26 in AGENTS.md, `docs/README.md` and the guide.

## Why
Architecture review findings H2, M1 and M4:
- **H2:** `RpmSource` serves Fedora, openSUSE and Silverblue. Removal and updates go through dnf, zypper and
  rpm-ostree, so putting them in the database file would make it branch on distribution, and a distribution sharing
  a database but not a tool could no longer be added as one file.
- **M1:** D4 said the helper "validates names against the inventory". Doing that as root would scan with root's home
  and environment, so user-scope sources would read the wrong places, and the helper would carry the whole scanner.
- **M4:** detection of script installers (Rust table in `manual.rs`) and their removal (TOML recipes, D8) would
  describe the same tool twice and drift apart.

They are decided now because Phase 3.1 and 3.4 are built on them.

## How it was verified
- Documentation only. Read the new rows against D2, D3, D4, D8 and D20 for contradictions; the links to
  `../architecture/README.md#decisions` and `../planning/plan.md` resolve.

## Known limits and follow-ups
- Nothing is implemented: package tools (D24) and the per-step runner (D25) arrive with Phase 3.1, recipe-based
  detection (D26) with Phase 3.4.
- How openSUSE's transactional systems are detected (read-only root plus `transactional-update`) is left to the
  transactional-update tool file.
