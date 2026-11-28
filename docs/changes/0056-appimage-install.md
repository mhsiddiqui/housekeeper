# 0056: Install an AppImage into the menu, and remove it the same way

- **Date:** 2026-11-29
- **Branch / PR:** `feat/appimage-integration`, #46
- **Plan chunks:** 5.3 (see [plan](../planning/plan.md))
- **Commits:** 61eeb34, 548024f, e628209, c7100b7, 85b49d1, e923f31 (completes 548024f, which left out the new
  files)

## What changed
- **Decision D48** (new): how an AppImage is installed and removed. The architecture's installer-files section and the
  `FileInstaller` row say so too.
- **Core:**
  - Two new steps: `Action::Move { from, to }` (`Step::move_file`) and `Action::WriteFile { path, contents }`
    (`Step::write_file`).
    - `Move` is a rename, never over something that is there. It makes the target's folder.
    - `WriteFile` is `Host::create_new`, so never over a file that is there.
    - Both run as the user. The helper is not asked to do either.
  - `FileInstaller::replaces_installed` (default `true`).
    - When it is `false`, the install plan lists every other installed copy of the app as a caution, with the
      `housekeeper remove` that removes it, instead of setting `replaces`.
    - For any installer, the file being installed no longer counts as an installed copy when its source lists it where
      it is (an AppImage in `~/Downloads`).
- **Linux:**
  - `squashfs.rs` (new):
    - `elf_end` finds where a type 2 AppImage's image starts: after its ELF section header table.
    - `list` reads the image with `unsquashfs -o <offset> -lls`.
    - `cat` reads one file of it with `unsquashfs -cat`.
    - `resolve` follows links inside the image to the regular file they end at, and never out of it.
  - `desktop.rs`:
    - `appimage_entry` makes the menu entry from the one the AppImage carries. Every `Exec` starts the AppImage, quoted
      as the specification asks with `%` doubled, with the arguments it had, past `env`. `TryExec` and
      `DBusActivatable` go, and `Icon` is the extracted icon. Without an entry, a plain one named after the file.
    - `icon_of` reads the icon an entry names.
    - `program` now reads `%%` as `%`.
  - AppImage source:
    - The install plan, as the user: `Move` into `~/Applications` (unless it is there already), `chmod u+x`, then
      `mkdir -p` and `unsquashfs -d` to take its icon out into `$XDG_DATA_HOME/icons/appimage/<name>`, then `WriteFile`
      of `$XDG_DATA_HOME/applications/appimage-<name>.desktop`.
    - The icon is the one the carried entry names (`.png`, `.svg`, `.svgz`, `.xpm` at the image's root), else
      `.DirIcon`.
    - Blockers: a file, entry or icon folder already there, or the AppImage on another file system than
      `~/Applications`.
    - Cautions: no `unsquashfs`, or a type 1 AppImage (plain entry, no icon).
    - The remover also trashes the `icons/appimage/<name>` folder of each launcher that starts the AppImage.
- **CLI:** steps print as `move A to B` and `write P`; `install --help` names AppImages.
- **Docs:**
  - architecture (D48, installer files, `FileInstaller` row) and D1–D48 in AGENTS.md, docs/README.md and the guide;
  - plan 5.3 ✅, CHANGELOG, and the README (status, "What is this file?", "Installing a downloaded file", roadmap).

## Why
Plan chunk 5.3: the spec installs an AppImage into `~/Applications` with `chmod +x`, a menu entry and its icon, and
removes it by the same convention (removal tier 3). Housekeeper writes nothing outside its own state except through a
reviewed plan (D42), so moving the file and writing the entry are steps in the plan, shown before anything happens.
The entry's `Icon` is an absolute path into a folder made for the app, so the removal finds what the install made.

## How it was verified
- `cargo fmt --all --check`, `rustfmt --check` on sources and tools, and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --workspace --all-features`: all pass (core 69, Linux 175, CLI 8, helper 1). New and changed tests:
  - where the image starts, for 64-bit little-endian and 32-bit big-endian headers;
  - links inside an image followed to the file they end at, and refused when they leave it;
  - a carried entry made to start an AppImage whose path has spaces, quotes, `$`, `\` and `%`, read back by the
    launcher scan;
  - a downloaded AppImage's plan: move, chmod, mkdir, unsquashfs of the icon its entry names, and the entry;
  - without `unsquashfs`: a plain entry, a caution, a blocker for a file of that name in `~/Applications`, and the
    other copy of the app as a caution (the file being installed is not counted);
  - removal trashes the icon folder made for the AppImage's own launcher, and not another app's;
  - an install's user steps move and write, but never over a file that is there.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps` and `cargo deny check`: clean.
- By hand on this machine (Ubuntu, unsquashfs 4.7.5), with `HOME` set to an empty temporary folder:
  - `housekeeper install --yes ~/Downloads/baandar_0.1.0_x86_64.AppImage` (a real type 2 AppImage) moved it into
    `~/Applications`, made it executable, took its icon out (followed from `baandar.png` to
    `usr/share/icons/hicolor/512x512/apps/baandar.png`) and wrote `appimage-baandar.desktop`.
  - `desktop-file-validate` accepted the entry. `housekeeper list --source appimage` listed it as `Baandar 0.1.0`.
  - `housekeeper remove --yes --leftovers none` moved the AppImage, the entry and the icon folder to the Trash.
    `housekeeper history` recorded the install and the removal as done.
  - The first run found that `unsquashfs -d` does not make the folders above its destination; the plan now has the
    `mkdir -p` step.

## Known limits and follow-ups
- Without `unsquashfs` the entry is plain and has no icon. Housekeeper does not read squashfs itself (D45, D48).
- A type 1 AppImage (an ISO 9660 image, long deprecated) gets the plain entry.
- Only the file's own `~/Applications` is used. Choosing the folder is a setting for the desktop app (6.3).
- Desktop environments that do not watch `~/.local/share/applications` may show the entry only after logging in
  again. Housekeeper does not run `update-desktop-database`.
- Not tried in CI, which has no AppImage to install; the unit tests and the run by hand above are the evidence.
- Archives and scripts (5.4) still have no plan.
