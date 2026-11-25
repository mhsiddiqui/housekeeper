# 0036: Plan how every installed item is removed

- **Date:** 2026-11-25
- **Branch / PR:** `feat/removal-planning`, #26
- **Plan chunks:** 3.1 (package tools, `Remover`, tiers 1–6, who runs each step), done; Phase 3 started
- **Commits:** 7869817, a7334bb, 0119707

## What changed
- **Core `remove.rs`**: the optional ability `Remover` (`Source::remover()`), which makes a `RemovalPlan`. A plan has
  a `Tier` (own uninstaller, package manager, format convention, recipe, manifest, unknown; D8), ordered `Step`s, each
  with an `Actor` (`User` or `Helper`, D25) and an `Action` (`Run { argv }`, `Trash { path }`,
  `RemovePackages { tool, packages, argv }`), plus `blockers` and `restart`. Nothing is carried out; Phase 4 does that.
- **`Housekeeper::show`** adds `Details::removal`: the source's plan, `Unknown` when the source has no remover, and
  `None` plus a warning when the remover failed.
- **Package tools (D24)**: `crates/linux/src/tools/` with `apt.rs`, `dnf.rs`, `zypper.rs`, `rpm_ostree.rs` and
  `pacman.rs`. Each implements `PackageTool` and `LinuxTool` and registers with `register_package_tool!(<Type>)`
  (`register_tool` is the name of a built-in attribute). `build.rs` now generates both `PROVIDERS` and `TOOLS`.
  `LinuxPlatform` picks the first tool whose `Serves::families` contains the native source's family and whose
  `image_based` matches the system. It hands that tool to the native source through `Context::tool`. `DpkgSource`,
  `RpmSource` and `PacmanSource` keep the tool and return its remover; tests build them with `default()`.
- **Native plans**: these are the `RemovePackages` steps the root helper will run:
  - `apt-get remove --yes <dpkg id>`
  - `dnf remove --assumeyes <NEVRA>`
  - `zypper --non-interactive remove <NEVRA>`
  - `pacman --remove --noconfirm <name>`
  - rpm-ostree: `rpm-ostree uninstall <name>` for packages the user layered, with a restart. The image's own packages,
    and packages whose layering rpm-ostree did not report, are blocked.
- **Other sources** (written by three subagents in separate worktrees against the core contract, then reviewed and
  merged here):
  - Flatpak: `flatpak uninstall --noninteractive --user|--system|--installation=NAME <ref>`.
  - Snap: `snap remove`.
  - Homebrew: `<prefix>/bin/brew uninstall [--cask]`, with the prefix taken from the keg or caskroom location.
  - npm: `<prefix>/bin/npm uninstall --global --prefix <prefix>`, using the npm of the package's own Node. Falls back to
    `npm` when that prefix has none. Blocked outside the home folder.
  - pipx: `pipx uninstall <venv>`.
  - uv: `uv tool uninstall <dir name>`.
  - pip: `<python> -m pip uninstall --yes`. Adds `--break-system-packages` only when that interpreter's standard library
    has `EXTERNALLY-MANAGED`.
  - cargo: `cargo uninstall --root <cargo home>`. Blocked outside the home folder.
  - AppImage: `FormatConvention`. Trash the file and the launchers that start it, directly or through a symlink. Steps
    outside the home folder go to the helper. Launcher files those launchers hide are left alone; I removed this part
    from the subagent's version, because a hidden system launcher can belong to a package.
  - Manual: rustup gets its own uninstaller (`<CARGO_HOME>/bin/rustup self uninstall -y`). Everything else is `Unknown`.
  - Every non-native step runs as the user.
- **CLI**: `housekeeper show` adds a `Removal:` block. It shows the tier, one line per step ("as you" or "as root, by
  Housekeeper's helper", arguments with spaces quoted), "takes effect after a restart" and `blocked: …` lines. `--json`
  includes the plan as `removal`.
- **`scripts/test-package.sh`**: on every distribution, `show housekeeper` must plan the removal with that
  distribution's tool (apt-get, dnf, zypper or pacman) through the helper.
- **Docs**:
  - Architecture: the `Remover` and `PackageTool` rows, the `Source` sketch, the removal paragraph, a "Package tools"
    section and D31.
  - Guide: the tools box is no longer "future"; the `Source` sketches, abilities section and file table are updated.
  - Also updated: README, CHANGELOG, plan 3.1 ✅ / Phase 3 🚧, AGENTS.md (tools rule, D1–D31) and docs/README.

## Why
This is plan chunk 3.1 and decisions D8, D24 and D25. Decision D31 records the plan's shape. The root helper must be
given only a tool and package names, never a command line, so the package step is typed and its `argv` is there for
review only. Language package managers installed outside the home folder are blocked rather than run as root. The
helper only runs the system's package tool (D25), and npm runs package scripts.

## How it was verified
- New tests:
  - core `show.rs`: the plan comes from the source, is `Unknown` without a remover, and a failing remover gives a
    warning.
  - linux `lib.rs`: the native source plans with apt, dnf (Fedora, and AlmaLinux through `ID_LIKE`), rpm-ostree
    (Silverblue), zypper and pacman, and with no tool on an unclaimed distribution.
  - `rpm_ostree.rs`: layered packages vs. image packages.
  - Per-source tests for Flatpak (user, system, named installation), Snap, Homebrew (formula and cask from a second
    prefix), npm (nvm prefix's npm, fallback, system blocked), pipx, uv, pip (`--break-system-packages` only with the
    marker), cargo (user, system blocked), AppImage (home vs. `/opt`, symlinked program, unrelated launcher left) and
    manual (rustup with `CARGO_HOME`, hand install unknown).
  - CLI render test with a helper step.
- `cargo fmt --all` plus `rustfmt` on `sources/*.rs` and `tools/*.rs` made no further changes. Also run and passed:
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test --workspace --all-features` (7 + 26 + 134 tests)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check --workspace --all-targets --all-features`
  - `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu`
- `cargo deny` is not installed here and was not run. No dependency was added.
- On this Ubuntu 26.04 machine, `show` printed the expected plans:
  - `dpkg:sublime-merge`: apt-get through the helper.
  - `snap:sublime-merge`: `snap remove`.
  - "Godot Engine": `flatpak uninstall --noninteractive --system app/org.godotengine.Godot/x86_64/stable`.
  - `cargo:rmux`: `~/.cargo/bin/cargo uninstall --root ~/.cargo rmux`.
  - `installer/rustup`: `rustup self uninstall -y`.
  - An npm package under nvm v22: that version's npm, with `--prefix`.
  - pipx, uv and pip (`python3.8`, no marker): their uninstall commands.
  - A hand install: "not known".
  - `show --json` prints the typed `remove-packages` step.
- In containers:
  - Tumbleweed: `zypper --non-interactive remove --dry-run` accepted `bash-5.3.20-11.1.x86_64` (name-version-release.arch).
  - Fedora: `dnf remove --assumeno gmp-1:6.3.0-6.fc45.x86_64` resolved the package; the transaction then stopped
    because protected packages need it.
- Packaging: `scripts/package.sh` (nfpm from the `goreleaser/nfpm` image), then `scripts/test-package.sh dist` with the
  new removal check. It passed in Debian stable, Ubuntu 24.04, Fedora (latest), AlmaLinux 9, openSUSE Leap, openSUSE
  Tumbleweed and Arch Linux containers (x86_64).

## Known limits and follow-ups
- Nothing checks whether a plan would work. The package manager's simulation of everything that would go is chunk 3.3,
  and protected packages blocking a plan are chunk 3.2. Phase 4 carries plans out.
- transactional-update (openSUSE Aeon, MicroOS) has no tool file yet. Those systems are not detected as image-based, so
  they get the zypper plan, which would fail on their read-only root. This must be fixed before Phase 4: detect them
  and add `tools/transactional_update.rs`.
- zypper with an epoch in the NEVRA was not tried: the Tumbleweed image had no package with an epoch.
- The pip plan only finds interpreters in `/usr/local/bin` and `/usr/bin`. Others (pyenv) get the bare `pythonX.Y`
  name. It does not check that pip is installed for that interpreter.
- Script installers other than rustup and hand installs stay `Unknown` until recipes (3.4).
- An AppImage plan does not include icons or other files an integration tool wrote. Leftovers are chunk 3.5.
