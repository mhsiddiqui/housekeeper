# 0009: AppImage and manual-install sources

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main`
- **Plan chunks:** 1.6 (completes Phase 1)
- **Commits:** `87005fd`

## What changed
- `owner.rs`: one batched "which of these paths does a package own?" query per native manager (`dpkg-query --search`
  with glob escaping, `rpm -qf` file lists, `pacman -Qo`); only exact path matches count.
- `AppImageSource`: type 1/2 magic bytes in the usual folders; name and version from the file name.
- `ManualSource`: `/opt` folders and loose executables not owned by the native manager; symlink farms grouped by install
  root (e.g. `~/Softwares/pgsql18-home`); links into pipx, uv, cargo, nvm, fnm, snap and flatpak directories and pip
  console scripts left to their own sources; known script installers (rustup, nvm, pyenv, atuin, bun, deno, SDKMAN,
  Homebrew, Volta, oh-my-zsh) listed as one item each.

## Why
Script installs and hand-copied software are what other tools miss entirely.

## How it was verified
102 workspace tests. Ubuntu 26.04: 37 manual items in 0.13 s; `.deb`-owned `/opt` folders (Brave, Zoom…) excluded.
Fedora, Tumbleweed and Arch containers: an unowned `/opt` app and `/usr/local/bin` script found, a link to a system
binary skipped, no failures; real `rpm -qf` and `pacman -Qo` output matched the parser.

## Known limits and follow-ups
- The native manager is a closed `NativeManager` enum; chunk 2.3 replaces it with the `OwnerLookup` capability so a
  new distribution touches fewer files (see the [architecture guide](../architecture/guide.html#limits)).
