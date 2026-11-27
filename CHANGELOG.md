# Changelog

All notable changes to Housekeeper. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project uses [Semantic Versioning](https://semver.org/).

## Unreleased

### Added
- `housekeeper inspect <file>`: what a downloaded installer is (`.deb`, `.rpm`, Arch package, `.flatpakref`, Flatpak
  bundle, `.snap`, AppImage, archive, script, `.run` program), told by its name and first bytes, with the package's
  name, version and processor where the file or the format's own tool says (`dpkg-deb`, `rpm`, `bsdtar`,
  `unsquashfs`), the source that would list it, and whether it fits this system: another distribution's package, a
  format whose tool is not here (Flatpak, snapd) or a program for another processor does not. It never runs or
  unpacks the file. `--json` for scripts.
- `housekeeper inspect` says how a file that fits would be installed, without installing it: `.deb`, `.rpm` and Arch
  packages through apt, dnf, zypper, rpm-ostree (after a restart) or pacman, as root through Housekeeper's helper;
  Flatpak references and bundles with `flatpak install --user`; snaps with `snap ack` and the store's `.assert` file
  beside them, or with `--dangerous` (flagged) without one. It shows what the package manager would install, need from
  its repositories or remove with it, asked without root (`apt-get --simulate`, `rpm -U --test`, `pacman --print`),
  and the version installed now that it would replace. A refusal (a conflict, a newer version installed, an installed
  package it would break) or a protected package it would remove blocks the install. Exits 0 when the file fits and
  nothing blocks it, 1 otherwise, 2 for no such file or an unknown format; `--json` has the plan as `install`.

## 0.2.0 (2026-11-26)

Housekeeper can now remove what it lists, the way each app's installer intended: it shows the plan and what would be
left behind, checks everything again just before acting, asks for an administrator's password only for steps that
need root, moves leftovers to the Trash instead of deleting them, and records every change.

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
  `RECORD`, otherwise its install folder; `--files` lists them) and the packages that directly need it (on RPM
  systems, also those that require one of its programs, such as `/usr/bin/sed`). `--json` for scripts.
  `housekeeper sources --json` now says how each source knows what is installed (`evidence`).
- `housekeeper show` says how the item would be removed the way its installer intended, without removing anything:
  through the distribution's own tool for system packages (apt, dnf, zypper, rpm-ostree, pacman), through Flatpak,
  Snap, Homebrew, npm (with the npm of the Node version it is under), pipx, uv, pip or cargo for theirs, by trashing
  an AppImage and the launchers that start it, or with rustup's own uninstaller. Each step says whether it would run
  as you or as root, and what blocks it (a package that is part of a Silverblue image, an npm or cargo install
  outside your home folder). Removing with dnf or zypper uses only what is installed, never online repositories, so a
  mirror that is down cannot stop it. `--json` has the plan as `removal`.
- A protected package's removal is blocked, and `housekeeper show` says why it is protected: dpkg's Essential or
  Protected mark (newly read; `init` and `libgcc-s1` are Protected on Debian and Ubuntu) or Priority required, dnf's
  protected packages (now also `protected_packages` in `/etc/dnf/dnf.conf`), Arch's `base` and everything it depends
  on (new), the core packages, snapd and the system's snaps, and the running kernel (new, on every distribution).
- `housekeeper show` lists what else a system package's removal would take, without root: `apt-get --simulate`
  (Debian, Ubuntu), `pacman --remove --print` (Arch), and on Fedora and openSUSE every package that needs it in the
  RPM database, directly or through others. A refusal, or a protected package among what would go (removing
  `libsemanage2` takes `passwd`), blocks the removal. `--json` has it as `removal.simulation`.
- Script-installed tools are described by reviewed recipes (`crates/platform/linux/recipes/`), which say how to find
  them and how to remove them: `housekeeper show` plans nvm, pyenv, Bun, Deno, SDKMAN!, Volta and Atuin by moving
  their folder to the Trash, and rustup with `rustup self uninstall`. Homebrew's and Oh My Zsh's own folders stay
  "not known".
- `housekeeper remove --dry-run <id or name>`: the removal plan in full with what the item would leave behind:
  folders named after it in `~/.config`, `~/.cache`, `~/.local/share`, `~/.local/state` and a Flatpak's
  `~/.var/app/<id>`, and lines in shell startup files that load it. Only folders the item can use are listed (none in
  `~/.config` for a Flatpak or a strictly confined snap, never folders every program shares such as
  `~/.config/dconf`), and folders of pip, npm, cargo and other language packages say that copies in projects or
  virtual environments may still use them. Each folder shows its size (counting at most 50,000 files and folders, so
  a bigger one shows "at least" its size), whether it is your data, and whether it would go with the item: a cache
  that nothing else may use "goes to the Trash with it"; everything else is "kept unless you choose it". It changes
  nothing. `housekeeper show` counts the leftovers; `--json` has them as `leftovers` (with `unseen_copies`,
  `size_complete` and the `identity` checked again before a folder moves).
- Housekeeper's root helper, `/usr/lib/housekeeper/housekeeper-helper`, and its polkit action are installed with the
  packages (the tarball carries them too). `pkexec` starts it, asking for an administrator's password each time; it
  takes one request on standard input and removes only installed packages, named plainly, through the system's own
  package tool, or moves a file outside every home folder (an AppImage in `/opt`) to root's Trash.
- `housekeeper remove <id or name>`: removes an installed item the way its installer intended. It shows the plan and
  the leftovers, asks (`--yes` skips the question), checks the plan again just before acting, and runs each step as
  you or, for what needs root, through the root helper (`pkexec` asks for an administrator's password). It waits while
  apt, dnf, zypper or pacman is busy, stops at the first step that fails, then moves the chosen leftovers to the Trash
  (`--leftovers caches`, the default, `none` or `all`) and writes the outcome to the history. Exit codes: 0 removed,
  3 nothing changed, 4 a step failed.
- `housekeeper history [--json]`: what Housekeeper changed on this computer, from `history.jsonl` in
  `~/.local/state/housekeeper`. Each scan (`list`, `show`) also keeps `seen.jsonl` there: every item found, with when
  it was first and last seen, so the leftovers of something removed outside Housekeeper can be named later. Nothing
  leaves the computer.

### Changed
- `housekeeper list` shows apps and commands by default. Native packages with neither a menu entry nor a command
  (fonts, firmware, themes, data, plugins) are a new kind, `component`, shown with `--all`. On an Ubuntu desktop the
  default list went from 525 to 210 entries.
- `--json`: `protected` is now the reason a package is protected (a string, or `null`) instead of `true`/`false`, so
  the scan's `schema` is `2`.
- Kernels other than the running one are no longer protected on Fedora, openSUSE and Arch, so older kernels can be
  removed; the running kernel is protected instead.

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
