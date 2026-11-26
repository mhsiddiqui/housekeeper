# Housekeeper build plan

Housekeeper is built in small chunks. Each chunk is one reviewable change that compiles, passes CI, and leaves the
tool usable. Later phases only add capabilities; they never require rewriting earlier ones (see
[architecture](../architecture/README.md)).

Status: ✅ done · 🚧 in progress · ⬜ not started.

How work is done (see [CONTRIBUTING.md](../../CONTRIBUTING.md) and [AGENTS.md](../../AGENTS.md)): every chunk gets its
own branch and pull request; CI must pass before merging; each chunk adds a [change record](../changes/README.md) and
updates the docs it affects (this plan, README, CHANGELOG, architecture).

## Phase 0: Foundation ✅
| Chunk | Delivers |
|---|---|
| 0.1 ✅ | Cargo workspace (`core`, `linux`, `cli`), licence (GPL-3.0-or-later since change 0018; MIT before), README, docs, mockup (now under `docs/design/mockup/`) |
| 0.2 ✅ | CI: `cargo fmt --check`, `clippy -D warnings`, tests |
| 0.3 ✅ | Full test pipeline: lint, tests, minimum Rust version (1.85), docs, dependency audit (`cargo deny`), packages installed and removed on every supported distribution for x86_64 and aarch64, Silverblue image (29 jobs) |
| 0.4 ✅ | Releases: publishing a GitHub release builds static `.deb`, `.rpm`, Arch package and tarball for x86_64 and aarch64, tests them, and attaches them with `SHA256SUMS`. Build and tests run on every pull request; first published as [v0.1.0](https://github.com/mhsiddiqui/housekeeper/releases/tag/v0.1.0) (change 0031) |
| 0.5 ✅ | Merge gate: a single `ci-ok` check that is green only when every CI job passed, required by a ruleset on `main` (no bypass, branches up to date) |

## Phase 1: Inventory (read-only) ✅
Goal: `housekeeper list` shows every installed app on Ubuntu/Debian, Fedora, openSUSE, Arch and Silverblue, from every
source, without root and without changing anything.

| Chunk | Delivers |
|---|---|
| 1.1 ✅ | Core model (`Package`, `SourceInfo`), `Source` and `Platform` traits, `Host` (root prefix, home, env), `CommandRunner` with timeouts, parallel `Inventory` scan |
| 1.2 ✅ | Linux platform: `/etc/os-release` parsing, distro family, image-based detection, native backend selection; dpkg source |
| 1.3 ✅ | rpm source (incl. rpm-ostree layered packages), pacman source (local database files) |
| 1.4 ✅ | Flatpak source (CLI columns), Snap source (snapd REST API over `/run/snapd.socket`) |
| 1.5 ✅ | npm (all nvm/fnm/system prefixes), pipx (venv metadata), `uv tool`, `pip install --user`, cargo (`.crates2.json`) sources |
| 1.6 ✅ | AppImage source (magic bytes), manual source (`/opt`, `/usr/local/bin`, `~/.local/bin`, symlink farms grouped by install folder; native-owned paths left out), known script installers (rustup, nvm, pyenv, atuin, bun, deno, sdkman, Homebrew, Volta, oh-my-zsh) |
| 1.7 ✅ | CLI: `housekeeper sources`, `housekeeper list` (table/JSON, filters) |
| 1.8 ✅ | CI smoke test inside Debian, Ubuntu, Fedora, openSUSE Tumbleweed and Arch containers |
| 1.9 ✅ | Homebrew on Linux (formulae, taps, casks) and one-file providers: every file in `crates/platform/linux/src/sources/` registers itself; native package managers declare their distributions |

## Phase 2: Identity and ownership ✅
| Chunk | Delivers |
|---|---|
| 2.1 ✅ | Desktop entries: map `.desktop` files to packages; app vs library/runtime/system; display names, icons. Commands found in the native package database too; packages with neither are components, hidden by default (change 0033) |
| 2.2 ✅ | Group one app installed from several sources (AppStream ID, desktop ID, name) without merging the installs; also by command, and twice from one source; `housekeeper list --duplicates` (change 0034) |
| 2.3 ✅ | Ownership: `OwnerLookup` names the owning packages on native sources (dpkg, rpm, pacman); `housekeeper who <command or path>` follows symlinks and names the source, package and evidence (change 0032) |
| 2.4 ✅ | `housekeeper show <id>`: details, files, launchers, dependants; how the source knows it is installed, other installs of the same app (change 0035) |

## Phase 3: Removal planning (no changes made) ✅
| Chunk | Delivers |
|---|---|
| 3.1 ✅ | Package tools separate from the package databases (apt, dnf, zypper, rpm-ostree, pacman; decision D24); `Remover` capability and removal tiers 1–6 (own uninstaller → package manager → format convention → recipe → tracked manifest → unknown), each plan step marked as run by the user or by the root helper (D25); plans shown by `housekeeper show` (change 0036) |
| 3.2 ✅ | Protected packages (dpkg Essential/Protected/Priority required, pacman `base` and its dependencies, dnf `protected_packages`, the running kernel) block the plan, with the reason (change 0037) |
| 3.3 ✅ | Simulation without root of everything a removal would take: `apt-get --simulate`, `pacman --remove --print`, and for dnf and zypper (whose own simulation needs root on most releases) the RPM database's chain of what requires the package (D33); refusals and protected packages in the result block the plan (change 0038) |
| 3.4 ✅ | Recipe format (TOML) with an allowlisted action vocabulary (own uninstaller or trash inside the install), holding both detection and removal of a script-installed tool (D26, D34); the manual source's installer list moved into the first ten recipes (change 0039) |
| 3.5 ✅ | Leftover discovery (folders named after the item in the XDG config, cache, data and state folders and Flatpak's `~/.var/app`; shell startup lines that load it; D36) and `housekeeper remove --dry-run` (change 0041) |
| 3.6 ✅ | Leftover hardening, from a review of 3.5: (a) leftovers only in folders the item can use (a Flatpak's own `~/.var/app`, none in `~/.config` for sandboxed apps), folders every program shares never one item's (`dconf`), language packages' folders flagged as possibly used by unscanned copies, shell lines not matched inside longer paths (change 0042); (b) `show` stops measuring leftovers, `remove --dry-run` measures them within a budget of 50,000 files and folders per folder, and `remove` without `--dry-run` exits with 3 (change 0043) |

## Phase 4: Making changes safely 🚧
| Chunk | Delivers |
|---|---|
| 4.1 ✅ | `housekeeper-helper`: privileged binary run through `pkexec`, polkit policy, one typed JSON operation on stdin, argv only, names checked against the package database (D25, D40); packaged with the CLI at `/usr/lib/housekeeper/` and tested by removing Housekeeper's own package on every distribution (change 0045) |
| 4.2 ✅ | Operation queue: one change per backend at a time, package-manager lock detection and waiting, cancellation (D41; change 0046) |
| 4.3 ✅ | History log (JSONL under `$XDG_STATE_HOME/housekeeper`), and a record of every item seen installed (key, name, AppStream and launcher IDs, source, when first and last seen), rewritten by each scan, so leftovers of apps removed outside Housekeeper can be named later (D39, D42); `housekeeper history` (change 0047) |
| 4.4 | Leftovers go to the freedesktop Trash, never `rm -rf`: only caches nothing else uses are preselected (D38), and each folder is checked again just before it moves (D37) |
| 4.5 | `housekeeper remove` executes the reviewed plan, checking each package again (version, protection, simulation) before anything changes (D37) |

## Phase 5: Install from file ⬜
| Chunk | Delivers |
|---|---|
| 5.1 | File type detection: `.deb`, `.rpm`, `.pkg.tar.zst`, `.flatpakref`, `.flatpak`, `.snap`, AppImage, archives, `.run`/`.sh` |
| 5.2 | Install plans per type (dependencies resolved by the native manager, `--dangerous` snaps flagged) |
| 5.3 | AppImage integration: move to `~/Applications`, desktop entry, icon; removal reverses it |
| 5.4 | Archives and scripts: run in the tracked mode (Phase 11) or refuse with an explanation |

## Phase 6: Desktop app (v1 release) ⬜
| Chunk | Delivers |
|---|---|
| 6.1 | Tauri 2 shell, strict CSP, no remote scripts, minimal capabilities; port the mockup's Library view, which opens on the last scan and refreshes it in the background (D39) |
| 6.2 | Install from file (drag and drop), Activity, History views |
| 6.3 | First-run setup, settings, accessibility, light/dark themes |
| 6.4 | Desktop app packaging (the CLI, the root helper and its polkit action are already packaged since 0.4 and 4.1): AUR `PKGBUILD`, AppImage (portable mode) |

**v1 = Phases 1–6**: see every app, remove it the right way, install downloaded files.

## Phase 7: Updates and holds ⬜
Per-source update checks, holds/pins, upstream release notes, update all with one queue.

## Phase 8: Environment ⬜
PATH entries, environment variables, aliases: read every shell startup file; edits go into a Housekeeper-managed
block with a backup.

## Phase 9: Discover ⬜
Flathub API, snapd `/v2/find`, distro AppStream (`appstreamcli`). Fetched by the backend, cached, HTML turned into
sanitised text before it reaches the UI; offline snapshot fallback.

## Phase 10: Housekeeping ⬜
Storage view, unused apps, caches and orphaned dependencies, default apps and startup apps; leftovers of apps that are
no longer installed, named from the record of items seen (4.3).

## Phase 11: Tracked terminal ⬜
Integrated terminal that records what an install script changed (file snapshot diff) and writes a manifest, so the
install can be removed later (removal tier 5).

## Phase 12: Backup and other platforms ⬜
Optional "Back up my apps" to a secret GitHub Gist, further platforms (macOS: Homebrew and `.app`; Windows: winget) as
new platform crates. Managing other computers over SSH is not planned
([decision D22](../architecture/README.md#decisions)).

## Later: Website ⬜
A project website (what it does, install instructions, downloads from GitHub releases). Not started; planned once the
desktop app exists.
