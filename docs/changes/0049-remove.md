# 0049: `housekeeper remove` carries out the reviewed plan

- **Date:** 2026-11-26
- **Branch / PR:** `feat/remove`, #39
- **Plan chunks:** 4.5, done; Phase 4 done
- **Commits:** 5c08f7f, 8c78a44, 9185318, d92b548

## What changed
- **New core module `carry.rs`** (decision D44).
  - `Housekeeper::still_as_reviewed(&Details)` plans again from a fresh scan. It returns why not when any of these
    changed:
    - the item is gone;
    - its version changed;
    - it is now protected;
    - the plan differs (steps, blockers, simulation).
  - `Housekeeper::remove(&Details, &[Leftover], &Cancel, events)` refuses a blocked or empty plan. Otherwise it runs
    the steps in order as one operation-queue job, whose backend is the plan's package tool or else its source:
    - `Run` steps run as the user, with a one-hour limit;
    - `Trash` steps run as the user (`core::trash`) or, for system paths, through the helper;
    - `RemovePackages` steps run through the helper.

    The first failure stops the rest. The chosen leftovers go to the Trash only when every step was done
    (`trash_leftover`, D37). One `HistoryEntry` is written. The result is a `Removal`.
  - A leftover that changed since it was found is recorded as skipped and does not fail the removal.
- **Root helper** (decision D40).
  - New operation `trash { path }`. It refuses a path that:
    - is not a plain absolute path at least two folders deep;
    - holds a symbolic link;
    - is or holds a system folder (`Platform::system_paths`);
    - is inside a home folder (`Platform::home_paths`);
    - is missing;
    - is owned by a package (the native source's `OwnerLookup`).

    Otherwise the path goes to root's Trash.
  - New `RootHelper` trait (`Platform::root_helper`). On Linux it is `Pkexec`: `pkexec
    /usr/lib/housekeeper/housekeeper-helper` with the request on stdin, or the helper directly when Housekeeper already
    runs as root. Only the installed helper is used.
  - pkexec's exit codes 126 (password not given) and 127 (not authorized) become plain messages.
  - `helper::effective_uid` is shared by the helper binary and `Pkexec`.
- **`Cmd` gains `stdin`**, which `SystemRunner` feeds from a thread and then closes.
- **CLI `housekeeper remove <id>`:** `--yes`/`-y`, `--leftovers caches|none|all` (default `caches`), `--json` (the
  history entry), and `--dry-run` as before.
  - It shows the plan, asks `[y/N]` on a terminal, and without a terminal does nothing unless `--yes` is given.
  - It checks again (D37), prints each `Waiting for …` and `Removing …`, then `done:`, `failed:` or `not done:` per step.
  - Exit codes: 0 removed, 1 several items match, 2 nothing matches, 3 nothing changed, 4 a step failed.
  - Lookup warnings are now printed before acting. The lookup no longer holds the stderr lock while a removal runs:
    the queue's threads print to stderr too.
- **`scripts/test-package.sh`:**
  - `housekeeper remove` without a terminal and without `--yes` exits 3 and changes nothing.
  - `housekeeper remove --yes --leftovers all housekeeper` then removes the package through the helper with the planned
    command, on every distribution. The test checks that the CLI and the helper are gone, the settings folder is in
    the Trash with its `.trashinfo`, and the history records the removal as done.
  - This replaces 0045's direct helper call.
- **Docs:**
  - new decision D44; D40 gains the trash operation;
  - architecture: the `RootHelper` row and a paragraph on carrying out a plan; D1–D44 in AGENTS.md, docs/README.md and
    the guide;
  - README: the usage line, a "Removing an app" section with a real run, the helper paragraph, the roadmap;
  - CHANGELOG: unreleased entries brought up to date, and the 0043 exit-code line removed, since 3 now means "nothing
    changed";
  - plan: 4.5 ✅, Phase 4 ✅.

## Why
Plan chunk 4.5 closes Phase 4: Housekeeper can now remove what it lists, the way each installer intended. It uses the
helper (4.1), the queue (4.2), the history (4.3) and the Trash (4.4), within decisions D37, D38, D40 and D41.

Trashing system paths was promised to the helper in D40. It arrives here because only plan steps need it (AppImages in
`/opt`, recipe folders outside home).

## How it was verified
- **New tests:**
  - core `carry.rs`, with a stub source and a fake root helper:
    - a plan of a user command and a package removal runs both, gives the helper only the tool and names, trashes the
      preselected cache, and writes the entry to the history;
    - a helper failure stops the rest, keeps the leftovers, and records the last stderr line as the reason;
    - a blocked plan asks nothing and runs nothing;
    - a changed version fails `still_as_reviewed`.
  - core `helper.rs`:
    - `/opt/Tool.AppImage` is trashed;
    - a relative path, `..`, `/opt`, `/usr/share`, a file in `/home`, a path through a symbolic link and a missing path
      are refused, and nothing is moved;
    - the effective-uid test moved here from the helper binary.
  - core `command.rs`: stdin is fed when given and closed otherwise.
  - Linux `elevate.rs`:
    - a user goes through `pkexec`, root runs the helper directly;
    - a dismissed prompt (126) and a missing helper give plain messages.
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (8 + 64 + 1 + 153)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - the Windows check of core and platform
  - `cargo deny` was not run (not installed); no dependency was added.
- **Packaging:** `scripts/package.sh` (nfpm from the `goreleaser/nfpm` image), then the updated
  `scripts/test-package.sh dist`, passed in Debian stable, Fedora latest, openSUSE Leap latest and Arch Linux (x86_64).
  Each run ended with Housekeeper removing its own package through the helper (`apt-get`, `dnf … --disablerepo=*`,
  `zypper … --no-remote`, `pacman`), its settings in the Trash and the history saying `done`.
- **A Debian container by hand:** `housekeeper remove --yes housekeeper` printed the plan, "Removing housekeeper…",
  `done:` for the apt step and for moving `~/.cache/housekeeper` to the Trash, and exited 0. `history.jsonl` held the
  entry with both steps done.
- **On this Ubuntu 26.04 machine, as my user:**
  - `pipx install cowsay`, then `housekeeper remove pipx:cowsay < /dev/null` exited 3 ("add --yes…").
  - `housekeeper remove --yes pipx:cowsay` ran `pipx uninstall cowsay` (`done:`), `cowsay` was gone,
    `housekeeper history` listed it as done, and a third run exited 2 (nothing matches).
  - `housekeeper remove --yes --leftovers none dpkg:sublime-merge`, with no helper installed here, failed the apt step
    with "Housekeeper's root helper is not installed (/usr/lib/housekeeper/housekeeper-helper)…". Sublime Merge stayed
    installed.
- **Not tried:** the `pkexec` password dialog on a desktop. It needs the packages installed with root on this machine;
  the containers run as root and use the helper directly.

## Known limits and follow-ups
- **Ctrl-C** while a package manager runs reaches it, as when it is run by hand. Housekeeper does not catch signals:
  that would need a signal-handling dependency, and putting the child in its own process group would stop `pkexec`'s
  terminal prompt.
- **One item per command.** Removing several items at once (the queue supports it) is left to the desktop app's
  selection.
- **Restarts.** rpm-ostree removals take effect after a restart; the CLI shows the plan's note but does not offer to
  restart.
- **The helper's trash** uses root's home trash, which only an administrator can browse to restore from.
