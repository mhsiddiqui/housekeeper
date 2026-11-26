# 0045: Add the root helper, which removes checked packages

- **Date:** 2026-11-26
- **Branch / PR:** `feat/root-helper`, #35
- **Plan chunks:** 4.1, done
- **Commits:** 3a5d2ee, a402d45, 704825f, d88b134

## What changed
- **New crate `crates/helper`, binary `housekeeper-helper`** (decisions D4, D25, D40).
  - It reads one JSON request on standard input (at most 64 KiB), checks it, carries it out and prints one JSON reply.
  - Exit code 0 means done, 1 means the package tool failed, 2 means refused.
  - It runs only when its effective user is root, read from `/proc/self/status`.
  - It runs programs with a fixed `PATH` (`/usr/sbin:/usr/bin:/sbin:/bin`) and never scans.
- **New core module `helper.rs`.**
  - Types: `HelperRequest` (`protocol: 1`, `operation`), `HelperOperation::RemovePackages { tool, packages }` and
    `HelperReply` (`done`, `failed`, `refused`). Unknown fields and operation kinds are rejected.
  - `PackageRemoval` trait: the tool's name, `is_installed` and `command`.
  - `carry_out(platform, host, request)`. It refuses when:
    - the protocol version differs;
    - the tool is not the system's;
    - there are no packages, or more than 100;
    - a name is not a plain package name (letters, digits and `+-._:~^@`, starting with a letter or digit);
    - the database does not list a package as installed.
  - Otherwise it runs the tool's command with a one-hour limit.
  - `Platform::package_removal` returns the system's tool; `is_package_name` is public.
- **Linux package tools implement `PackageRemoval`; `LinuxPlatform` keeps its tool.**

  | Tool | Installed check |
  |---|---|
  | apt | `dpkg-query --show` with the exact id and status `installed` |
  | dnf, zypper, rpm-ostree | `rpm -q --quiet` |
  | pacman | `pacman --query` |

  - Plans now build their argv from the same `command`, so the helper runs exactly the command a plan shows.
- **dnf and zypper remove without repositories:** `dnf remove --assumeyes --disablerepo=*` and
  `zypper --non-interactive --no-remote remove`. Removing needs only what is installed, and a mirror that is down could
  otherwise stop it. These are the flags `scripts/test-package.sh` already used for its own removal.
- **Polkit action** `packaging/io.github.mhsiddiqui.housekeeper.policy`.
  - Action `io.github.mhsiddiqui.housekeeper.helper`, `auth_admin` in every session kind, with `exec.path` set to the
    helper.
  - Installed to `/usr/share/polkit-1/actions/`.
- **Packaging.** `nfpm.yaml` installs the helper at `/usr/lib/housekeeper/housekeeper-helper` and the policy.
  `scripts/package.sh` builds both binaries and puts the helper and the policy in the tarball.
- **`scripts/test-package.sh`.**
  - On every distribution, the helper refuses a package that is not installed and an option passed as a name.
  - Where the package is installed, it checks the policy file and removes Housekeeper's own package through the
    helper, using the id from `housekeeper show --json`, instead of calling the package manager directly. It then
    checks that the CLI and the helper are gone.
- **Docs:**
  - architecture: the layer diagram and crate list, the `PackageRemoval` row, new decision D40;
  - AGENTS.md's crate list and D1–D40 (with docs/README.md and the guide, which lists the helper's files);
  - README (what the packages install, the roadmap), CHANGELOG (Added, Changed), and the plan (4.1 ✅, Phase 4 🚧,
    6.4 reworded).

## Why
Plan chunk 4.1: nothing can change the system until a root helper exists (D4). The helper takes the `tool` and
`packages` of a plan's `RemovePackages` step and nothing else (D31). It checks the names itself and builds the command
itself (D25), so a compromised or buggy front end cannot turn it into a root shell. Trashing system paths (AppImages and
recipe folders outside the home folder) joins the protocol with the Trash work in 4.4.

## How it was verified
- **New tests:**
  - core `helper.rs`, with a stub platform and tool:
    - an installed package is removed with the tool's command;
    - a failing tool is reported with its exit code and output;
    - names that could be options, patterns or paths (`-y`, `--purge`, `vim*`, `../etc`, `/usr/bin/vim`, spaces,
      empty, newline) are refused before anything runs (the fake runner panics on any unexpected command), while
      `libc6:i386`, NEVRAs, `gtk+3`, `kernel-1:6.1~rc1^git` and `python@3` pass;
    - a package the database does not list is refused, and so are another tool, no tool, an empty or 101-package
      request, and another protocol version.
  - helper binary:
    - a valid request parses;
    - an extra `argv` field, an unknown `run` operation, an extra top-level field, empty input and an oversized
      request are refused;
    - only an effective uid of 0 counts as root.
  - Linux:
    - for every distribution, the helper's `tool` and `command` equal the plan's `tool` and `argv`;
    - expectations updated for the dnf and zypper flags.
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (8 + 41 + 2 + 148)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - the Windows check of core and platform
  - `cargo deny` was not run (not installed); no new dependency (the helper uses `serde_json`, already in the
    workspace).
- **The static helper as root in containers on this machine:**

  | Container | Refused (not installed, `-y`, wrong tool) | Removed | Asked again |
  |---|---|---|---|
  | Debian stable | yes | `hello` with `apt-get remove --yes hello` | refused: not installed (`config-files` state) |
  | Fedora 45 | yes | `hello-2.12.3-2.fc45.x86_64` with `dnf remove --assumeyes --disablerepo=* …` (dnf5) | refused |
  | openSUSE Leap 16.1 | yes | `hello-2.12.1-bp161.1.3.x86_64` with `zypper --non-interactive --no-remote remove …` | refused |
  | Arch | yes | `figlet` with `pacman --remove --noconfirm figlet` | refused |

  - As my user (not root), the helper refused with exit 2.
- **Packaging:** `scripts/package.sh`, with nfpm run from the `goreleaser/nfpm` image, built the `.deb`, `.rpm`, Arch
  package and tarball (which lists `housekeeper-helper` and the policy). The updated `scripts/test-package.sh dist`
  passed in Debian stable, Fedora latest, openSUSE Leap latest and Arch Linux (x86_64), each ending with the helper
  removing Housekeeper's package.
- **Not tried here:** `pkexec` itself with the policy installed on a desktop. The containers have no polkit daemon, and
  installing the policy on this machine needs root.

## Known limits and follow-ups
- **Nothing calls the helper yet.** `housekeeper remove` will in 4.5, through `pkexec`.
- **Trashing system paths** (AppImages and recipe folders outside the home folder) is not in the protocol yet; 4.4.
- **No lock handling.** The helper reports a held package-manager lock as the tool's failure; waiting for it is 4.2.
- **No `Recommends:` on polkit.** The packages do not depend on polkit, whose package name differs per distribution
  (`pkexec`, `polkit`). The CLI will say when `pkexec` is missing (4.5).
- **Protected packages are not re-checked by the helper.** The front end blocks protected packages (D32) and re-checks
  them before acting (D37, 4.5); the helper checks only what D25 gives it: the tool, the names, and that each package
  is installed.
