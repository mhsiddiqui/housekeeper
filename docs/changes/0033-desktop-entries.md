# 0033: Launchers and commands tell apps from components

- **Date:** 2026-11-24
- **Branch / PR:** `feat/desktop-entries`
- **Plan chunks:** 2.1 (desktop entries), done
- **Commits:** 64567d6, 53092c7

## What changed
- **Model** (`crates/core/src/model.rs`): `Launcher` (id, name, icon, file, files it hides, program, hidden);
  `Package::launchers` and `Package::icon`; `PackageKind::Component` for packages with nothing a person launches;
  `Scan::warnings` for problems that belong to no source (left out of JSON when empty). `SCAN_SCHEMA` stays 1: fields
  and a kind were added, none removed or changed in meaning.
- **`Platform::launchers` and `Platform::command_dirs`** (defaults: none, and the `PATH`).
- **Linux** (`crates/linux/src/desktop.rs`): desktop entries from `applications/` under `$XDG_DATA_HOME` and each
  `$XDG_DATA_DIRS` entry (default `/usr/local/share:/usr/share`), subfolders included; desktop IDs from the path
  (`kde4/okular.desktop` → `kde4-okular.desktop`); the first file per id wins and records the files it hides. Only
  `Type=Application`; `NoDisplay`/`Hidden` make it hidden; string escapes and `Exec` quoting are resolved; the program
  is found past `env` and its variables, looked up in the command folders when bare, and left unknown for shells.
  `LinuxPlatform::command_dirs` is the `PATH` plus the standard program folders (`/usr/sbin` and friends, which a
  Debian user's `PATH` lacks).
- **Core `ownership.rs`**: `who`'s evidence chain (listed file → inside an install → inside a source folder) moved out
  of `who.rs` and batched: every package database is asked once about every file of every chain.
- **Core `identity.rs`**, run by every scan: while the sources are read, a thread reads the launchers and every program
  in the command folders (each folder once, so `/bin` = `/usr/bin` counts once) and asks the selected sources' package
  databases about all of their files. Afterwards:
  - a launcher goes to the first of its file, the files it hides, and its program that a source accounts for;
  - a program counts as a package's command only when a package database lists it (`/snap/bin/x` leads to
    `/usr/bin/snap`, which is snapd's file, but it is the snap's command);
  - an `Unknown` package with a visible launcher or a command becomes `App`; with neither, when its package database
    answered, `Component`;
  - the main launcher (the only visible one, or the one named after the package) gives the title and icon when the
    source has none.
  A package database that cannot answer becomes a warning of its source and leaves its packages' kinds alone.
- **`who` fix**: inside a source folder, the package is now matched by the names along the chain in order. Before,
  any name could match, so `/snap/bin/sublime-merge` → `/usr/bin/snap` → `/usr/lib/snapd/snapd` was credited to the
  snapd snap whenever snapd came first in the list.
- **rpm**: above 64 paths, owners come from `rpm -qa --queryformat '[%{=NAME}\t%{FILENAMES}\n]'` (every file once)
  instead of `rpm -qf`, which prints an owning package's whole file list once per path.
- **CLI**: the default `list` hides components; `--all` help and the summary line say so; scan warnings are printed.
- **`scripts/test-package.sh`**: on every distribution, the default list must show `housekeeper` (its command makes it
  an app, through the real batched ownership query) and the native source must report at least one component.
- Docs: decision D28 and the `Platform` row in the architecture, the guide's `Platform` walkthrough, README (default
  list, menu entries, JSON fields), CHANGELOG, plan 2.1 ✅, decision range D1–D28 in AGENTS.md and docs/README.md.

## Why
Plan chunk 2.1 and spec principle 3 ("Apps first, not packages"). Package managers do not say which packages a person
launches, and apt marks everything the installer put in as manual: on this Ubuntu 26.04 desktop `housekeeper list`
showed 525 entries, 448 of them dpkg packages (104 Python modules, 40 Perl, 23 fonts, firmware…). Only 70 dpkg packages
own a desktop file, so desktop entries alone would have left most of that noise; adding commands from the package
database was agreed with the user before starting (D28 records the alternatives).

## How it was verified
- New tests: `identity.rs` (a native package with a launcher and a command; a command reached through a merged-`/usr`
  folder counted once; a browser and a web app it made, with the title from the launcher named after the package; a
  font package as a component; a library left a library; a failing ownership query leaving kinds alone with a warning;
  a user's copy of a Flatpak entry staying with the Flatpak app, not the `flatpak` package its `Exec` runs; a snap
  entry going to its snap, not the snapd snap nor the snapd package), `desktop.rs` (entry parsing, `Exec` quoting and
  escapes, `env`, shells, precedence and hidden files, ids from subfolders, default data dirs), rpm's whole-database
  listing, and the CLI filter. The snap test failed on the first version of the command collection, which dropped
  `/usr/bin/snap` as a duplicate of the `/snap/bin` links leading to it.
- `cargo fmt --all --check`, `rustfmt --check crates/linux/src/sources/*.rs`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo test --workspace --all-features` (5 + 20 + 102 tests), `cargo doc` with
  `-D warnings`, and `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu` pass.
  `cargo deny` is not installed here and was not run; no dependency was added.
- On this Ubuntu 26.04 machine (dpkg, Snap, Flatpak, npm, pipx, uv, pip, cargo, hand installs): the default list went
  from 525 to 210 entries; with `--all`, dpkg has 479 apps, 561 components and 1,019 libraries; 136 of the 141 desktop
  entries found a package (Brave got its four web apps, the `code` package became "Visual Studio Code", the Godot
  entry the user copied into `~/.local` stayed with the Flatpak app, every snap entry went to its snap).
  `housekeeper list` took 0.57–0.60 s over three runs (0.30 s before); `who git` 0.55 s.
  `who sublime-merge` answered "snapd" with a build of `main` and "Sublime Merge" with this branch.
- `scripts/package.sh` (nfpm run from its container image) and `scripts/test-package.sh dist` in Debian stable,
  Ubuntu 24.04, Fedora (latest), AlmaLinux 9, openSUSE Leap and Arch Linux containers, x86_64: all passed, including
  the new checks. In Fedora, Arch and Debian containers `list` took 0.26 s, 0.11 s and 0.05 s; Fedora's 148 rpm
  packages split into 36 apps, 41 components and 71 libraries.

## Known limits and follow-ups
- Launchers come from the folders `$XDG_DATA_DIRS` names. A session that lacks Flatpak's or Snap's folders there (a
  login without their profile scripts, SSH) does not see their entries; the apps are still listed by their sources.
- Names are the entries' untranslated `Name`; icons are the `Icon` value (a theme name or a path), not resolved to a
  file. The desktop app (Phase 6) will need the file.
- Only the command currently selected through alternatives counts: a second JDK whose `java` is not the selected one
  is a component.
- An entry that starts a shell script (`sh -c …`) or a relative path is matched by its file only.
- Grouping one app installed from several sources by desktop ID is chunk 2.2; listing launchers per package is 2.4.
