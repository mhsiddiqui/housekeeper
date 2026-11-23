# 0005: Flatpak and Snap sources

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main`
- **Plan chunks:** 1.4
- **Commits:** `2ed0617`

## What changed
- `FlatpakSource`: `flatpak list --columns=…` across system, user and custom installations; apps are explicit,
  runtimes are dependencies; branches stay separate; human sizes parsed.
- `SnapSource`: snapd REST API over `/run/snapd.socket` (HTTP/1.0, std only, 30 s timeout); publisher verification
  and channel in `origin`; app commands; install date from RFC 3339; bases and command-less content snaps are
  runtimes; snapd, kernel and gadget snaps are protected.

## Why
Universal formats are present on most desktops and are the main source of user-facing apps.

## How it was verified
Unit tests including a real Unix-socket server in a fixture. Counts match `snap list` (34) and `flatpak list` (10) on
Ubuntu 26.04.

## Known limits and follow-ups
- A first version of the snap socket test was flaky because the request was written in several pieces; fixed by
  writing it in one call (5 consecutive green runs).
