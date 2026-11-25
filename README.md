# Housekeeper

[![CI](https://github.com/mhsiddiqui/housekeeper/actions/workflows/ci.yml/badge.svg)](https://github.com/mhsiddiqui/housekeeper/actions/workflows/ci.yml)

*Every app, however it got here.*

Housekeeper shows every app installed on a Linux machine, whatever installed it: the distribution's package manager
(apt/dpkg, dnf/rpm, zypper, pacman), Flatpak, Snap, Homebrew, AppImage, npm, pipx, uv, pip, cargo, or a script you
ran once and forgot. Later versions will also remove each one the way its own installer intended.

It is a front door to the native tools, not a replacement for them: it reads what they already know and, when it
changes something, it will ask them to do it.

> **Status:** 0.1, early development. Housekeeper lists software and changes nothing. Removal, installing downloaded
> files and the desktop app are planned; see the [build plan](docs/planning/plan.md).

## Why
Linux software arrives through many channels, and each keeps its own list. `apt list --installed` knows nothing about
your Flatpaks, `flatpak list` knows nothing about `npm -g`, and nothing knows about the tarball you unpacked into
`/opt` two years ago. Housekeeper asks every one of them and shows the answers in one list, with where each app came
from.

## What it finds
| Kind | Sources | How it reads them |
|---|---|---|
| Distribution packages | dpkg (Debian, Ubuntu…), rpm (Fedora, RHEL, openSUSE…), pacman (Arch…) | `dpkg-query`, `rpm --queryformat`, pacman's database files; apt, zypp and pacman marks tell apps you chose from dependencies |
| Image-based systems | rpm-ostree (Fedora Silverblue, Kinoite, Bazzite…) | packages you layered on top of the image count as yours |
| Universal formats | Flatpak (system, user and custom installations), Snap, Homebrew on Linux | `flatpak list`, snapd's API, Homebrew's Cellar and Caskroom |
| Portable apps | AppImage | the AppImage signature, in `~/Applications`, `~/Downloads`, `~/.local/bin`, `/opt` and similar folders |
| Language tools | npm globals (system Node, npm prefixes, every nvm and fnm version), pipx, `uv tool`, `pip install --user`, `cargo install` | their own metadata files; no interpreter is started |
| Installed by hand | `/opt`, `/usr/local/bin`, `~/.local/bin`, `~/bin`, and script installers such as rustup, nvm, pyenv, Bun, Deno, SDKMAN!, Volta, Homebrew, Oh My Zsh | what is left once every other source and the package manager have claimed their files |

Exactly one distribution package manager is used per system, chosen from `/etc/os-release`, so a stray `rpm` on
Ubuntu does not show up as a second system.

## Install
Download the package for your distribution from the
[latest release](https://github.com/mhsiddiqui/housekeeper/releases/latest) and install it with your package manager:

| Distribution | Command |
|---|---|
| Debian, Ubuntu, Linux Mint, Pop!_OS | `sudo apt install ./housekeeper_*_amd64.deb` |
| Fedora, RHEL, AlmaLinux, Rocky Linux | `sudo dnf install ./housekeeper-*.x86_64.rpm` |
| openSUSE Leap, Tumbleweed | `sudo zypper install --allow-unsigned-rpm ./housekeeper-*.x86_64.rpm` |
| Arch Linux, EndeavourOS, Manjaro | `sudo pacman -U ./housekeeper-*-x86_64.pkg.tar.zst` |
| Fedora Silverblue and anything else | the tarball, see below |

On 64-bit ARM (aarch64), take the files named `arm64` or `aarch64` instead. The packages are not signed yet; check
what you downloaded against the release's checksums:

```sh
sha256sum -c SHA256SUMS --ignore-missing
```

**Tarball.** Every build is one statically linked binary, so it runs on any distribution, old or new:

```sh
tar -xzf housekeeper-*-x86_64-linux.tar.gz
install -Dm755 housekeeper-*-x86_64-linux/housekeeper ~/.local/bin/housekeeper
```

**Uninstall** the same way you installed: `sudo apt remove housekeeper`, `sudo dnf remove housekeeper`,
`sudo zypper remove housekeeper`, `sudo pacman -R housekeeper`, or delete `~/.local/bin/housekeeper`.

## Use
```sh
housekeeper sources                            # which sources exist on this machine
housekeeper list                               # apps and commands someone installed on purpose
housekeeper list --all                         # plus dependencies, libraries, runtimes, components and system packages
housekeeper list --source flatpak --source snap  # only these sources (repeatable)
housekeeper list --search godot                # name, title or summary contains this text
housekeeper list --duplicates                  # apps installed more than once, grouped
housekeeper list --json                        # the whole scan as JSON
housekeeper who vim                            # which source and package a command or file comes from
housekeeper show code                          # everything about one item: files, what needs it, how it is removed
```

`housekeeper sources` shows what was detected:

```text
Ubuntu 26.04.1 LTS (debian, x86_64)

SOURCE                       ID        STATUS
Debian packages              dpkg      available, 1.23.7
Flatpak                      flatpak   available, 1.16.6
Homebrew                     homebrew  not available: Homebrew is not installed
Snap                         snap      available, 2.77.1+ubuntu26.04.2
AppImage                     appimage  available
cargo install                cargo     available
npm (global)                 npm       available
pip (user)                   pip       available
pipx                         pipx      available
uv tools                     uv        available
Installed by hand or script  manual    available
```

`housekeeper list` prints a table on standard output and a summary on standard error:

```text
NAME                VERSION             SOURCE   SIZE
Godot Engine        4.7.2               flatpak  171.8 MB
Visual Studio Code  1.141.0-1791276758  dpkg     1.1 GB

2 shown of 10 found (use --all to include dependencies, libraries, runtimes and components).
```

By default the list keeps what someone chose to install and a person uses: something with an entry in the
application menu or a command on the `PATH`. It hides what came along with it (dependencies, libraries, Flatpak
runtimes, snap bases) and components: packages of the distribution's package manager with neither a menu entry nor a
command, such as fonts, firmware, themes and data. Names come from the menu entry when the source has none
(`Visual Studio Code` for the `code` package). `--all` shows everything. The IDs in the `sources` table are the values
`--source` accepts.

Menu entries are the desktop entries in `applications/` under `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, the folders
your desktop's menu reads, so an entry a terminal session cannot see (Flatpak and Snap add their folders to
`XDG_DATA_DIRS` at login) is missing from the list too.

### Installed more than once
`housekeeper list --duplicates` groups the apps installed more than once: from several sources (Sublime Merge as a
`.deb` and as a snap), or twice from one (a tool installed with npm under two Node versions). The installs stay
separate rows; `FROM` tells them apart:

```text
APP            NAME           VERSION  SOURCE  SIZE     FROM
Sublime Merge  Sublime Merge  2136     dpkg    28.7 MB
               Sublime Merge  2136     snap    82.4 MB  Snap Store, Snapcrafters (verified), latest/stable
python-dotenv  python-dotenv  1.2.4    pip              pip, Python 3.14
               python-dotenv  1.0.1    pip              pip, Python 3.8
```

Installs are grouped when they share an AppStream ID (from the app's catalog entry in `/usr/share/metainfo`, its
Flatpak ID or its snap's `common-id`), a menu-entry ID, a name or title, or a command. Within one source only
packages of the same name count as two installs, so `vim` and `vim-common` stay apart. A group shows when any of its
installs passes the other filters, with all of its installs. The plain `list` says how many apps are installed
more than once.

### Everything about one item
`housekeeper show` takes an ID from `list --json` or `--duplicates` (`dpkg:code`, `snap:sublime-merge`), or a name or
title, and prints what Housekeeper knows about that one item:

```text
$ housekeeper show dpkg:sublime-merge
Sublime Merge
dpkg:sublime-merge

Summary:    Sublime Merge is a Git Client, from the makers of Sublime Text
Version:    2136
Source:     Debian packages
Known from: dpkg's database, through `dpkg-query --show`
Installed:  installed on purpose, for all users
Kind:       app
Size:       28.7 MB
Commands:   smerge
Launchers:  Sublime Merge (sublime_merge.desktop)
Also as:    Sublime Merge 2136 (snap:sublime-merge)
Same app:   shares the name sublimemerge
Files:      72 files, 28.7 MB, as the source records them (--files lists them)
Needed by:  nothing installed
Removal:    through its package manager
            as root, by Housekeeper's helper: apt-get remove --yes sublime-merge
```

- **Files** come from the package database (`dpkg-query --listfiles`, `rpm -ql`, pacman's database) or pip's `RECORD`
  when the source records them; otherwise they are everything in the item's install folder (Flatpak, Snap, npm,
  pipx, AppImage, hand installs). `--files` lists every one.
- **Needed by** lists the installed packages of the same source that directly depend on it (dpkg, rpm and pacman
  dependencies, Flatpak apps on a runtime, snaps on a base or using its content), up to 10; `--json` has all of them.
  Optional dependencies (Recommends, optdepends) do not count. "not known" means the source does not record
  dependencies.
- **Removal** is how the item would be removed the way its installer intended: its own uninstaller (rustup), its
  package manager (apt, dnf, zypper, rpm-ostree or pacman for the system's packages; Flatpak, Snap, Homebrew, npm,
  pipx, uv, pip, cargo), its format's convention (an AppImage and the launchers that start it go to the Trash), or
  "not known" (hand installs). Each step says whether it would run as you or as root through Housekeeper's helper,
  and what blocks it (a package of a Silverblue image, a system-wide npm install). Nothing is removed yet; that comes
  with the helper.
- When a name matches several items, they are listed with their IDs and the command exits with 1; when nothing
  matches, it exits with 2. `--json` prints the same as a document.

### Who installed this?
`housekeeper who` takes a command on your `PATH` or a path to a file, follows its symlinks, and names the source
and package it belongs to, with the evidence:

```text
$ housekeeper who vim
/usr/bin/vim → /etc/alternatives/vim → /usr/bin/vim.basic
Owner:   Debian packages · vim 2:9.1.2141-1ubuntu4.10 (installed on purpose, for all users)
Because: Debian packages lists /usr/bin/vim.basic as a file of vim
```

The evidence is one of: the package manager lists the file (`dpkg-query --search`, `rpm -qf`, `pacman -Qo`), the
file is inside a package's install folder (a pipx venv, an npm package, an `/opt` folder, nvm…), or it is inside a
folder a source installs into (`/snap`, Flatpak exports…). When a command exists more than once on the `PATH`, the
others are shown as `Hidden:` behind the one that runs. `--json` prints the same as a document.

It exits with 0 when an owner is found, 1 when the file exists but nothing on the machine accounts for it (copied
by hand or by a script, outside the folders Housekeeper reads), and 2 when there is no such command or file.

### JSON output
`housekeeper list --json` prints one document with the machine (`system`), every source's outcome (`sources`, with
`state` `ok`, `unavailable` or `failed`, and any warnings), every package (`packages`: source, id, name, title,
version, summary, kind (`app`, `runtime`, `library`, `component` or `unknown`), install reason, scope, size, install
time, origin, location, commands, launchers with their id, name, icon, file and program, icon, and AppStream IDs as
`app_ids`), the apps installed more than once (`groups`: name, the installs' `source:id` keys, and what they share)
and problems that belong to no one source (`warnings`, left out when empty). The `schema` field is `1`; it changes
when a field is removed or changes meaning. Filters apply to `packages` only, except with `--duplicates`, which keeps
the groups it shows and their installs.

### When a source is missing or fails
- **not available** in `housekeeper sources` means the tool is not on this machine (or, for Snap, snapd is not
  running). That source is skipped.
- **warning: could not read …** after `housekeeper list` means a source was found but failed, for example a package
  manager that did not answer within 60 seconds. The other sources are still listed.
- **warning: Installed by hand or script: could not ask …** means the package manager could not say which entries of
  `/opt` and `/usr/local/bin` it owns; those entries are left out rather than listed twice.

## Privacy and safety
- **Read-only.** This version never changes the system. It reads package databases and metadata files and runs the
  package managers' query commands, each with a timeout.
- **No root.** Run it as yourself; it never asks for a password.
- **Nothing leaves the machine.** No network access, no account, no telemetry.

## Supported distributions
Every release is installed, run and removed on these, for x86_64 and aarch64, before it is published:

Debian (stable, oldstable) · Ubuntu 22.04, 24.04 and the latest release · Fedora (latest) · AlmaLinux 9 and 10 ·
openSUSE Leap and Tumbleweed · Arch Linux (x86_64 only) · Fedora Silverblue (tarball, x86_64 only).

Derivatives work through `ID_LIKE` in `/etc/os-release` (Linux Mint, Pop!_OS, Rocky Linux, EndeavourOS, Manjaro,
CachyOS…). On a distribution none of the package managers claims, Housekeeper uses whichever of dpkg, rpm or pacman
is installed, and every other source works as usual.

## Build from source
You need Rust 1.85 or newer.

```sh
git clone https://github.com/mhsiddiqui/housekeeper.git
cd housekeeper
cargo build --release -p housekeeper-cli
./target/release/housekeeper list
```

`scripts/package.sh` builds the same static packages as a release (it needs the `x86_64-unknown-linux-musl` Rust
target and [nfpm](https://nfpm.goreleaser.com)). Running the checks and contributing are described in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Roadmap
The [build plan](docs/planning/plan.md) has the details. In order:

1. **Identity and ownership:** link apps to their desktop entries and icons, group one app installed from several
   sources, `housekeeper who <command or path>`, `housekeeper show <app>`.
2. **Removal planning:** for each app, the way its installer intended (its own uninstaller, its package manager, or a
   reviewed recipe), simulated first, with protected system packages blocked.
3. **Making changes safely:** a small privileged helper started through polkit, an operation queue, a history log, and
   leftovers moved to the Trash.
4. **Installing downloaded files** (`.deb`, `.rpm`, AppImage, Flatpak references…) the right way for the distribution.
5. **Desktop app** (v1), then updates, environment and PATH, discovery, cleanup and a tracked terminal for install
   scripts.

## Documentation
All documentation lives in [docs/](docs/README.md), grouped by section:

- **Product**: [spec](docs/product/spec.md), [interactive mockup](docs/design/mockup/index.html) (open in a browser).
- **Architecture**: [overview and decisions](docs/architecture/README.md),
  [illustrated guide](docs/architecture/guide.html) (how to add a distribution or operating system).
- **Planning**: [build plan](docs/planning/plan.md), phases and chunks with status.
- **Development**: [contributing](CONTRIBUTING.md), [releasing](docs/development/releasing.md).
- **History**: [change records](docs/changes/README.md) (one per change), [changelog](CHANGELOG.md) (per release).

Working on the code with an AI agent? It follows [AGENTS.md](AGENTS.md).

## Contributing
Bug reports and pull requests are welcome on [GitHub](https://github.com/mhsiddiqui/housekeeper/issues). When
something is listed wrongly, missing, or shown twice, the output of `housekeeper sources` and your distribution's
`/etc/os-release` help a lot. How to work on the code is in [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence
Copyright (C) 2026 Muhammad Hassan Siddiqi.

Housekeeper is free software: you can redistribute it and/or modify it under the terms of the
[GNU General Public License](LICENSE) as published by the Free Software Foundation, either version 3 of the License,
or (at your option) any later version (`GPL-3.0-or-later`). Anyone who distributes Housekeeper, modified or not, must
make the source available under the same licence.
