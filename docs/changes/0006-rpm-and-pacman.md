# 0006: rpm and pacman sources

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main`
- **Plan chunks:** 1.3
- **Commits:** `cf13613`

## What changed
- `RpmSource`: `rpm -qa --queryformat` with `LONGSIZE`; full NEVRA ids so parallel kernels stay apart; openSUSE reasons
  from `/var/lib/zypp/AutoInstalled`; Silverblue layered packages from `rpm-ostree status --json` (booted deployment);
  protected names from dnf's `protected.d` plus a minimal core list.
- `PacmanSource`: reads the local database (`DBPath` from `pacman.conf`) directly; `%REASON%` for explicit vs
  dependency; `pacman -Qmq` marks AUR/local builds; official kernels protected.
- `LinuxPlatform` picks rpm for the Fedora and SUSE families and pacman for Arch.

## Why
Covers the Fedora/RHEL, openSUSE, Arch and image-based families from the supported-distributions table.

## How it was verified
19 unit tests. Real containers: Fedora 45 (148 packages), openSUSE Tumbleweed (118, 27 explicit), Arch (137).

## Known limits and follow-ups
- On dnf systems the install reason is "unknown" (dnf's history database is not read yet).
- Not tested on a booted Silverblue system; layered-package detection is covered by fixtures only.
