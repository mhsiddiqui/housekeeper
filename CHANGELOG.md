# Changelog

All notable changes to Housekeeper. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project uses [Semantic Versioning](https://semver.org/).

## Unreleased

### Added
- `housekeeper who <command or path>`: which source and package a command or file comes from, following its
  symlinks, with the evidence (the package manager lists it, it is inside a package's install folder, or inside a
  folder a source installs into) and the other copies hidden behind it on the `PATH`. `--json` for scripts; exits 0
  when an owner is found, 1 when nothing accounts for the file, 2 when there is no such command or file.
- Menu entries (desktop entries) are attached to the packages that installed them, whatever the source: packages
  get their launchers, a title and an icon from them (`Visual Studio Code` for the `code` package). In `--json`:
  `launchers` and `icon` on each package, `warnings` for problems reading the menu.
- Packages from dpkg, rpm and pacman now list the commands they put on the `PATH`.
- `housekeeper list --duplicates`: apps installed more than once, from several sources (a `.deb` and a snap) or twice
  from one (npm under two Node versions), grouped by a shared AppStream ID, menu-entry ID, name or command, with a
  `FROM` column that tells the copies apart. The plain `list` says how many there are. In `--json`: `groups`, and
  `app_ids` on each package (from AppStream metainfo, Flatpak IDs and snaps' `common-id`).
- `housekeeper show <id or name>`: everything about one installed item: details, how its source knows it is
  installed, commands, launchers, AppStream IDs, its other installs, its files (from the package database or pip's
  `RECORD`, otherwise its install folder; `--files` lists them) and the packages that directly need it. `--json` for
  scripts. `housekeeper sources --json` now says how each source knows what is installed (`evidence`).
- `housekeeper show` says how the item would be removed the way its installer intended, without removing anything:
  through the distribution's own tool for system packages (apt, dnf, zypper, rpm-ostree, pacman), through Flatpak,
  Snap, Homebrew, npm (with the npm of the Node version it is under), pipx, uv, pip or cargo for theirs, by trashing
  an AppImage and the launchers that start it, or with rustup's own uninstaller. Each step says whether it would run
  as you or as root, and what blocks it (a package that is part of a Silverblue image, an npm or cargo install
  outside your home folder). `--json` has the plan as `removal`.
- A protected package's removal is blocked, and `housekeeper show` says why it is protected: dpkg's Essential or
  Protected mark (newly read; `init` and `libgcc-s1` are Protected on Debian and Ubuntu) or Priority required, dnf's
  protected packages (now also `protected_packages` in `/etc/dnf/dnf.conf`), Arch's `base` and everything it depends
  on (new), the core packages, snapd and the system's snaps, and the running kernel (new, on every distribution).
- `housekeeper show` lists what else a system package's removal would take, without root: `apt-get --simulate`
  (Debian, Ubuntu), `pacman --remove --print` (Arch), and on Fedora and openSUSE every package that needs it in the
  RPM database, directly or through others. A refusal, or a protected package among what would go (removing
  `libsemanage2` takes `passwd`), blocks the removal. `--json` has it as `removal.simulation`.

### Changed
- `housekeeper list` shows apps and commands by default. Native packages with neither a menu entry nor a command
  (fonts, firmware, themes, data, plugins) are a new kind, `component`, shown with `--all`. On an Ubuntu desktop the
  default list went from 525 to 210 entries.
- `--json`: `protected` is now the reason a package is protected (a string, or `null`) instead of `true`/`false`, so
  the scan's `schema` is `2`.
- Kernels other than the running one are no longer protected on Fedora, openSUSE and Arch, so older kernels can be
  removed; the running kernel is protected instead.
- `housekeeper show`'s "Needed by" on RPM systems also counts packages that require one of the package's programs
  (`/usr/bin/sed`), not only what it provides.

### Fixed
- `housekeeper who` named the wrong snap for a snap command when a later file on its symlink chain had another
  snap's name (`/snap/bin/pinta` → … → `/usr/lib/snapd/snapd` could be credited to the snapd snap).

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
