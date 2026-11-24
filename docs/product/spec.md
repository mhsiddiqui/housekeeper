# Housekeeper: one place to see, install, and remove every app, on any distro

Name: **Housekeeper** ("Every app, however it got here."). Target: every mainstream desktop Linux. The app detects the
distro and adapts to it. It does not assume Ubuntu. Build plan: [plan.md](../planning/plan.md); design decisions:
[architecture](../architecture/README.md).

## Problem

Linux software arrives through many channels. Each distro family has its own native package manager
(apt, dnf, zypper, pacman), and then there are universal formats (Flatpak, Snap, AppImage), developer
tools (npm -g, pipx, cargo), community repos (AUR, COPR, PPA), and "curl | sh" scripts. Each channel
has its own way to list, update, and remove things, and its own place where it leaves data behind.

## Product principles

1. **An easy front door, not a replacement.** Native tools (apt, dnf, pacman, Software Center, Discover, flatpak, npm…)
   stay the experts and keep all their options. Housekeeper makes the common jobs easy (see what's installed,
   install a downloaded file, remove something completely), and uses the native tools to do them.
   Anything installed outside the app still shows up.
2. **Adapt to the distro.** Detect it from `/etc/os-release` (`ID`, `ID_LIKE`, `VARIANT_ID`), then choose a
   native backend and enable only the sources that actually exist on the machine.
3. **Apps first, not packages.** By default, show what the user installed. Libraries, runtimes, and base
   snaps sit behind a toggle.
4. **Show every action before running it.** Each action shows its exact command first. Root actions go
   through polkit (`pkexec`). The UI never runs as root.
5. **Remove it the way the software says to.** Every removal uses the uninstall method that the software's installer
   provides (see "How uninstall works"). Housekeeper never invents its own removal. Deleting user data and
   config is a separate, opt-in step.

## Supported systems

| Family | Distros | Native backend | Installs files | "User-installed" query | Orphans |
|---|---|---|---|---|---|
| Debian | Debian, Ubuntu, Mint, Pop!_OS, Zorin, elementary | apt / dpkg | `.deb` | `apt-mark showmanual` | `apt autoremove` |
| Fedora / RHEL | Fedora, RHEL, Alma, Rocky, Nobara | dnf / rpm | `.rpm` | `dnf repoquery --userinstalled` | `dnf autoremove` |
| SUSE | openSUSE Tumbleweed / Leap | zypper / rpm | `.rpm` | `/var/lib/zypp/AutoInstalled` (inverse) | `zypper packages --unneeded` |
| Arch | Arch, EndeavourOS, Manjaro, CachyOS | pacman (+ AUR via paru/yay) | `.pkg.tar.zst`, `PKGBUILD` | `pacman -Qe`, foreign `pacman -Qm` | `pacman -Qdtq` |
| Image-based | Fedora Silverblue/Kinoite, Bazzite, Aurora, openSUSE Aeon | rpm-ostree / transactional-update | layering only | `rpm-ostree status --json` | `rpm-ostree cleanup` |
| Later | Alpine (apk), Void (xbps), Solus (eopkg), Gentoo (emerge), NixOS (nix) | via backend trait | | | |

NixOS is declarative, so "remove" means editing `configuration.nix` or using `nix profile remove`. It needs
its own mode. Target it last.

### Distro-specific rules the app must follow
- **Arch: no partial upgrades.** Never update a single repo package (`pacman -Sy pkg`). "Update" for any
  repo package means a full `pacman -Syu`. AUR helpers must run as the user, never with sudo. Show the
  PKGBUILD for review before each build.
- **Image-based systems:** native installs become rpm-ostree *layers* that need a reboot. Recommend Flatpak
  for apps and toolbox/distrobox for CLI tools. Mark entries as "after reboot". `dnf install` on these
  systems should be explained, not attempted.
- **Tumbleweed:** a full system update is `zypper dup`, not `zypper up`.
- **Snap** exists by default only on Ubuntu. Elsewhere, hide the source unless `snapd` is present. Linux
  Mint blocks snapd on purpose, so respect that.
- **Foreign package formats** (`.rpm` on Debian, `.deb` on Fedora, either on Arch) are refused. Offer
  equivalents instead, because converters (alien, debtap) break dependencies.

## How uninstall works

Rule: use the uninstall method the software's own installer provides. For each item, Housekeeper uses the first
method below that applies, and shows which one it picked (and its source link) before running it.

| # | Method | Used for | Examples |
|---|---|---|---|
| 1 | **The tool's own uninstaller** | Script-installed tools that ship one | `rustup self uninstall`, `uninstall_oh_my_zsh`, `sudo /opt/VirtualBox/uninstall.sh`, `nvidia-installer --uninstall`, Homebrew's `uninstall.sh` |
| 2 | **The package manager that installed it** | deb/rpm/pacman, Snap, Flatpak, npm, pnpm, bun, pipx, uv, cargo, gem, brew | `npm uninstall -g <pkg>`, `cargo uninstall <crate>`, `pipx uninstall <pkg>`, `uv tool uninstall <pkg>`, `pnpm rm -g <pkg>`, `bun remove -g <pkg>` |
| 3 | **The format's convention** (the format has no uninstaller) | AppImage, tarball extracted by Housekeeper | Delete the file plus the `.desktop` entry and icon that were created for it |
| 4 | **Uninstall recipe** (written from the tool's official docs) | Tools whose docs say "delete these folders" | nvm, bun, pyenv, `go install` binaries |
| 5 | **Tracked manifest** | Installs run in the tracked terminal / shell hook | The files and rc lines recorded at install time |
| 6 | **Unknown** | Nothing above matches | Show what was found and let the user decide. Offer "contribute a recipe" |

### npm, cargo, pipx and similar don't need per-package docs
Each of these managers has one standard uninstall command (method 2) that works for every package it installed.
The hard parts are elsewhere:
- **Use the right copy of the manager.** npm globals belong to one Node version (nvm/fnm/volta), so run that
  version's npm: `~/.nvm/versions/node/v22.18.0/bin/npm uninstall -g <pkg>` (`volta uninstall <pkg>` for Volta).
  On the Python side, keep pipx, `uv tool` and `pip --user` apart (`pip uninstall` leaves dependencies behind).
- **Leave alone what the manager doesn't own.** `cargo`, `rustc` and friends in `~/.cargo/bin` are rustup proxies,
  not `cargo install` crates, so never run `cargo uninstall` on them. `go install` has no uninstall: delete the
  binary, and use `go version -m <binary>` to identify which module it came from.
- **Find side effects the manager can't see.** These are covered by recipes and the opt-in data cleanup:
  - npm packages with install scripts (npm metadata marks them with `hasInstallScript`): warn that they may have
    created files elsewhere.
  - Config and data folders (e.g. `~/.claude`, `~/.gemini`).
  - Shell completions or rc lines the tool added.
  - systemd user services.
  - Self-updaters that moved the tool somewhere else.

### Uninstall recipes
- Same idea as Homebrew Cask's `uninstall`/`zap` stanzas and Scoop's `uninstaller` field.
- Stored as TOML in a public GitHub repo, shipped inside the app and refreshed from GitHub at no cost.
- Written once per tool, never per user. An AI can draft a recipe from the tool's official docs; a maintainer
  reviews and merges it. Docs are never read on the user's machine at uninstall time.
- Every recipe links to the doc it was written from, and the app shows that link next to the removal.
- Recipes only use a fixed set of actions (trash a path under allowed roots, remove lines an installer added, run a
  program inside the install). Never free shell.

```toml
# recipes/nvm.toml
id = "nvm"
detect.any_path = ["$NVM_DIR/nvm.sh", "~/.nvm/nvm.sh"]

[uninstall]
source = "https://github.com/nvm-sh/nvm#uninstalling--removal"
steps = [
  { remove_path = "$NVM_DIR" },
  { remove_rc_lines = { match = "NVM_DIR" } },  # the lines nvm's installer added
]

[data]  # only with "also delete user data"
paths = ["~/.npm"]

# recipes/npm/@anthropic-ai/claude-code.toml
id = "npm:@anthropic-ai/claude-code"
uninstall.method = "package-manager"  # npm uninstall -g, run with the owning Node version
data.paths = ["~/.claude", "~/.claude.json"]
```

Coverage: methods 2 and 3 handle most installs without any recipe. Recipes are needed for the long tail of
script-installed tools (start with the ~50 most common). Anything still unknown is labelled as unknown.

## Core features

### 1. Unified Library
- One list covering every source, with filters, search, sorting, the exact removal command, and two layouts:
  - **Columns** (default): app list, then details (location, update source, removal method, Home/Repo/Docs links),
    then a live web view of the project's site with how it was installed and how it will be removed.
  - **Table**, for sorting and selecting many items.
- Sidebar: Discover; this computer's Installed (split by source) and Updates; the Manage tools; and Categories.
- **Duplicate detection** (the same app from two sources).
- Details show **why we believe it's installed**: the ownership command used (`dpkg -S`, `rpm -qf`, `pacman -Qo`,
  `flatpak info`, manifest file, …).

### 2. Install: drag & drop + unified search
| Dropped file | Debian family | Fedora / SUSE | Arch | Image-based |
|---|---|---|---|---|
| `.deb` | `apt install ./f.deb` | refuse + alternatives | refuse + alternatives | refuse |
| `.rpm` | refuse + alternatives | `dnf`/`zypper install ./f.rpm` | refuse + alternatives | `rpm-ostree install` (warn: reboot, prefer Flatpak) |
| `.pkg.tar.zst` | refuse | refuse | `pacman -U` | refuse |
| `PKGBUILD` | refuse | refuse | review → `makepkg -si` | refuse |
| `.AppImage` | integrate (all distros: `~/Applications`, `chmod +x`, `.desktop`, icon) | ← | ← | ← |
| `.flatpakref` | `flatpak install --user` (all) | ← | ← | ← |
| `.snap` | `snap install --dangerous` | only if snapd present | ← | ← |
| `.tar.gz` / `.zip` | extract to `~/.local/opt`, launcher, manifest (all) | ← | ← | ← |
| `.sh` / `.run` | run in tracked terminal (all) | ← | ← | ← |

- Unified search across the native repos, AUR (on Arch), Flathub, Snap Store (if present), and npm.
  The recommendation depends on the distro: on image-based systems, recommend Flatpak.

### 3. Tracked terminal + shell hook
- Integrated PTY terminal (xterm.js + portable-pty), with OSC 133 markers for command boundaries.
- A **shell hook** (`eval "$(housekeeper shell-hook bash)"` in bash/zsh/fish) sends the same events from the
  user's own terminal (GNOME Terminal, Konsole, Ghostty, …), so tracking doesn't depend on our terminal.
- Classifier for every package-manager command (apt/dnf/zypper/pacman/paru/yay/rpm-ostree/snap/flatpak/npm/pipx/uv/cargo):
  re-query that source after the command finishes.
- Unmanaged installs (`curl | sh`, `make install`): snapshot-diff the usual locations before and after, save a
  manifest, and record edits to shell rc files so they can be reverted on removal.
- Background watcher: inotify on the package databases (`/var/lib/dpkg/status`, `/var/lib/rpm`,
  `/var/lib/pacman/local`, `/var/lib/flatpak`, snapd socket) keeps the Library live.

### 4. Repositories
- Lists PPAs, COPR, OBS home repos, vendor repos (Microsoft, Brave, Docker, …), AUR, Flathub/Fedora flatpak
  remotes, and chaotic-aur, together with the apps that come from each one.
- After removing the last app from a vendor repo, offer to remove the repo and its signing key.

### 5. "Who installed this?"
- Enter a command or path. The app resolves it with `command -v`, then asks each source who owns it.
- If nothing owns it, offer to **adopt** it as a tracked manual install.

### 6. Updates, Cleanup, History
- Updates across every source, following the distro's rules (Arch = full upgrade; image-based = new deployment + reboot).
- Cleanup: orphans, disabled snap revisions, old ostree deployments, unused flatpak runtimes, leftover
  config, broken `.desktop` entries, stray AppImages, unmerged `.pacnew`/`.rpmnew` files, package caches.
- History of every action with its command and result.

### 7. Export / migrate
- Export the app list to a file. Portable sources (Flatpak, npm, pipx, cargo, AppImage) reinstall as-is.
  Native packages are mapped to the target distro's names using **Repology** data, with a Flatpak fallback.
  This lets a user move from Ubuntu to Fedora or Arch and keep their apps.

### 8. Environment: variables, PATH, aliases
- **One view of every place that sets them:** `~/.bashrc`, `~/.profile`, `~/.zshrc`, fish `conf.d`,
  `~/.config/environment.d/*.conf` (apps started from the menu), `/etc/environment` and `/etc/profile.d` (all users).
  Each entry shows its file:line, which shells/apps get it, and who added it (linked to the app in the Library).
- **Where Housekeeper writes:** its own files, never scattered edits:
  - `~/.config/housekeeper/env.sh`: loaded by both bash and zsh. Housekeeper adds one `source` line to each rc file, once.
  - `~/.config/fish/conf.d/housekeeper.fish` (`set -gx`, `abbr`).
  - `~/.config/environment.d/60-housekeeper.conf` for apps started from the menu (read at login by systemd).
  - `/etc/environment` (through polkit) for every user.
  - It edits an existing line in place only when you edit that entry. Each change shows a diff first, makes a backup,
    and can be undone from History.
- **Secrets** (names like `*_KEY`, `*_TOKEN`) are stored in the system keyring (`secret-tool` / libsecret) and loaded with
  `$(secret-tool lookup …)`. They are never written to a plain-text file and are masked in the UI.
- **PATH editor:** shows the effective order, duplicates, and missing folders, plus "Which command runs?" (every copy of a command
  on PATH, and any alias that shadows it). Edits are staged and only written after review. Your own PATH lines are
  combined into one ordered, de-duplicated line. Lines added by installers (nvm, pyenv, cargo, atuin) are left in place.
- **Aliases:** add, edit, or remove. Warns when an alias hides a real command or uses deprecated tools (`fgrep`/`egrep`).
  Fish uses `abbr`.
- **Suggested fixes:**
  - Variables or aliases that only bash sees while zsh is installed.
  - Variables that IDEs need (`JAVA_HOME`, `ANDROID_HOME`) but that apps started from the menu can't see.
  - A global `LD_LIBRARY_PATH`.
  - Duplicate or missing PATH folders, and commands installed twice.
- **Tied to the rest of the app:**
  - `export`/`alias` typed in the tracked terminal offers "Save permanently".
  - `which`/`type`/`echo $VAR` use the same data.
  - Removing an app (e.g. nvm, Bun) also removes the variables, PATH entries, and aliases it added.

### 9. Safe removal
- Every removal shows **which uninstall method** is used (see "How uninstall works") and links to its source.
- **Dry run first** (`apt-get -s remove`, `dnf remove --assumeno`, `pacman -Rsp`, `zypper --dry-run`). The preview lists
  what will be removed, including surprises like `rustup self uninstall` deleting cargo-installed tools, and which packages
  become unneeded.
- **Protected items:** libraries the desktop needs, snap bases and Flatpak runtimes in use are refused with a reason.
- **User data goes to the Trash** (`gio trash`) by default. System folders (e.g. `/var/lib/docker`) can't go to the
  Trash, and the UI says so.
- npm globals are removed with the npm of the Node version that owns them.

### 10. Updates, holds and release notes
- **Version holds:** `apt-mark hold`, `dnf versionlock`, `zypper addlock`, pacman `IgnorePkg` (with a partial-upgrade
  warning), `snap refresh --hold`, `flatpak mask`. Sources without holds (npm, pipx, cargo, AppImage) are skipped by "Update all".
- **Updates for apps installed from downloaded files** (a .deb/.rpm without a repo, AppImages, vendor installers):
  check GitHub releases (cached, since unauthenticated API calls are limited to 60/hour) or the vendor's download page.
  The user can set the update source when it's unknown.
- **"What's new" before updating**, from the right place for each source: `apt changelog`, `dnf updateinfo`, AppStream
  release notes, GitHub releases, the Snap Store listing.

### 11. Unused apps
- "Last opened" comes from desktop usage data for GUI apps and from the program file's access time for CLI tools
  (best effort; unavailable when the disk is mounted with `noatime`).
- A Library filter for "not opened in 6+ months", a "Least recently used" sort, and a Cleanup card that selects them
  for review.

### 12. Bulk actions and the Activity queue
- Select several items (checkboxes, Space, Ctrl+A), then Remove / Update / Hold them together.
- Every install, update and removal goes through one queue: one operation at a time, "waiting for the package lock"
  when another updater is running, progress, cancel before start, full log, and Retry after a failure. Nothing is
  changed when a step fails.

### 13. Storage (things that aren't apps)
- **Plugins**, each removing items with its own tool's command:
  - **Containers:** Docker/Podman images, volumes and build cache; Distrobox containers.
  - **Development:** old Node versions (nvm), rustup toolchains, Python venvs, IDE extensions, Android SDK images, Gradle caches.
  - **Games:** Steam, Lutris, Heroic and Bottles games, plus Wine prefixes.
  - **System:** old kernels, the journal, user fonts, GNOME/KDE extensions.
- Community plugins: Homebrew, JetBrains Toolbox, Nix profile, mise/asdf, and anything else people need.

### 14. Defaults & startup
- Default app per file type (`xdg-mime default`), listing only apps whose `.desktop` file supports that type. Flags
  defaults that point to a removed app.
- Startup apps (`~/.config/autostart`) and background services (systemd user/system units): turn them on or off,
  and remove leftovers from uninstalled apps. Removing an app removes its startup entries.

### 15. Command line
- A CLI with the same engine (`list --json`, `which`, `remove`, `update --all`) for scripts and servers without a
  desktop.
- Housekeeper manages only the computer it runs on; other computers over SSH are not planned
  ([decision D22](../architecture/README.md#decisions)).

### 16. Settings, first run, accessibility, translations
- **First run:** explains what's scanned (read-only) and asks three opt-in questions: Trash, shell hook, and anonymous
  user count. It then shows the scan's progress for each source.
- **Settings:**
  - Turn sources and plugins on or off.
  - Choose which folders are scanned for AppImages.
  - Safety: Trash, and a snapshot before changes when Snapper/Timeshift is present.
  - Privacy: shows exactly what's sent.
  - Theme (system/dark/light) and language.
- **Keyboard:** `/` search, `j`/`k` move, Enter opens, Space selects, Delete removes, `?` lists shortcuts. Every
  control is reachable with Tab and labelled for screen readers.
- **Translations:** text lives in translation files, translated by the community (e.g. Weblate); right-to-left
  languages such as Urdu are supported.

### 17. Discover
- Home page:
  - A guide/editorial hero (e.g. "Flatpak, Snap or AppImage?").
  - A personal card (e.g. "Free up 29.5 GB" → Storage).
  - What's new in the app.
  - "For you" and "Recommended for you": worked out on this computer from what's installed; nothing is sent.
  - Popular apps for this distro, and handy command-line tools.
- Category pages use AppStream categories (AI, Audio & Video, Development, Education, Games, Graphics, Networking,
  Office, Science, System, Utilities): picks to install, plus what's already installed in that category.
- Install from a card always shows the available sources for this distro, recommends one (and says why), shows the
  exact command, and runs it through Activity. Script installers run tracked, so they can be removed later.
- **Store data (checked live on 2026-11-23):**
  - **Flathub API** `https://flathub.org/api/v2`. No key needed; 3,429 apps.
    - `GET /collection/popular`, `/collection/category/{Category}` (paged; each hit has `app_id`, name, summary,
      icon URL, monthly installs, verified flag).
    - `GET /appstream/{app_id}` for full details (description, screenshots, releases, links).
    - `POST /search` for search.
    - Install: `flatpak install flathub <app_id>`.
  - **Snap Store API** `https://api.snapcraft.io/v2/snaps/find?q=…` or `?category=featured`, sent with the header
    `Snap-Device-Series: 16`.
    - On machines with snapd, use the local socket instead: `GET /v2/find` on `/run/snapd.socket`. This is what
      Ubuntu's App Center does.
    - Install through snapd, which asks for the password with polkit.
    - App Center itself has no API; it's a front-end over snapd and the distro's AppStream data.
  - **Distro repositories:** the AppStream catalog the distro already installs locally. Read it with
    `appstreamcli search` / libappstream, so it works offline. Install with the native package manager.
  - **Offline Flathub catalog:** flatpak keeps it at `/var/lib/flatpak/appstream/flathub/x86_64/active/appstream.xml.gz`.
  - **Who calls the APIs:** the backend (Rust), never the web UI. Browsers block most of these endpoints (CORS);
    the backend has no such limit. Responses are cached on disk, so Discover opens instantly and works offline.
  - **Cost:** $0. Users' machines call the stores directly; no server of ours is involved. Store icons load from
    `dl.flathub.org` / `dashboard.snapcraft.io`.
- Editorial content (guides, "What's new") is plain files in the project's GitHub repo, so it costs nothing to host.

### 19. Backup my apps (later, not in v1)
- **No sign-in in v1.** Everything works without an account. Users are counted with download numbers and the
  off-by-default anonymous check-in (Settings → Privacy).
- Moving apps to a new computer in v1: **Export app list** to a file, then import it on the other machine.
- Version 2, only if users ask for it: an optional **"Back up my apps (GitHub)"** in Settings, not an account icon.
  - GitHub device flow ("enter this code at github.com/login/device"), which needs only a public client ID: no server
    and no secret.
  - The app list and settings are encrypted on the computer and saved to a **secret Gist in the user's own GitHub
    account**. We store nothing and run nothing.
  - Same model as Atuin's sync: optional, works fully offline without it, encrypted before upload.

### 20. Quality
- Every change is tested in CI on Ubuntu, Debian, Fedora, Tumbleweed, Arch and Silverblue containers/VMs, because
  package-manager output changes between versions (apt 3, dnf5).

## What the first draft was missing (now in scope)
1. Distro detection and native backends for dnf, zypper, pacman, and rpm-ostree.
2. AUR, COPR, and PPA awareness: these community repos need trust warnings and PKGBUILD review.
3. Immutable/atomic distro mode.
4. A Repositories view, plus removing vendor repos and signing keys after uninstalling.
5. "Who installed this?" ownership lookup and adoption.
6. Tracking in the user's own terminal (shell hook), not just the built-in one.
7. More sources: Homebrew on Linux, Nix profile, `uv tool`, pnpm/bun/yarn global, `go install`,
   Distrobox-exported apps (remove inside the container, not on the host), JetBrains Toolbox, Wine/Bottles.
8. Multiple runtime versions: npm globals live inside each nvm/fnm/volta Node version, and must be listed per version.
9. Side effects left behind by installs: autostart entries, systemd user services/timers, shell rc edits.
10. Safety: snapshot before risky actions when Timeshift/Snapper is available (openSUSE has Snapper by default).
11. Robustness: package-manager lock handling (e.g. unattended-upgrades or PackageKit holding the lock), offline
    mode, and streaming live output for every action.
12. Export/migrate between distros.

## Architecture

How Housekeeper is built (crates, traits, the `Host`, and every design decision) is in
[architecture](../architecture/README.md). That document is the only source for it; this section only records the
product reasons behind two of its choices.

- **Why not PackageKit only?** It covers apt/dnf/zypper/pacman through one D-Bus API with polkit. But it hides
  "manually installed vs dependency", has no AUR, its pacman backend is limited, and it can't follow Arch's
  rules. So each distribution family gets its own native source; PackageKit may later serve distributions that have
  none (not decided).
- **Why no Flatpak of Housekeeper itself?** A package manager inside the Flatpak sandbox needs `flatpak-spawn --host`
  for nearly everything ([D16](../architecture/README.md#decisions)).

## Feasibility and hard parts
- Listing and removal for every source above: straightforward.
- AppImage integration: solved problem (FUSE2 availability varies by distro).
- Arbitrary-script tracking: best-effort (snapshot diff). Precise tracking needs root fanotify or an overlay.
- Data cleanup: heuristic. Always show what was found and ask first.
- Cross-distro testing: needs a CI matrix of containers/VMs (Ubuntu, Debian, Fedora, Tumbleweed, Arch, Silverblue).

## Related tools (Housekeeper calls them; it doesn't replace them)
- GNOME Software / KDE Discover: native + Flatpak + Snap via PackageKit. No AppImage, npm, or manual installs.
- bauh: closest match (AppImage, Flatpak, Snap, deb, Arch/AUR). No dnf/zypper, no npm/pipx/cargo, no tracking.
- Gear Lever / AppImageLauncher: AppImage only. Warehouse: Flatpak only.

## Roadmap
1. Read-only Library: distro detection + all sources.
2. Removal (polkit) using the uninstall rules above + recipe repo (top ~50 script-installed tools);
   drag-and-drop install for native formats + AppImage.
3. Tracked terminal + shell hook + manifests.
4. Updates + Cleanup + Repositories (with each distro's rules).
5. Unified search, Who-owns, Export/migrate, Environment (variables, PATH, aliases).
6. Storage plugins, Defaults & startup, CLI.
7. Image-based mode, Nix, PackageKit fallback for other distros.
