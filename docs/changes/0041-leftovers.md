# 0041: Leftovers and `housekeeper remove --dry-run`

- **Date:** 2026-11-26
- **Branch / PR:** `feat/leftovers`, PR not opened yet
- **Plan chunks:** 3.5 (leftover discovery and `remove --dry-run`), done; Phase 3 done
- **Commits:** 7a5d460, 5e93feb

## What changed
- **New `leftovers.rs` in core.** It defines `Leftovers`, which holds `folders` (each a `Leftover`) and `shell_lines`
  (each a `ShellLine`).
  - A `Leftover` has a path, a `LeftoverKind` (config, cache, data or state), a size, and `also_used_by`.
  - `LeftoverKind::is_user_data` is false only for caches.
  - `Details::leftovers` carries the result, so `show` and its `--json` output include it.
- **Which folders count.** A folder counts when it sits directly in one of the platform's leftover roots and its name
  equals the item's name, one of its AppStream IDs or one of its launcher IDs. The match ignores case and must be the
  whole name.
  - Folders several programs share (`applications`, `fonts`, `icons`…) never count.
  - Neither do folders holding an install (another package's location, or a source's install root) or folders inside
    the item's own location.
  - A folder whose name also matches another installed item lists that item's key in `also_used_by`.
  - Results are sorted largest first. Sizes do not follow symlinks.
- **Which shell lines count.** A line counts when it mentions the item's own install folder, written absolute or as
  `$HOME/…`, `${HOME}/…` or `~/…`. A longer name that starts the same way (`~/.nvmrc`) does not count.
  - Only items installed in a folder of their own in the home folder are searched.
  - A folder inside a source's install root, or shared with another item (cargo crates in `~/.cargo/bin`), belongs
    to the source, so its lines are not searched.
- **New `Platform` methods, each empty by default:**
  - `leftover_roots`. Linux returns the XDG config, cache, data and state homes (honouring `XDG_*_HOME`) and Flatpak's
    `~/.var/app`.
  - `shell_files`. Linux returns `~/.bashrc`, `~/.bash_profile`, `~/.bash_login`, `~/.profile`, the zsh files in
    `$ZDOTDIR` or the home folder, and fish's `config.fish`.
- **CLI `housekeeper remove --dry-run <id or name>`.** It prints the plan (tier, steps, simulation, blockers), every
  leftover folder with its kind and size, whether it is the user's data ("kept unless you choose it"), any other
  install that may still use it, and the shell lines. It ends with "Nothing was changed".
  - `--json` prints `package`, `removal`, `leftovers` and `warnings`.
  - Exit codes match `show`: 0 when found, 1 when several items match, 2 when nothing does. Without `--dry-run` it
    refuses with exit 2.
  - `show` and `remove` now share one lookup function. When several items match, its hint names the right command.
- **`show`** gains a `Leftovers:` summary line, for example "2 folders, 15.0 MB (`housekeeper remove --dry-run` lists
  them)".
- **`scripts/test-package.sh`** checks, on every distribution, that:
  - `remove --dry-run housekeeper` lists a planted `~/.config/housekeeper` as settings and says nothing was changed;
  - `remove` without `--dry-run` refuses;
  - the package is still installed afterwards.
- **Docs:**
  - README: usage line, a `Leftovers` bullet, a new section with an example, and the roadmap now lists the finished
    phases.
  - Also updated: CHANGELOG, architecture (the `Platform` row, the removal paragraph and new decision D36), the D1–D36
    range in AGENTS, docs/README and the guide, and the plan (3.5 ✅, Phase 3 ✅).

## Why
Plan chunk 3.5 and decision D7. Before anything is removed, the user should see what stays behind and how big it is.
Guessing wrong here could put the user's files or another app's state in the Trash, so D36 keeps matching narrow:

- exact folder names only;
- no shared folders and no folders that hold installs;
- the kind of each folder is shown, and only caches may go without the user choosing;
- shell lines are listed, never edited.

## How it was verified
- **New tests:**
  - core `leftovers.rs`:
    - folders found by kind and sorted largest first, with exact sizes;
    - a launcher-style AppStream folder is matched;
    - shared `fonts` and a near-miss `toolbox` are not matched;
    - `also_used_by` names the snap install of the same app;
    - `~/.local/share/flatpak` (holding a Flatpak install) is not the `flatpak` package's leftover;
    - nvm's lines in `.bashrc` are found by `$HOME` and absolute forms, while the `$NVM_DIR` line and `~/.nvmrc-helper`
      are not;
    - a cargo crate in `~/.cargo/bin` gets no lines.
  - CLI:
    - the `show` summary line;
    - the dry-run output, which marks user data and other users of a folder.
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (8 + 31 + 147)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - the Windows check of core and platform
  - `cargo deny` was not run (not installed); no dependency was added.
- **On this Ubuntu 26.04 machine:**
  - `remove --dry-run dpkg:sublime-merge`: `~/.cache/sublime-merge` (15.0 MB) and `~/.config/sublime-merge`
    (36.2 kB), both marked "snap:sublime-merge may still use it".
  - The same from the snap side.
  - Godot (Flatpak): `~/.var/app/org.godotengine.Godot`, 473.5 MB of data.
  - `installer/nvm`: `.bashrc` lines 144 (`export NVM_DIR="$HOME/.nvm"`) and 204 (a PATH entry into `~/.nvm`).
  - `installer/pyenv`: `.bashrc` line 124 (`export PYENV_ROOT="$HOME/.pyenv"`).
  - `remove sublime-merge` without `--dry-run` refused with exit 2.
  - Each dry run took 0.6–1.6 s.
- **Packaging:** `scripts/package.sh` ran, then `scripts/test-package.sh dist` with the new dry-run checks passed in
  Debian stable, Ubuntu 24.04, Fedora, AlmaLinux 9, openSUSE Leap, openSUSE Tumbleweed and Arch Linux (x86_64).

## Known limits and follow-ups
- **Home dot-folders are not searched.** Folders like `~/.vim` and `~/.mozilla` rarely match a package name and often
  hold irreplaceable data. Folders whose name differs from every ID, such as `~/.config/Code` for `code` (matched)
  versus `~/.vscode` (not), are missed.
- **Some shell lines are missed.** Lines that reach the folder through a variable (`. "$NVM_DIR/nvm.sh"`) are not
  found.
- **No folder-level ownership check.** A system package's leftovers in the home folder belong to the user running
  Housekeeper only.
- **Nothing is removed yet.** Carrying out the plan, and moving leftovers the user chose to the Trash, is Phase 4
  (4.4 and 4.5).
