# 0038: Simulate removals without root, and block what would break

- **Date:** 2026-11-25
- **Branch / PR:** `feat/removal-simulation`, #28
- **Plan chunks:** 3.3 (simulation through the package manager), done
- **Commits:** ebc1b60, 8a160d2, ad4d147

## What changed
- **Core**:
  - `Remover::simulator()` offers an optional `Simulator`. `simulate(host, package)` returns a `Simulation`:
    - `by`: who answered.
    - `removes`: the source's ids that would go, the item first.
    - `refused`: the package manager's own words when it would not go ahead.
  - `RemovalPlan::simulation` holds the result, and `--json` prints it as `removal.simulation`.
  - `remove::plan` simulates only packages that are not protected. It adds a blocker for each refusal and for each
    protected package in the result ("it would also remove passwd, which is protected: …"). A failed simulation
    becomes a warning.
- **apt**:
  - Runs `apt-get --simulate remove <id>`, which needs no root.
  - Its `Remv` lines are mapped to dpkg ids: apt drops the native architecture, which dpkg's id keeps for multi-arch
    packages. `dpkg-query --show` and `dpkg --print-architecture` resolve those.
  - Exit 100 with `E:` messages is a refusal. Each message keeps its indented explanation, so apt 3's solver says
    which package forced it.
- **pacman**: runs `pacman --remove --print --print-format %n` without root. pacman's `:: removing X breaks
  dependency …` lines are the refusal.
- **dnf and zypper**: the simulation comes from the RPM database, through the new `crates/linux/src/rpmdb.rs`. It
  takes the package plus every installed package that requires one of its capabilities or programs, directly or
  through others, two rpm calls per level. dnf4 and zypper need root to simulate, and dnf5's root-free `--assumeno`
  prints only a table, so neither tool's own simulation is used (D33).
- **rpm source's "Needed by"** uses the same `rpmdb::direct_dependants`. It now also counts packages that require one
  of the package's programs (`/usr/bin/…`, `/usr/sbin/…`). The running-kernel query uses the shared `ID_FORMAT`.
- **CLI**: the `Removal:` block says "removes nothing else, says …" or "also removes N packages: …, says …", listing
  at most 10 names. A refusal shows only as a blocker.
- **`scripts/test-package.sh`**: on every distribution, `show housekeeper` must report that removing it takes nothing
  else.
- **Docs**:
  - Architecture: the `Simulator` row, the removal paragraph, D5, and new decision D33.
  - Also updated: README, CHANGELOG (Added and Changed), plan 3.3 ✅, and D1–D33 in AGENTS.md, docs/README and the
    guide.

## Why
This is plan chunk 3.3 and decision D5: show everything a removal would take before anything happens. The plan text
named each manager's own simulation, but trying them unprivileged showed:

| Tool | Release tried | Simulates without root |
|---|---|---|
| `apt-get --simulate` | Debian | yes |
| `pacman --remove --print` | Arch | yes |
| `dnf --assumeno` | Fedora 45 (dnf5) | yes, but prints only a table |
| `dnf --assumeno` | AlmaLinux 9 (dnf4) | no: "This command has to be run with superuser privileges" |
| `zypper --dry-run` | Tumbleweed | no: "Root privileges are required" |

Running these as root would contradict D4. D33 records the choice: use the RPM database for both rpm tools.

## How it was verified
- **New tests**:
  - core `show.rs`:
    - the simulation is attached;
    - a protected package in the result blocks the plan;
    - a refusal blocks it;
    - a protected package is not simulated;
    - a failing simulation is a warning.
  - `rpmdb.rs`: the closure follows dependants of dependants once each, including through a program.
  - `apt.rs`:
    - `Remv` lines are mapped to ids (native multi-arch, foreign, Arch:all);
    - an apt 3 refusal keeps its explanation.
  - `pacman.rs`: a leaf goes alone; a needed package is refused, with who needs it.
  - The CLI render test covers the "also removes" line and its cap at 10.
  - The rpm "Needed by" test was updated for program requirements.
- **Local checks**:
  - Run: `cargo fmt --all`, and `rustfmt` on `sources/*.rs` and `tools/*.rs`.
  - Passed:
    - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    - `cargo test --workspace --all-features` (7 + 28 + 142 tests)
    - `cargo doc` with `-D warnings`
    - `cargo +1.85 check`
    - `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu`
  - Not run: `cargo deny` (not installed). No dependency was added.
- **This Ubuntu 26.04 machine**:
  - `show dpkg:sublime-merge`: removes nothing else.
  - `show dpkg:libgtk-3-0t64:amd64`: also removes 85 packages, including `code` and `evolution-data-server`; about
    1.6 s in all.
  - `show dpkg:python3`: blocked, because apt refuses (shim-signed, which is Protected, depends on grub2-common,
    which needs python3).
  - `show dpkg:libsemanage2:amd64`: blocked because it is itself required.
- **Containers, static build**:
  - Fedora: `findutils` removes nothing else. `gmp` also removes 32 packages, blocked by coreutils, dnf5, rpm and
    sudo. dnf's own `--assumeno` (without protected packages or unused-dependency cleanup) also reports 33 in all.
  - Tumbleweed: `libacl1` also removes 28. `zypper --dry-run` as root reports 28 in all, so the database closure has
    one more package: `patterns-base-fips`.
  - Arch: `libnsl` is refused ("removing libnsl breaks dependency 'libnsl' required by pam"). `gzip` is protected.
  - Debian: `libsemanage2` also removes `passwd`, which blocks it as required.
- **Packaging**: `scripts/package.sh` with the new simulation check passed in Debian stable, Ubuntu 24.04, Fedora,
  AlmaLinux 9, openSUSE Leap, openSUSE Tumbleweed and Arch Linux (x86_64), and so did `scripts/test-package.sh dist`.

## Known limits and follow-ups
- **dnf's unused-dependency cleanup** (`clean_requirements_on_remove`) is not included. With it, dnf removes more
  (100 packages for `gmp` on Fedora 45). The helper (Phase 4) can run the tool's own simulation as root before
  carrying a plan out.
- **The RPM closure is not the solver.** Rich (boolean) dependencies and alternatives are not weighed, so it can list
  a package zypper would keep (`patterns-base-fips` above). Requirements on files outside program folders are not
  followed.
- **apt** removes only what needs the package. It reports packages left unneeded ("no longer required"), but they are
  not listed; those are leftovers, chunk 3.5.
- **No simulator yet** for rpm-ostree, Flatpak (runtimes in use), Snap, Homebrew or the language tools.
- **Speed**: one `show` runs one simulation, about 0.5–1.5 s for a deep chain on this machine. Simulating many items
  at once (`remove --dry-run` in 3.5) may need batching.
