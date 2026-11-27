# 0054: Plan how a downloaded file would be installed

- **Date:** 2026-11-28
- **Branch / PR:** `feat/install-plans`, #44
- **Plan chunks:** 5.2 (see [plan](../planning/plan.md)); adds chunk 5.5
- **Commits:** (filled in below)

## What changed
- **Scope, agreed with the owner:** plans only, as Phase 3 did for removal. Nothing is installed. Carrying the plans
  out (the helper's install operation and `housekeeper install`) is the new chunk 5.5.
- **Core** (`crates/core/src/install.rs`, new):
  - `FileInstaller` (`Source::file_installer`, default `None`) and `InstallSimulator`.
  - `InstallFile`: the path, the format id and what is inside.
  - `InstallPlan`: steps, blockers, cautions, `restart`, the version it `replaces` and an optional `InstallSimulation`
    (`installs`, `needs`, `removes`, `refused`).
  - `Action::InstallPackageFiles { tool, files, argv }` and `Step::install_package_files`, always for the root helper.
    Removals refuse such a step (`carry.rs`).
  - `Housekeeper::inspect` plans the install when the file fits. It scans only the file's source to find the installed
    copy (by name or AppStream ID), and notes it when the same version is installed. It simulates when the installer
    can, then blocks on a refusal or on a protected package the install would remove. A planning or simulation
    failure is a warning. `Inspection` has a new `install` field.
- **Linux:**
  - `PackageTool::file_installer`; dpkg, rpm and pacman hand theirs over like `remover`.
  - Shared in `tools/mod.rs`: `installation` (one helper step, the file's path appended) and `rpm_install_simulation`.
  - Tools:

    | Tool | Plan | Simulation (no root) |
    |---|---|---|
    | apt | `apt-get install --yes <file>` | `apt-get --simulate install`: `Inst` lines, `Remv` lines as dpkg ids, `E:` refusals |
    | dnf | `dnf install --assumeyes <file>` | `rpm -U --test --nosignature --nodigest` |
    | zypper | `zypper --non-interactive install <file>` | the same |
    | rpm-ostree | `rpm-ostree install <file>`, restart, caution to prefer a Flatpak | the same |
    | pacman | `pacman --upgrade --noconfirm <file>` | `pacman --upgrade --print --print-format '%n %v'` |

    The RPM simulation reads `X is needed by Y` lines as requirements the repositories supply. `… is needed by
    (installed) …` and every other line (conflicts, already installed) is a refusal.
  - apt's name-to-dpkg-id lookup is split out of `ids` as `resolve`, shared by removal and install simulations.
  - Flatpak: `flatpak install --user --noninteractive --from` (reference) or `--bundle`, as the user, with a caution
    naming where it downloads from or that the runtime must be available.
  - Snap: `snap ack <file>.assert` then `snap install <file>` when the `.assert` file is beside it; otherwise
    `snap install --dangerous <file>` with a caution. Both run as the user.
  - Existing tests that called `plan`/`simulate` on types that now have two such methods call them through their trait.
- **CLI:** `inspect` prints an `Install:` section with the steps, the replaced version, what the package manager said
  ("checked with …: installs …, needs from the repositories …, removes …"), restart, notes and blockers. For a format
  without a plan it says there is none yet. It now exits 1 when the plan is blocked. `show::step_text` and
  `SIMULATED_SHOWN` are shared.
- **`scripts/test-package.sh`:** the inspect step runs on the package just installed. It now accepts exit 0 or 1
  (rpm refuses the same version, so RPM systems block). It checks for `Fits: yes`, the package and version, an
  `Install:` step through the system's tool, and the note that this version is installed already.
- **Docs:** decision D46 (D45's exit code amended there); architecture traits table (`FileInstaller`,
  `InstallSimulator`), the `Source` sketch and the installer-files section; guide file table and D1–D46; plan (5.2 ✅,
  new 5.5); CHANGELOG; README "What is this file?" and roadmap.

## Why
Plan chunk 5.2. Each family's own dry run needs root somewhere: dnf4 and zypper. `rpm -U --test` reads the same
database without root on every RPM system, as D33 chose for removals. apt and pacman simulate without root
themselves. Planning before installing keeps installs reviewable the way removals are (D31, D37).

## How it was verified
- `cargo fmt --all --check`, `rustfmt --check` on sources and tools, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo test --workspace --all-features` (169 Linux tests) and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`: all passed.
- New tests:
  - apt plans `apt-get install`, lists `Inst` packages, and blocks a `.deb` whose install would remove the protected
    `init`;
  - on Fedora, dnf plans the file, the RPM database's requirements are listed, and the installed version is what it
    replaces;
  - an upgrade that breaks an installed package (`(installed)`) is blocked;
  - pacman lists what it would install, or why it refuses;
  - a snap with an `.assert` file is acknowledged and one without is `--dangerous` with a caution;
  - a `.flatpakref` is installed with `--user --from` and names the installed copy it replaces.
- Before writing the code, each simulation was run as an unprivileged user (uid 65534) in containers:
  `apt-get --simulate install` (Ubuntu 24.04), `rpm -U --test` and `dnf install --assumeno` (Fedora 45),
  `pacman --upgrade --print` (Arch).
- The new static binary, run as uid 65534 in containers:
  - Ubuntu 24.04: Housekeeper's `.deb` installs 1 package; a `cloud-init-base` `.deb` would install 177.
  - Fedora 45: `cowsay` needs 8 Perl requirements from the repositories, `hello` installs alone. With
    `housekeeper-0.2.0` installed, its own `.rpm` is blocked ("already installed") and noted as the same version.
  - openSUSE Tumbleweed: `cowsay` is planned with `zypper` and the same 8 requirements.
  - Arch: `cowsay` installs `db5.3`, `perl` and `cowsay`; Housekeeper's package installs alone.
- On this Ubuntu 26.04 machine:
  - a newer `cloud-init-base` `.deb` replaces the installed version and brings `cloud-init`;
  - a `.deb` whose version is installed is noted as changing nothing;
  - `snap download hello-world` plans `snap ack` and `snap install`, and a copy without its `.assert` plans
    `--dangerous` with the caution;
  - Flathub's Calculator `.flatpakref` plans `flatpak install --user --from` and names Flathub.
- Packages built from this branch, then `scripts/test-package.sh` in `debian:oldstable`, `ubuntu:24.04`,
  `almalinux:9`, `fedora:latest`, `opensuse/tumbleweed:latest` and `archlinux:latest`: all `OK`.

## Known limits and follow-ups
- Nothing is installed yet; that is 5.5. The helper will need to copy the file somewhere only root can write and
  check the copy against the reviewed package, since the user can change the file after review.
- Flatpak references and bundles, and snaps, are not simulated: neither tool has a dry run.
- On RPM systems the simulation names requirements (`perl(Cwd)`), not the packages dnf or zypper would pick for them.
- An unsigned `.rpm` is planned the same as a signed one. Whether zypper accepts it is up to zypper (it refuses
  unsigned local packages without `--allow-unsigned-rpm`); 5.5 decides how to show that before installing.
- `snap ack`/`snap install` as the user rely on snapd's polkit prompt; that is checked when 5.5 runs them.
- rpm-ostree plans were not run on a Silverblue system; CI's Silverblue job tests the tarball only.
- AppImages (5.3), archives and scripts (5.4) have no install plan.
