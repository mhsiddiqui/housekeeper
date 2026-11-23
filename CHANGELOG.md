# Changelog

All notable changes to Housekeeper. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project uses [Semantic Versioning](https://semver.org/).

## Unreleased

### Changed
- Licence: Housekeeper is now under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`) instead of MIT,
  so every distributed version, modified or not, must stay open source.

### Added
- `housekeeper sources` and `housekeeper list` (`--all`, `--source`, `--search`, `--json` with schema version 1).
- Read-only inventory from every source, scanned in parallel; a failing source is reported without hiding the rest:
  - native packages: dpkg (with apt's auto-installed marks), rpm (zypp auto-installed marks, rpm-ostree layered
    packages, dnf protected packages), pacman (local database, AUR/foreign packages);
  - Flatpak (system, user and custom installations), Snap (snapd API) and Homebrew on Linux (formulae with
    explicit/dependency reasons and taps, casks);
  - AppImage files in the usual folders;
  - npm globals for system Node, npm prefixes, every nvm and fnm version; pipx; `uv tool`; `pip install --user`;
    `cargo install`;
  - installs by hand or script: `/opt`, `/usr/local/bin`, `~/.local/bin`, symlink farms grouped by install folder,
    and well-known script installers (rustup, nvm, pyenv, atuin, bun, deno, SDKMAN, Homebrew, Volta, oh-my-zsh).
- Packages for every release: `.deb`, `.rpm`, Arch package and tarball for x86_64 and aarch64, statically linked.
- CI: lint, tests, minimum Rust version, docs, dependency audit, and package install/remove tests on Debian 12 and 13,
  Ubuntu 22.04, 24.04 and latest, Fedora, AlmaLinux 9 and 10, openSUSE Leap and Tumbleweed, Arch Linux, and
  Fedora Silverblue.
