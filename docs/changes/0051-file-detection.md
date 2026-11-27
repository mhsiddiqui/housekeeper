# 0051: Tell what a downloaded installer file is

- **Date:** 2026-11-27
- **Branch / PR:** `feat/file-detection`, #41
- **Plan chunks:** 5.1 (see [plan](../planning/plan.md)), which starts Phase 5
- **Commits:** ef88569, 63c6361, 4e4c4a8, 678cf34, a61a161

## What changed
- **Core** (`crates/core/src/inspect.rs`, new):
  - `Housekeeper::inspect(path)` returns an `Inspection`: the format (`FormatInfo`: id and title), how it was told
    (`evidence`), the source that would list it once installed, what is inside (`FilePackage`: name, version, title,
    processor, origin), whether it fits (`Fit`) and warnings.
  - The fit is decided in core, in this order: another system's own package format (`OtherSystem` with the families
    it is for), the source not available here (`SourceMissing` with the probe's reason), built for another processor
    (`OtherArch`), otherwise `Fits`.
  - `InspectError`: no such file, not a file, or the file could not be read.
  - `Platform::recognise_file` (default `Ok(None)`) gives core a `Recognised`.
- **Linux platform:**
  - `provider.rs`: `FileFormat` (id, title, name endings, a first-bytes check, evidence, a `ReadFile` function) and
    `LinuxSource::FILES` (default empty); `Provider` carries them.
  - `formats.rs` (new): `recognise` reads the first 1024 bytes and keeps the formats whose name ending and bytes both
    match. When several match, the longest ending wins. Also shared helpers: processor names (`machine`, `elf_machine`),
    compressed and tar magic, `key: value`/`key = value` fields, a tool's `name<TAB>version<TAB>processor` line.
  - `LinuxPlatform` keeps every provider's formats, including those of native sources it did not pick, as
    `Owner::Foreign(families)`.
  - Formats, each declared in its source's file:

    | Source | Format | Told by | Inside, from |
    |---|---|---|---|
    | dpkg | `.deb` | `!<arch>\ndebian-binary` | `dpkg-deb --show` |
    | rpm | `.rpm` (binary only) | lead magic, type 0 | `rpm -qp --queryformat`, else the lead's `name-version-release` |
    | pacman | `.pkg.tar.{zst,xz,gz}`, `.pkg.tar` | compressed or tar | `bsdtar -xOqf … .PKGINFO` |
    | flatpak | `.flatpakref` | `[Flatpak Ref]` in the first 256 bytes | the file (`Name`, `Title`, `Url`) |
    | flatpak | `.flatpak` bundle | `flatpak\0\x01\0\x89\xe5` (or `xdg-app…`) | nothing |
    | snap | `.snap` | `hsqs` | `unsquashfs -cat … meta/snap.yaml` |
    | appimage | any name | ELF with `AI\x01`/`AI\x02` | the file name (as the source names kept AppImages), ELF processor |
    | manual | `.tar.*`, `.tgz`… `.zip` | compressed, tar or zip magic | nothing |
    | manual | any name | `#!` | nothing |
    | manual | `.run`, `.bin` | ELF, not an AppImage | ELF processor |
  - `files::is_appimage_head` split out of `is_appimage` so the AppImage format and the scan share it.
- **CLI:** `housekeeper inspect <file> [--json]` (`crates/cli/src/inspect.rs`): format, how it was told, package,
  title, processor, origin, source, fit. Exit codes 0 fits, 1 does not, 2 no such file or unknown format.
- **`scripts/test-package.sh`:** on every distribution where the package is installed, `housekeeper inspect` on that
  package must exit 0 and name `housekeeper <version>`.
- **Docs:** decision D45; architecture section "Installer files"; guide (Alpine example, file table); D1–D45 in
  AGENTS.md, docs/README.md and the guide; README usage line, "What is this file?" section and roadmap; CHANGELOG.

## Why
Plan chunk 5.1. Installing a downloaded file (5.2) needs to know what it is first: the name alone can lie, and a
`.deb` on Fedora must be named as Debian's rather than "unknown", although no dpkg source exists there. Declaring each
format next to its source keeps D20 (one file per source) and makes a new distribution's file format part of that
same file (D45).

No dependency was added. Reading a `.deb`'s or an Arch package's metadata without the distribution's tool would need
decompressors (xz, zstd, gzip). Instead the reader uses the format's own tool where it is installed, and the
evidence the file gives plainly everywhere else.

## How it was verified
- `cargo fmt --all --check`, `rustfmt --check` on the sources and tools, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo test --workspace --all-features` and `RUSTDOCFLAGS="-D warnings" cargo doc
  --workspace --all-features --no-deps`: all passed. `cargo deny` is not installed here (no dependency changed).
- New tests: another distribution's `.deb` does not fit and names `debian`; first bytes decide, and the longest name
  ending breaks ties (`.pkg.tar.zst` vs `.tar.zst`, a script named `.deb`, a renamed AppImage, a fake `.AppImage`); an
  AppImage for another processor does not fit; `.deb` read with `dpkg-deb` (`all` fits, `s390x` does not); `.rpm`
  from the lead without `rpm`, and from `rpm -qp` on Fedora; a source RPM is not an installer; `.flatpakref` with and
  without Flatpak; `.snap` from `snap.yaml` (one, several and `all` architectures); an Arch package from `.PKGINFO`.
- On this Ubuntu 26.04 machine (x86_64), `housekeeper inspect` on:
  - a real downloaded `.deb`: `borg-collective 0.0.20`, processor x86_64, fits;
  - `hello-2.12.2-2.fc43.x86_64.rpm` from Fedora's mirror: `hello 2.12.2-2.fc43` from the lead, "a package for fedora
    and suse systems", exit 1;
  - Flathub's `org.gnome.Calculator.flatpakref`: name, title and `https://dl.flathub.org/repo/`, fits;
  - `hello-world_29.snap` (`snap download`): `hello-world 6.4`, fits;
  - an Arch `which` package: Arch package, not for this system;
  - an ELF header marked as an aarch64 AppImage: "built for aarch64; this computer is x86_64", exit 1;
  - a zip, a shell script and an ELF `.run`: archive, script and installer program; `Cargo.toml`: unknown, exit 2;
    a missing path and a folder: exit 2.
- Packages built locally with `scripts/package.sh` (nfpm through its Docker image), then `scripts/test-package.sh` in
  `debian:oldstable`, `ubuntu:24.04`, `almalinux:9`, `fedora:latest`, `opensuse/tumbleweed:latest` and
  `archlinux:latest` (x86_64): all ended with `OK`, including the new inspect check.
- The tarball's binary in `fedora:latest` and `archlinux:latest`, on the three built packages: Fedora read the `.rpm`
  with `rpm` (`housekeeper 0.2.0-1`, x86_64, fits) and refused the `.deb` and the Arch package; Arch read its package
  with `bsdtar` (`housekeeper 0.2.0-1`, x86_64, fits) and refused the `.rpm` (named from its lead) and the `.deb`.

## Known limits and follow-ups
- A Flatpak bundle's app is not read: its metadata is a GVariant after the header, and `flatpak` has no command that
  shows a bundle without installing it.
- Without the format's tool, a `.deb`, an Arch package or a `.snap` shows only its format (a `.deb` on Fedora, a
  `.snap` where squashfs-tools is missing).
- A `.snap` listing several architectures, or none, is taken to fit any processor; snapd checks it when installing.
- A shell script without a `#!` line is not recognised, whatever its name.
- `PKGBUILD` files (in the product spec's table) are not recognised yet; 5.2 decides whether they belong with the Arch
  package plans.
- Install plans per format are 5.2; AppImage integration 5.3; archives and scripts 5.4.
