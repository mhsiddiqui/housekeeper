# Changelog

All notable changes to Housekeeper. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project uses [Semantic Versioning](https://semver.org/).

## Unreleased

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
