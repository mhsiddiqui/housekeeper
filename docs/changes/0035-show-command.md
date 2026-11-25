# 0035: `housekeeper show`: everything about one installed item

- **Date:** 2026-11-25
- **Branch / PR:** `feat/show-command`
- **Plan chunks:** 2.4 (`housekeeper show`), done; Phase 2 done
- **Commits:** 8d48d99, 355c6c7

## What changed
- **Two optional abilities** in core (`source.rs`), offered like `owner_lookup`:
  - `FileList` (`Source::file_list`): the files one package installed. dpkg (`dpkg-query --listfiles`, without `/.`
    and diversion notes), rpm (`rpm -ql`, without `(contains no files)`), pacman (the local database's `files`),
    pip (`RECORD`, paths resolved from site-packages, quoted CSV fields) and cargo (its binaries under cargo's home:
    every crate shares `~/.cargo/bin`, so its location says nothing).
  - `Dependants` (`Source::dependant_lookup`): the installed packages of the same source that directly need one
    package. dpkg (Depends and Pre-Depends, alternatives included, against the package's name and Provides; one
    `dpkg-query --show`), rpm (`rpm -q --provides`, then one `rpm -q --whatrequires` over those capabilities), pacman
    (`%DEPENDS%` of every local entry against the target's `%NAME%` and `%PROVIDES%`), Flatpak (apps whose runtime is
    the target runtime) and Snap (snaps whose `base` is the target, and established `content` connections to it,
    from snapd's `/v2/connections`). Optional dependencies do not count. The accessor is `dependant_lookup`, so it does
    not clash with the trait's `dependants` method.
- **`SourceInfo::evidence`**: how each source knows what is installed ("dpkg's database, through
  `dpkg-query --show`"), set by all 13 sources and shown in `sources --json`.
- **Core `Housekeeper::show(query)`** (`show.rs`): scans, then finds the item by `source:id`, then by id or name, then
  by name or title ignoring case. It returns `Lookup::Found(Details)`, `Several` or `NotFound`. `Details` holds the
  package, its source, its other installs and what this install shares with them, its files (`Recorded`, or
  `Location` when the source has no `FileList`: everything in the install location, symlinked folders listed but not
  entered, unreadable folders reported) with the count, the missing ones and the size, and `needed_by` (`None` when
  the source cannot tell). `group::keys` is shared so `shared` holds only this install's keys.
- **CLI `housekeeper show <ID_OR_NAME> [--files] [--json]`** (`crates/cli/src/show.rs`): one labelled line per known
  fact, `Needed by` capped at 10 with the rest counted, every file with `--files`. Exit codes 0 (found), 1 (several
  match; they are listed with their IDs), 2 (nothing). `who`'s reason/scope wording moved into a shared
  `circumstances` function.
- **`scripts/test-package.sh`**: on every distribution `housekeeper show housekeeper --files` must list
  `/usr/bin/housekeeper` through the real database query and say that nothing installed needs it.
- Docs: README (`show` section and usage line), architecture (two trait rows, the `Source` sketch, the `show`
  paragraph, D30), guide (both `Source` sketches), CHANGELOG, plan 2.4 ✅ and Phase 2 ✅, decision range D1–D30.

## Why
Plan chunk 2.4 and spec §1 ("details show why we believe it's installed"). Defaults agreed before starting: files are
summarised and listed only with `--files`; dependants are direct only, because the full set a removal would take is
the package manager's simulation in chunk 3.3. The per-source abilities were written in parallel by five subagents
against the traits above and reviewed and integrated here.

## How it was verified
- New tests: core `show.rs` (lookup by key, id, name and title; several matches; not found; files from the source
  with a folder, a symlink and a missing file; files from the install location with a symlinked folder not entered;
  dependants mapped to packages; `None` for a source without dependants; `shared` limited to this install's keys),
  dpkg, rpm, pacman, Flatpak, Snap, cargo and pip tests for their abilities (shapes from real output), and a CLI render
  test (labels, continuation lines, the 10-dependant cap, `--files`).
- `cargo fmt --all --check`, `rustfmt --check crates/linux/src/sources/*.rs`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo test --workspace --all-features` (7 + 25 + 118 tests), `cargo doc` with
  `-D warnings`, and `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu` pass.
  `cargo deny` is not installed here and was not run; no dependency was added.
- On this Ubuntu 26.04 machine: `show dpkg:sublime-merge` (72 files, also installed as a snap), `show code` (2,610
  files, the URL-handler launcher hidden), `show dpkg:libc6:amd64` (1,384 dependants: 10 shown and the rest counted),
  `show "Godot Engine"` (Flatpak, 54 files from its install folder), `show claude` (hand install sharing the command
  `claude` with two npm installs), `show cargo:rmux` (2 binaries), `show sublime-merge` (two matches, exit 1) and an
  unknown name (exit 2). `show code` took 0.37–0.45 s over three runs.
- In containers, with the static build: Fedora `show gmp` (epoch 1: `rpm -ql gmp-1:6.3.0-6.fc45.x86_64` works; needed
  by coreutils) and `show glibc` (dependants through `libc.so.6()(64bit)`-style capabilities); Arch `show glibc`
  (needed by acl, attr, audit, base…) and `show pacman`; Debian `show libc6` and `show bash`. Files that container
  images strip (docs, locales) show as "listed but missing".
- `scripts/package.sh` (nfpm from its container image) and `scripts/test-package.sh dist`, with the new `show` checks,
  in Debian stable, Ubuntu 24.04, Fedora (latest), AlmaLinux 9, openSUSE Leap, openSUSE Tumbleweed and Arch Linux
  containers, x86_64: all passed.

## Known limits and follow-ups
- Dependants are direct and within one source; what a removal would take in total is chunk 3.3.
- rpm requirements on files (`/bin/sh`) and Flatpak runtime extensions (GL drivers, locales) are not followed; Snap
  counts only bases and `content` connections.
- dpkg matches a multi-arch package by name, so `libc6:amd64` also lists packages that need `libc6:i386`.
- npm, pipx, uv, Homebrew, AppImage and hand installs cannot tell what depends on them ("not known").
- `show` reads every source, like `list`, to find one item.
