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
| 1.9 ✅ | Homebrew on Linux (formulae, taps, casks) and one-file providers: every file in `crates/linux/src/sources/` registers itself; native package managers declare their distributions |

## Phase 2: Identity and ownership 🚧
| Chunk | Delivers |
|---|---|
| 2.1 ✅ | Desktop entries: map `.desktop` files to packages; app vs library/runtime/system; display names, icons. Commands found in the native package database too; packages with neither are components, hidden by default (change 0033) |
| 2.2 | Group one app installed from several sources (AppStream ID, desktop ID, name) without merging the installs |
| 2.3 ✅ | Ownership: `OwnerLookup` names the owning packages on native sources (dpkg, rpm, pacman); `housekeeper who <command or path>` follows symlinks and names the source, package and evidence (change 0032) |
| 2.4 | `housekeeper show <id>`: details, files, launchers, dependants |

## Phase 3: Removal planning (no changes made) ⬜
| Chunk | Delivers |
|---|---|
| 3.1 | Package tools separate from the package databases (apt, dnf, zypper, rpm-ostree, pacman; decision D24); `Remover` capability and removal tiers 1–6 (own uninstaller → package manager → format convention → recipe → tracked manifest → unknown), each plan step marked as run by the user or by the root helper (D25) |
| 3.2 | Protected packages (dpkg Essential/Priority required, pacman `base`, dnf `protected_packages`) |
| 3.3 | Simulation through the package manager (`apt-get -s`, `dnf --assumeno`, `pacman -Rp`, `zypper -D`) to list everything that would go |
| 3.4 | Recipe format (TOML) with an allowlisted action vocabulary, holding both detection and removal of a script-installed tool (D26); the manual source's installer list moves into the first recipes |
| 3.5 | Leftover discovery (config, cache, data dirs) and `housekeeper remove --dry-run` |

## Phase 4: Making changes safely ⬜
| Chunk | Delivers |
|---|---|
| 4.1 | `housekeeper-helper`: privileged binary run through `pkexec`, polkit policy, one typed JSON operation on stdin, argv only, names checked against the package database (D25) |
| 4.2 | Operation queue: one change per backend at a time, package-manager lock detection and waiting, cancellation |
| 4.3 | History log (JSONL under `$XDG_STATE_HOME/housekeeper`) |
| 4.4 | Leftovers go to the freedesktop Trash, never `rm -rf` |
| 4.5 | `housekeeper remove` executes the reviewed plan |

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
| 6.1 | Tauri 2 shell, strict CSP, no remote scripts, minimal capabilities; port the mockup's Library view |
| 6.2 | Install from file (drag and drop), Activity, History views |
| 6.3 | First-run setup, settings, accessibility, light/dark themes |
| 6.4 | Desktop app packaging (the CLI is already packaged since 0.4): polkit policy and helper in `.deb`/`.rpm`/Arch, AUR `PKGBUILD`, AppImage (portable mode) |

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
Storage view, unused apps, caches and orphaned dependencies, default apps and startup apps.

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
