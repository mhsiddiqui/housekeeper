# 0003: Distro detection, Linux platform and dpkg source

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main`
- **Plan chunks:** 1.2
- **Commits:** `5f2f16c`

## What changed
- `distro.rs`: parses `/etc/os-release` (falls back to `/usr/lib/os-release`), family from `ID` then `ID_LIKE`, image-based
  detection via `/run/ostree-booted`.
- `LinuxPlatform`: composes exactly one native source per family (decision D2).
- `DpkgSource`: `dpkg-query --showformat`, apt's auto-installed marks per architecture, Essential/required packages
  protected, library sections classified. It is the reference implementation other sources copy.

## Why
The native package manager is the one part that differs per distribution; starting with dpkg proves the pattern on
the machine Housekeeper is developed on.

## How it was verified
12 unit tests. Real run on Ubuntu 26.04: 2,059 packages read in 50 ms.

## Known limits and follow-ups
- Ubuntu marks ~590 of its own packages as manually installed, so the default list is long. Desktop entries in
  chunk 2.1 will separate real apps.
