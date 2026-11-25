# 0034: Apps installed more than once are grouped

- **Date:** 2026-11-25
- **Branch / PR:** `feat/group-apps`
- **Plan chunks:** 2.2 (group one app installed from several sources), done
- **Commits:** c720b7f, 48c9eac

## What changed
- **Model:** `Package::app_ids` (AppStream component IDs); `Metainfo` (an app's catalog entry on disk: ID, file,
  the launcher IDs it names); `Scan::groups` with `AppGroup` (name, the installs' `source:id` keys, what they share)
  and `Shared` (`app-id`, `name`, `command`). `SCAN_SCHEMA` stays 1: only additions.
- **`Platform::metainfo`** (default: none). Linux (`crates/linux/src/appstream.rs`) reads `metainfo/` and `appdata/`
  under `$XDG_DATA_HOME` and each `$XDG_DATA_DIRS` entry and takes each file's component ID (the first `<id>`, after
  comments are removed, so `<provides>`/`<replaces>` IDs are not taken for it) and its `desktop-id` launchables. The
  files are scanned for those two elements, not parsed as general XML; no dependency was added. The XDG data-dir list
  moved from `desktop.rs` to `xdg.rs`, shared by both.
- **Core `identity.rs`:** catalog entries join the same batched ownership query as launchers and commands. An entry's
  ID goes to the package that accounts for its file, or else to the package whose launcher it names.
- **Sources:** a Flatpak app's ID is its AppStream ID; snaps report their apps' `common-id`.
- **Core `group.rs`:** `App` packages sharing an AppStream or launcher ID (without `.desktop`, case-insensitive), a
  name or title (lowercase letters and digits only), or a command are linked, transitively. Within one source only
  packages of the same name are linked (two npm installs of one tool), so the parts of one install (`vim`,
  `vim-common`) stay apart. Libraries, runtimes and components never take part. Groups come in list order and are
  named after their first install with a title. Decision D29.
- **CLI:** `housekeeper list --duplicates` shows only grouped apps, one row per install with the group's name on its
  first row and a `FROM` column (origin, or else location) to tell copies apart. A group shows when any of its
  installs passes `--all`/`--search`, with all of its installs. The plain `list` adds "N apps installed more than once;
  `housekeeper list --duplicates` shows which." to its summary.
- Docs: README (`--duplicates`, JSON `groups` and `app_ids`), architecture (Platform row, the scan paragraph, D29),
  guide, CHANGELOG, plan 2.2 ✅, decision range D1–D29.

## Why
Plan chunk 2.2 and spec §1 ("Duplicate detection: the same app from two sources"). The plan named AppStream ID,
desktop ID and name. On this Ubuntu desktop no two installs shared an AppStream ID, while names and commands found the
four real duplicates (one of them, Claude Code from npm and from its own installer, only by the command `claude`), so
commands were added as a key. The first run also grouped `vim` with `vim-common` by title; that is why packages of one
source are linked only by an identical name.

## How it was verified
- New tests: `group.rs` (a `.deb` and a Flatpak with different titles tied by the metainfo the `.deb` installs; a
  `.deb` and a snap by name; an npm package and a hand install by command; one tool installed twice with npm; `vim`
  and `vim-common` not grouped; libraries and runtimes not grouped; group names and what they share), a catalog entry
  nobody installed reaching its package through the launcher it names, `appstream.rs` (component ID before
  `<replaces>`, comments, `<identifier>` not taken for `<id>`, entities, empty IDs; metainfo and appdata under every
  data dir), the snap `common-id`, and `--duplicates` filtering (a group shows when one install matches, with all of
  its installs, in group order).
- `cargo fmt --all --check`, `rustfmt --check crates/linux/src/sources/*.rs`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo test --workspace --all-features` (6 + 22 + 104 tests), `cargo doc` with
  `-D warnings`, and `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu` pass.
  `cargo deny` is not installed here and was not run; no dependency was added.
- On this Ubuntu 26.04 machine: `housekeeper list --duplicates` shows four groups: Claude Code (npm under Node
  22.18.0 and 22.19.0, and the hand install in `~/.local/share/claude`), CUPS (dpkg and snap), python-dotenv (pip for
  Python 3.14 and 3.8) and Sublime Merge (dpkg and snap). 53 of the 59 metainfo files gave their ID to a dpkg package;
  4 snaps reported `common-id`s. `housekeeper list` took 0.26–0.28 s over three runs.
- `scripts/package.sh` (nfpm from its container image) and `scripts/test-package.sh dist` in Debian stable, Ubuntu
  24.04, Fedora (latest), AlmaLinux 9, openSUSE Leap, openSUSE Tumbleweed and Arch Linux containers, x86_64: all
  passed.

## Known limits and follow-ups
- A name match can join unrelated software that shares a name (Debian's `yarn` from cmdtest and npm's `yarn`); the
  group lists what was shared, so the evidence is visible, but nothing separates them yet.
- Two installs that share nothing Housekeeper reads (an AppImage named differently from the Flatpak) stay apart.
- The group's name is its first install's title, which may be a package name (`@anthropic-ai/claude-code`).
- Desktop IDs only match when another install reports the same ID; two launchers of one ID cannot both show, since the
  first hides the second (D28).
