# 0042: Find leftovers only in folders the item can use

- **Date:** 2026-11-26
- **Branch / PR:** `fix/leftover-matching`, #32
- **Plan chunks:** 3.6 (a), done; 3.6 (b) next
- **Commits:** 9e52dfe, cfb165c, 59d7a1c

## What changed
A review of [0041](0041-leftovers.md) found leftover folders credited to items that cannot own them. This change
narrows which folders are searched for which item, and links back to that record rather than rewriting it.

- **Folders tied to their source.**
  - New `Source::leftover_roots`, empty by default. Flatpak returns `~/.var/app`, which leaves
    `LinuxPlatform::leftover_roots`; only Flatpak's items are matched against it.
  - Before, a `.deb` whose AppStream ID was a Flatpak's (`org.mozilla.firefox`) listed that Flatpak's
    `~/.var/app/<id>` as its own, with no warning once the Flatpak was gone.
- **Sandboxed items.**
  - New `Package::sandboxed`, set by Flatpak on every ref and by Snap when snapd reports `strict` or `devmode`
    confinement (a new `confinement` field read from `/v2/snaps`).
  - A sandboxed item has no leftovers in the platform's XDG folders: Flatpak redirects an app's XDG folders into
    `~/.var/app/<id>`, and a confined snap's into `~/snap/<name>`.
  - It is also left out of other items' `also_used_by` there. Classic snaps (Ghostty, Sublime Merge) keep matching
    `~/.config/<name>`.
- **Shared folders.**
  - The list of folders several programs share moved out of core into `Platform::shared_state_folders`: the names were
    freedesktop's, and core names no OS.
  - Linux adds `containers`, `dconf`, `environment.d`, `gtk-2.0`, `gtk-3.0`, `gtk-4.0`, `keyrings` and `pulse`.
    Fedora, openSUSE and Arch ship a package named `dconf`, which before would have claimed `~/.config/dconf`, the
    settings of every GNOME app.
- **Copies Housekeeper does not read.**
  - New `Leftover::unseen_copies` (always in `--json`), true for items of a language source (pip, npm, cargo, pipx,
    uv).
  - The CLI adds "copies in projects or virtual environments Housekeeper does not read may still use it".
  - Example: `~/.config/matplotlib` of a user-site matplotlib is also read by every venv's matplotlib.
- **Shell lines.** `mentions` now also needs a boundary before the path, so `/mnt/backup/home/u/.nvm` no longer counts
  as `/home/u/.nvm`.
- **Docs:** D36 rewritten (which folders each item is matched against, sandboxing, the shared list, unseen copies),
  the `Source` sketch and the removal paragraph in the architecture, README's leftovers bullet, CHANGELOG (Fixed), and
  the plan (new chunk 3.6, Phase 3 back to 🚧 until 3.6 (b)).

## Why
A wrong leftover becomes a wrong Trash move in Phase 4 (4.4). The review's findings 2, 3, 4 and 7 were all wrong
attribution:

- a folder shared by every app credited to one package;
- a sandboxed app's folder credited to a native install, and the other way round;
- other users of a folder missed;
- a shell line in another path.

D36 is changed rather than worked around.

## How it was verified
- **New tests:**
  - core `leftovers.rs`:
    - a package named `Fonts` gets no `~/.config/fonts` (shared folder; the old `fonts` test never matched a name);
    - a Flatpak and a `.deb` with the same AppStream ID: the `.deb` gets `~/.config/<id>` only, with no `also_used_by`;
      the Flatpak gets `~/.var/app/<id>` only, with its other installation in `also_used_by`;
    - a pip package's folder has `unseen_copies`, a system package's has not;
    - `cp -r /mnt/backup/home/user/.nvm` is not an nvm line.
  - `snap.rs`: a strict snap is sandboxed, a classic one (new fixture entry) is not.
  - CLI: the dry-run rendering shows the unseen-copies note.
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (8 + 34 + 148)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - `cargo check --target x86_64-pc-windows-gnu -p housekeeper-core -p housekeeper-platform`
  - `cargo deny` was not run (not installed); no dependency was added.
- **`remove --dry-run` on this Ubuntu 26.04 machine:**
  - `snap:sublime-merge` and `snap:ghostty` (both classic, per `snap list`) still list their `~/.config` folders,
    with the `.deb` of Sublime Merge in `also_used_by`.
  - Godot (Flatpak) lists `~/.var/app/org.godotengine.Godot` only.
  - The pip `matplotlib` folders carry the unseen-copies note.
  - The uv binary's `~/.local/share/uv` (uv's tools) is still left out.

## Known limits and follow-ups
- **Flatpak permissions are not read.** An app with host file access that writes to `~/.config` by a hard-coded path
  anyway is missed; Flatpak sets `XDG_CONFIG_HOME` inside `~/.var/app` for every app, so this is rare.
- **The shared-folder list is curated.** A shared folder not on it, named exactly like a package, would still match.
- **`unseen_copies` is per source, not per folder.** A cargo tool's own settings folder gets the note even when no
  project copy exists.
- **Sizes and exit code.** `show` still measures every leftover folder, and `remove` without `--dry-run` still exits 2
  like "nothing matches". Both are 3.6 (b).
