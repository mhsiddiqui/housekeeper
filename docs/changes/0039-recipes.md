# 0039: Recipes for script-installed tools

- **Date:** 2026-11-26
- **Branch / PR:** `feat/recipes`, PR not opened yet
- **Plan chunks:** 3.4 (recipe format), done
- **Commits:** 296e060, 4bd5820

## What changed
- **`crates/linux/recipes/*.toml`**: one reviewed file per script-installed tool, named by the tool's key. Each file
  has a `title`, a `[detect]` section and an optional `[remove]` section.
  - `[detect]`: candidate `roots`, each `{ var, default }`, `{ home }` or an absolute `{ path }`; a `marker` file; and
    an optional `marker_root`.
  - `[remove]`: exactly one of two actions.
    - `uninstaller`: a program inside the install or the marker's root, with fixed arguments. Its plan tier is
      "own uninstaller" and it runs as the user.
    - `trash`: paths inside the install root. Its plan tier is "recipe". The user trashes paths in the home folder;
      the root helper trashes paths elsewhere.
  - The ten tools from the old `INSTALLERS` table moved here: rustup, nvm, pyenv, Atuin, Bun, Deno, SDKMAN!, Homebrew,
    Volta and Oh My Zsh.
  - rustup runs `rustup self uninstall -y`. Seven tools move their folder to the Trash. Homebrew and Oh My Zsh have no
    `[remove]` section, and each file's comment says why.
- **`build.rs`** lists `recipes/` and compiles each file in with `include_str!` (`RECIPE_FILES`), so adding a recipe
  edits no list. File names must be lowercase letters, digits and `-`. The source-listing code is shared with the
  existing generation.
- **`crates/linux/src/recipe.rs`** parses the files once (`LazyLock`) and checks them:
  - Every table rejects unknown keys, so an action outside the vocabulary is an error rather than silently ignored.
  - Every relative path must not be absolute and must not contain `..`.
  - `[remove]` holds exactly one action.
  - `from = "marker-root"` requires a `marker_root`.
  - When planning, a trash path that is the home folder, an ancestor of it or `/` blocks the plan instead of being
    trashed. This can happen when a variable points the root there.
- **Manual source**:
  - It lists what the recipes find, with ids, titles, scopes and locations unchanged from before.
  - It plans removals from the recipes.
  - A recipe that does not parse becomes a scan warning.
  - The Rust `Root`, `Installer` and `INSTALLERS` code is gone, along with the hard-coded rustup remover.
- **Docs**:
  - Architecture: a "Recipes" section, D26 updated to say the move is done, and new decision D34.
  - Guide: corrected the claim that recipes could be added without a release; added a file-table row.
  - Also updated: README (the Removal bullet), CHANGELOG, AGENTS.md (composition rule, D1–D34), docs/README and
    plan 3.4 ✅.

## Why
Plan chunk 3.4, decisions D8 and D26. Before this change a script-installed tool was described twice: its detection
was in Rust and its removal was not recorded anywhere. A recipe deletes things, so D34 limits what one can say:

- two actions only;
- paths confined to the install;
- the home folder is never trashed;
- uninstallers run as the user, and the root helper only trashes system paths (D25);
- the files are compiled in and reviewed like code.

No new dependency: `toml` was already used by the uv source.

## How it was verified
- **New tests** (`recipe.rs`):
  - Every built-in recipe parses.
  - Parsing rejects:
    - a root escaping with `..`;
    - a relative `path` root;
    - trash paths with `..` or absolute;
    - an unknown action (`run = "rm -rf ~"`);
    - an empty trash list;
    - `marker-root` without a `marker_root`.
  - rustup: found via `RUSTUP_HOME`, its uninstaller run from a custom `CARGO_HOME`.
  - Bun: trashes `~/.bun` as the user, shown without a trailing slash. A root equal to the home folder is blocked,
    with no steps.
  - nvm under `/srv/node` is trashed by the helper.
  - Homebrew plans "unknown".
  - The existing manual-source tests pass unchanged: detection, ids, titles, scopes and locations; rustup's plan
    through `ManualSource`; hand installs "unknown".
- **Local checks:**
  - Passed:
    - `cargo fmt --all --check`
    - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
    - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    - `cargo test --workspace --all-features` (7 + 28 + 147)
    - `cargo doc` with `-D warnings`
    - `cargo +1.85 check --workspace --all-targets --all-features`
  - `cargo deny` was not run, because it is not installed here.
- **This Ubuntu 26.04 machine** (`show`):
  - `manual:installer/rustup`: "with its own uninstaller", `~/.cargo/bin/rustup self uninstall -y` as you.
  - `installer/nvm`, `installer/bun` and `installer/pyenv`: "with a reviewed recipe", each moving its folder to the
    Trash as you.
  - `show --json` prints the trash step.
- **Debian container, static musl build:** with a fake `~/.bun/bin/bun`, `show installer/bun` plans `/root/.bun` to
  the Trash, so the recipes are inside the static binary.
- **Packaging:** `scripts/package.sh` and `scripts/test-package.sh dist` passed in Debian stable, Ubuntu 24.04,
  Fedora, AlmaLinux 9, openSUSE Leap, openSUSE Tumbleweed and Arch Linux (x86_64).

## Known limits and follow-ups
- **Shell startup files are not edited.** nvm and SDKMAN! guard their lines, but pyenv's `eval "$(pyenv init -)"` and
  Deno's `. "$HOME/.deno/env"` will print errors until they are removed. Editing these files is Phase 8; listing
  them as leftovers belongs to 3.5.
- **Data outside the install folder stays**, for example Atuin's history in `~/.local/share/atuin`. That is leftover
  discovery (3.5).
- **Homebrew and Oh My Zsh stay "not known".** Homebrew's own uninstaller is a downloaded script. Oh My Zsh needs
  `~/.zshrc` restored.
- **A tool's own uninstaller runs as the user even when its files are outside the home folder.** If it lacks
  permission it fails; it is never escalated.
