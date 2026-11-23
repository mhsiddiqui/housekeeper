# 0025: Spec points to the architecture instead of its own outdated design

- **Date:** 2026-11-23
- **Branch / PR:** `docs/spec-architecture-link`
- **Plan chunks:** none
- **Commits:** 11e1fce

## What changed
- `docs/product/spec.md`, section "Architecture": removed the first-draft design (one `Source` trait with
  `install`, `remove`, `updates` and `owner` methods; backends named `Apt`, `Dnf`…; a helper paragraph; distribution
  through the openSUSE Build Service, AppImage and an AUR PKGBUILD). The section now links to
  [the architecture](../architecture/README.md) and keeps only the product reasons for two choices: why not
  PackageKit only (PackageKit as a fallback is now "not decided" instead of "plan"), and why Housekeeper itself is
  not a Flatpak (linking D16).

## Why
Architecture review finding L1. The spec's design contradicted the architecture: it showed every ability as a method
of `Source` (the code uses optional ability traits), planned a PackageKit fallback no decision covers, and named a
distribution channel D16 replaced with nfpm packages. AGENTS.md tells contributors and agents to read the docs first,
so a stale design there gets implemented.

## How it was verified
- Read the edited section next to `docs/architecture/README.md`; the link targets `../architecture/README.md` and its
  `#decisions` heading, both present.
- No code changed, so no build or test was run.

## Known limits and follow-ups
- The spec's roadmap item 7 still lists "PackageKit fallback for other distros" as a product idea; it needs a
  decision before any work on it.
