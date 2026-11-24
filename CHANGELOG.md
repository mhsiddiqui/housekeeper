# Changelog

All notable changes to Housekeeper. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project uses [Semantic Versioning](https://semver.org/).

## Unreleased

### Added
- `housekeeper who <command or path>`: which source and package a command or file comes from, following its
  symlinks, with the evidence (the package manager lists it, it is inside a package's install folder, or inside a
  folder a source installs into) and the other copies hidden behind it on the `PATH`. `--json` for scripts; exits 0
  when an owner is found, 1 when nothing accounts for the file, 2 when there is no such command or file.

## 0.1.0 (2026-11-24)

First release. Housekeeper lists every app installed on a Linux machine, whatever installed it. It only reads; it
changes nothing.

### Added
- `housekeeper sources` and `housekeeper list` (`--all`, `--source`, `--search`, `--json` with schema version 1).
- Read-only inventory from every source, scanned in parallel; a source that fails is reported with a warning and the
  rest are still listed:
  - native packages: dpkg (with apt's auto-installed marks), rpm (zypp auto-installed marks, rpm-ostree layered
    packages, dnf protected packages), pacman (local database, AUR/foreign packages);
  - Flatpak (system, user and custom installations), Snap (snapd API) and Homebrew on Linux (formulae with
    explicit/dependency reasons and taps, casks);
  - AppImage files in the usual folders;
  - npm globals (system Node, npm prefixes, every nvm and fnm version), pipx, `uv tool`, `pip install --user`,
    `cargo install`;
  - installs by hand or script: `/opt`, `/usr/local/bin`, `~/.local/bin`, symlink farms grouped by install folder,
    and well-known script installers (rustup, nvm, pyenv, atuin, Bun, Deno, SDKMAN!, Homebrew, Volta, Oh My Zsh),
    leaving out anything another source or the package manager already owns.
- Packages: `.deb`, `.rpm`, Arch package and tarball for x86_64 and aarch64, one static binary for every distribution,
  tested by installing, running and removing them on Debian, Ubuntu, Fedora, AlmaLinux, openSUSE, Arch Linux and
  Fedora Silverblue.
- Licensed under GPL-3.0-or-later.
