# 0037: Block the removal of protected packages, and say why they are protected

- **Date:** 2026-11-25
- **Branch / PR:** `feat/protected-packages`, #27
- **Plan chunks:** 3.2 (protected packages), done
- **Commits:** 607dec0, d1ccf40, 4cd1aa5

## What changed
- **`Package::protected` is now the reason** (`Option<String>`) instead of a boolean: `dpkg marks it Essential`,
  `listed in dnf's protected packages (/etc/dnf/protected.d/sudo.conf)`, `the base metapackage needs it`,
  `it is the running kernel`. The scan's JSON `schema` is now 2 (`SCAN_SCHEMA`) because the field's type changed.
- **Core blocks the plan.** `remove::plan` makes the source's plan, or `Unknown` when the source has no remover. For a
  protected package it then adds a `protected: <reason>` blocker. The steps stay in the plan, so the reader still sees
  what removal would take. `Housekeeper::show` uses it, and 3.5 can use it too.
- **dpkg** also reads `${Protected}`. On Debian and Ubuntu `init`, `systemd-sysv` and `libgcc-s1` are Protected, and
  were not protected before. Order of reasons: Essential, then Protected, then Priority required. Older dpkg prints an
  empty field.
- **rpm**:
  - Reads `protected_packages` from `/etc/dnf/dnf.conf` `[main]`. `glob:` entries are skipped because they point at
    the protected.d folders, which are already read.
  - The reasons name the file that protects the package.
  - `kernel-core` and `kernel-default` left the core list.
- **pacman**:
  - `base` and everything it depends on are protected. Dependencies are matched by package name or through what a
    package provides.
  - The official kernels left the core list.
- **Snap**: snapd says "it runs every snap". The os, kernel and gadget snaps say the system runs on them.
- **Running kernel** (`crates/linux/src/kernel.rs`, shared by the three native sources):
  - The release from `/proc/sys/kernel/osrelease` gives the image: `/usr/lib/modules/<release>/vmlinuz`,
    `/lib/modules/…` or `/boot/vmlinuz-<release>`.
  - The native source names the exact package that owns it. dpkg and pacman use their owner lookup, because there the
    kernel's package name is unique. rpm uses `rpm -qf` with the NEVRA parts, because several `kernel-core` packages
    share one name.
  - Older kernels are no longer protected.
  - A failed lookup is a scan warning.
- **CLI**: `Protected:` shows the reason.
- **Docs**: README (the Protected bullet, JSON `protected` and schema 2), CHANGELOG (Added and Changed), architecture
  (the removal paragraph, D5, new D32), guide (file table, D1–D32), AGENTS.md and docs/README (D1–D32), plan 3.2 ✅.

## Why
This is plan chunk 3.2 and decision D5. A blocked plan has to say what blocks it, so the protection reason moved
into the model (D32). Turning protection into a blocker exposed a mistake from Phase 1: Fedora, openSUSE and Arch
protected every installed kernel, which would have blocked removing old kernels. The package managers themselves
allow that. dnf only protects the running kernel, so Housekeeper now does the same on every distribution.

## How it was verified
- **New or updated tests:**
  - core `show.rs`: a protected package's plan is blocked and keeps its steps, an unprotected one is not, and a
    protected package with no remover is blocked too.
  - `kernel.rs`: only the running kernel's exact id is protected, and a failed lookup becomes a warning.
  - dpkg: Essential, Protected and required reasons, and the running kernel protected while an older one is not.
  - rpm: reasons from protected.d and dnf.conf, a `.conf`-only rule, and only the running `kernel-core` of two
    protected.
  - pacman: core reasons, `base`, a dependency by name, a dependency through `%PROVIDES%`, and only the running
    `linux` protected while `linux-lts` is not.
  - snap: snapd's reason.
- **Local checks:**
  - `cargo fmt --all` and `rustfmt` on `sources/*.rs` and `tools/*.rs` were run.
  - These passed:
    - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    - `cargo test --workspace --all-features` (7 + 27 + 138)
    - `cargo doc` with `-D warnings`
    - `cargo +1.85 check --workspace --all-targets --all-features`
    - `cargo check -p housekeeper-core -p housekeeper-platform --target x86_64-pc-windows-gnu`
  - `cargo deny` was not run (it is not installed). No dependency was added.
- **This Ubuntu 26.04 machine:**
  - `show dpkg:bash` was blocked as "dpkg marks it Essential".
  - `show dpkg:init` was blocked as "dpkg marks it Protected".
  - `show dpkg:linux-image-7.0.0-42-generic` (the running kernel) was blocked. Only 1 of the 16 installed kernels is
    protected.
  - `show snap:snapd` was blocked as "it runs every snap".
  - `dpkg:sublime-merge` was not blocked.
  - In `list --all --json`, `schema` is 2. The 81 protected packages are 21 Essential, 5 Protected, 52 required,
    1 running kernel, snapd and the core snap.
- **Containers, with the static build:**
  - Fedora: `sudo` and `setup` are protected by their protected.d files, `glibc` by the core list.
  - Arch: `base` defines the system, `gzip` is needed by base, `pacman` is in the core list.
  - Tumbleweed: `zypper` and `bash` are protected.
  - Debian: `bash` is Essential, `mawk` is required, `libgcc-s1:amd64` is Protected.
- **Packaging:** `scripts/package.sh` (nfpm from its container image) and `scripts/test-package.sh dist` passed in
  Debian stable, Ubuntu 24.04, Fedora, AlmaLinux 9, openSUSE Leap, openSUSE Tumbleweed and Arch Linux (x86_64).

## Known limits and follow-ups
- The running-kernel lookup was not seen on a real Fedora or Arch install: containers have no kernel package. It is
  covered by tests with the recorded output shapes.
- dnf5's drop-in configuration (`/etc/dnf/libdnf5.conf.d`) is not read for `protected_packages`. Its default list is
  the protected.d folder, which is read.
- openSUSE has no protected-package configuration of its own. The core list and the running kernel are all it gets.
- Packages that become protected only because a protected package depends on them (a removal pulling in `libc6`) are
  found by the package manager's simulation, which is chunk 3.3.
