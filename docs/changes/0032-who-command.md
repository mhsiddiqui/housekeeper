# 0032: `housekeeper who`: which source and package a command or file comes from

- **Date:** 2026-11-24
- **Branch / PR:** `feat/who-command`
- **Plan chunks:** 2.3 (ownership), done
- **Commits:** fa8e0dd, d8f1acb, a253a17

## What changed
- **`OwnerLookup::owners`** (was `owned_paths`) returns, for each path asked about, the packages that own it, not
  just whether it is owned. dpkg reads them from `dpkg-query --search` (`pkg[, pkg2]: path`, `:arch` kept), rpm from
  `rpm -qf --queryformat '[%{=NAME}\t%{FILENAMES}\n]'` (the name repeated beside each file), pacman from
  `pacman -Qo` (`path is owned by pkg version`). The manual source uses the paths only, as before.
- **Core `Housekeeper::who(query)`** (`crates/core/src/who.rs`): finds the command on the `PATH` (every copy, one
  per real file so `/bin/x` and `/usr/bin/x` count once on merged-`/usr` systems) or the absolute path, follows its
  symlinks (and a linked directory on the way, through the new `Host::canonical`), scans every source, and returns
  the owner of each copy with its evidence:
  1. a package database lists the file (`ListedFile`, with every owning package; a shared directory names none);
  2. the file is inside a package's install location (`InsideInstall`); the deepest location along the chain wins,
     so an npm link in nvm's `bin` belongs to the npm package, and packages sharing a location
     (`~/.cargo/bin`) are told apart by their commands;
  3. the file is inside a folder a source installs into (`InsideFolder`, from `install_roots`), so `/snap/bin/firefox`
     is the Firefox snap even though the link leads to `/usr/bin/snap`, which the snapd package owns.
  The first file in the chain that one of these accounts for decides. Unreadable sources become warnings.
- **`Host::file_type`** and **`Host::canonical`**; `Host` now follows up to 40 symlinks, as Linux does.
- **CLI `housekeeper who <COMMAND_OR_PATH> [--json]`**: the chain, `Owner:`, `Because:` and `Hidden:` lines;
  relative paths are taken from the current directory. Exit codes 0 (owner found), 1 (nobody), 2 (not found).
- **`scripts/test-package.sh`**: on every supported distribution, `housekeeper who housekeeper` must say that the
  native package database lists `/usr/bin/housekeeper` as a file of the `housekeeper` package.
- Docs: README (usage and a "Who installed this?" section), architecture (`OwnerLookup` row, how `who` decides),
  guide, CHANGELOG, plan 2.3 ✅.

## Why
Plan chunk 2.3 and spec §5 ("Who installed this?"). The design was agreed beforehand: resolve like `command -v`,
follow symlinks, ask every source, show the evidence and the copies hidden on the `PATH`. Adopting an unowned file
as a tracked manual install (spec §5) writes to disk, so it waits for Phase 4.

## How it was verified
- New tests in `crates/core/src/who.rs` (Fixture and stub sources): symlink into a pipx venv; first `PATH` match
  runs and others are hidden, with merged-`/usr` duplicates removed; an alternatives chain ending at a dpkg file;
  an npm link inside nvm; packages sharing `~/.cargo/bin`; a snap link that leads to snapd's binary; unowned file,
  unknown command, missing path and an unreadable source. Owner tests for dpkg, rpm and pacman now check the package
  names (rpm's output shape taken from `rpm -qf` in a Fedora container). A render test for the CLI text.
- `cargo fmt --all --check`, `rustfmt --check crates/linux/src/sources/*.rs`, `cargo clippy … -D warnings`,
  `cargo test --workspace --all-features` (5 + 18 + 97 tests), `cargo doc` with `-D warnings`, and
  `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu` pass.
- On this Ubuntu 26.04 machine: `who git`, `vim` (alternatives chain), `sh`, `code`, `pipx`, `/etc/os-release`
  (dpkg); `node` (nvm, with `/usr/bin/node` hidden), `python3` (pyenv, with Debian's hidden), `claude` (hand install,
  with the npm copy in nvm hidden), `uv` (hand install); `java` in `/mnt/data/jdk` (nobody, exit 1); `./README.md`
  (relative path, nobody); an unknown command (exit 2).
- A static musl build in Fedora, Arch Linux, openSUSE Tumbleweed and Debian containers: `who bash` names the bash
  package through rpm, pacman, rpm and dpkg; `who /etc` names `filesystem` on rpm and the shared owners on Arch and
  Debian; `who /usr/local/bin/housekeeper` is a hand install.

## Known limits and follow-ups
- `who cargo` and `who rustup` (rustup's proxies in `~/.cargo/bin`) answer "cargo install, though none of its
  packages lists cargo": the rustup installer the manual source detects lives in `~/.rustup`, not where its proxies
  are. Describing script installers in recipes (D26, Phase 3.4) can record where their commands go.
- A file outside every folder Housekeeper reads and not in a package database is "nobody" (exit 1), even when a
  script put it there; tracking such installs is Phase 11.
