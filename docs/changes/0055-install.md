# 0055: Install a downloaded file the way its plan says

- **Date:** 2026-11-28
- **Branch / PR:** `feat/install`, #45
- **Plan chunks:** 5.5 (see [plan](../planning/plan.md))
- **Commits:** filled in below before merging

## What changed
- **Decision D47** (new): how an install is carried out, and why the root helper checks a checksum rather than the
  package's name and version. D40 lists the new helper operation and D46 the step's new shape.
- **Core:**
  - `Host::sha256`, `Host::copy_new` (copies into a new file, mode 644, and returns the checksum of what it copied) and
    `Host::create_owner_dir`. The checksum comes from the new dependency `sha2` (MIT or Apache-2.0, pure Rust).
  - `Inspection::sha256`: `Housekeeper::inspect` reads the whole file once for it. `Inspection::refusal`,
    `package_name` and `display_name` say why a file is not installed and what it holds.
  - The step `Action::InstallPackageFiles { files }` is now `Action::InstallPackageFile { file }`: one file, as every
    plan had (`Step::install_package_file`).
  - Helper (`helper.rs`):
    - `HelperOperation::InstallPackageFile { tool, file, sha256 }`.
    - The new `PackageInstall` trait, a `PackageRemoval` with the formats it installs and `install_command`.
    - `Platform::package_install` and `Platform::helper_staging`.
    - The operation refuses another system's tool, a malformed checksum and a path that is not plain. Otherwise it
      copies the file into the staging folder: made writable by its owner only, and checked to be a real folder owned
      like its parent. It refuses unless the copy has the checksum and is a package of a format the tool installs,
      made for this system (`Platform::recognise_file` on the copy).
    - It then runs the tool's command on the copy and removes the copy afterwards.
    - `run_tool` is shared with removals.
    - `REMOVAL_TIMEOUT` is now `STEP_TIMEOUT`.
  - `carry.rs`:
    - `Housekeeper::still_as_inspected` inspects again and compares the checksum, the fit and the plan.
    - `Housekeeper::install` runs the plan through the operation queue under the tool's lock and records it.
    - Removals and installs share one job (`ChangeJob`) and one recorder (`carry`).
    - `Removal` is now `Recorded`.
  - History: `Change::Install` and an optional `file` on `HistoryEntry`.
- **Linux:**
  - apt, dnf, zypper, rpm-ostree and pacman implement `PackageInstall`, offered through `PackageTool::package_install`.
    Their install plans now come from `install_command`, so a plan shows exactly the command the helper runs.
  - zypper adds `--allow-unsigned-rpm` for an `.rpm` without signature tags (`rpm -qp` of `RSAHEADER`, `DSAHEADER`,
    `SIGPGP`, `SIGGPG`); its plan then has a caution.
  - `LinuxPlatform` stages copies in `/var/cache/housekeeper`.
- **CLI:**
  - `housekeeper install <file> [--yes] [--json]`: it shows the inspection, refuses what does not fit or is blocked,
    asks, checks again, installs, and prints how each step went.
  - Exit codes: 0 installed, 2 no such file or unknown format, 3 nothing changed, 4 a step failed.
  - `inspect` prints `SHA-256:`.
  - `history` lists installs.
  - `remove` and `install` share `confirm`, `progress` and `finish`.
- **`scripts/test-package.sh`:**
  - The helper refuses an install request whose checksum differs.
  - `inspect` prints the same checksum `sha256sum` gives.
  - `housekeeper install --yes` of the package just installed goes through the helper and is recorded on apt and
    pacman systems, and is refused as blocked on RPM systems (rpm refuses the same version).
  - No copy is left in `/var/cache/housekeeper`.
- **Docs:**
  - architecture: the `PackageInstall` and `Job` rows, the installer-files section, D40, D46 and D47;
  - the guide's `Source` sketch and file table; D1–D47 in AGENTS.md, docs/README.md and the guide;
  - plan 5.5 ✅, CHANGELOG, and the README (intro, helper, usage, "Installing a downloaded file", safety, roadmap).

## Why
Plan chunk 5.5. 0054 left installing to this chunk. Its known limits said the helper must install a copy that only
root can change, checked against what was reviewed, since the user's file can change after the review. A SHA-256
checksum taken during the inspection ties the root install to the reviewed bytes. The copy closes the window between
that check and the package tool reading the file. The name and version inside a package cannot do that, since a
different file can carry both. 0054 also left open how to show zypper's refusal of unsigned packages; the plan now says
so before asking, as `--dangerous` does for snaps.

## How it was verified
- `cargo fmt --all --check`, `rustfmt --check` on sources and tools, and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --workspace --all-features`: all pass (core 68, Linux 170, CLI 8, helper 1). New tests:
  - the helper installs from its own copy, runs the tool on the copy and removes it;
  - the helper refuses a changed file, another system's package, a malformed checksum, a relative path and another
    tool, leaving no copy;
  - an inspected file is installed with the checksum it had and recorded;
  - a file changed since it was inspected is refused, and a blocked plan asks nothing of the helper;
  - on openSUSE, an unsigned `.rpm` gets `--allow-unsigned-rpm` and a caution, and a signed one neither.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`: clean.
- `cargo deny check` (0.21.0): advisories, bans, licences and sources ok.
- Packages built with `scripts/package.sh` (nfpm run from its container image). `scripts/test-package.sh dist` passed
  in `debian:stable`, `ubuntu:24.04`, `fedora:latest`, `archlinux:latest` and `opensuse/tumbleweed` (x86_64). That
  covers the reinstall through the helper on Debian, Ubuntu and Arch, the blocked install on Fedora and Tumbleweed,
  and the helper refusing a wrong checksum everywhere.
- By hand in `debian:stable`: with only the tarball's binary and the helper copied to `/usr/lib/housekeeper`,
  `housekeeper install --yes dist/housekeeper_0.2.0-1_amd64.deb` installed the package through the helper
  (`done: … apt-get install --yes …`). `housekeeper history` listed it as `install … dpkg:housekeeper done`.

## Known limits and follow-ups
- Flatpak and snap installs run as the user and are not simulated (neither tool has a dry run). A `snap install`
  without a polkit agent (a bare terminal) is refused by snapd. That is not tried in CI, which has no snapd.
- zypper is not told to trust a signature whose key it lacks; it refuses such a package and the step fails with its
  message.
- The helper's install is tried on real systems only as a reinstall (apt, pacman) or a blocked install (RPM), and by
  hand on Debian. CI cannot install a package that is not already there without first removing Housekeeper itself.
- rpm-ostree installs were not run on a Silverblue system; CI's Silverblue job tests the tarball only.
- AppImages (5.3), archives and scripts (5.4) still have no plan.
