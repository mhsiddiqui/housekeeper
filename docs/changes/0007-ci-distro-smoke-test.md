# 0007: Smoke-test the binary inside real distro containers

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main`
- **Plan chunks:** 1.8
- **Commits:** `4a25d0b`

## What changed
- CI builds the binary once (Ubuntu 22.04 for an old glibc) and runs it in `debian:stable`, `ubuntu:latest`,
  `fedora:latest`, `opensuse/tumbleweed` and `archlinux` containers.
- Each job checks that the native source reads more than 50 packages and that no source fails.

## Why
Package-manager output and database formats change between releases (decision D1); fixtures alone cannot catch that.

## How it was verified
GitHub Actions runs 6 and 7: all five containers green.

## Known limits and follow-ups
- Replaced by the package install/remove matrix in [0010](0010-test-and-release-pipeline.md).
