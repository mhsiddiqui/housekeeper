# Housekeeper

*Every app, however it got here.*

Housekeeper shows every app installed on a Linux machine, whatever installed it (apt/dnf/zypper/pacman, Flatpak, Snap,
AppImage, npm, pipx, uv, cargo, or a script), and removes each one the way its own installer intended.

It is a front door to the native tools, not a replacement for them.

> **Status:** early development. See the [build plan](docs/PLAN.md) for what works today.

## Install
Download the package for your distribution from the
[latest release](https://github.com/mhsiddiqui/housekeeper/releases/latest) and install it with your package manager:

| Distribution | Command |
|---|---|
| Debian, Ubuntu, Mint, Pop!_OS | `sudo apt install ./housekeeper_*_amd64.deb` |
| Fedora, RHEL, AlmaLinux, Rocky | `sudo dnf install ./housekeeper-*.x86_64.rpm` |
| openSUSE | `sudo zypper install --allow-unsigned-rpm ./housekeeper-*.x86_64.rpm` |
| Arch, EndeavourOS, Manjaro | `sudo pacman -U ./housekeeper-*-x86_64.pkg.tar.zst` |
| Silverblue and anything else | extract `housekeeper-*-linux.tar.gz` and put `housekeeper` on your `PATH` |

ARM64 (aarch64) packages are attached too. Check downloads against `SHA256SUMS` with `sha256sum -c SHA256SUMS
--ignore-missing`. Remove it the same way you installed it (`sudo apt remove housekeeper`, …).

## Use
```sh
housekeeper sources                       # which sources exist on this machine
housekeeper list                          # apps installed on purpose
housekeeper list --all                    # plus dependencies, libraries, runtimes
housekeeper list --source flatpak --search godot
housekeeper list --json                   # machine-readable scan (schema 1)
```
Nothing is changed on the system: this version only reads. From a source checkout, use
`cargo run -p housekeeper-cli -- <command>` instead.

## Documents
- [Product spec](docs/SPEC.md): what Housekeeper does and why.
- [Architecture](docs/ARCHITECTURE.md): how the code is organised and the design decisions.
  [Illustrated guide](docs/architecture.html): composition, and how to add a distribution or operating system.
- [Build plan](docs/PLAN.md): phases and chunks, with status.
- [Changelog](CHANGELOG.md).
- [Contributing](CONTRIBUTING.md): branch-per-task workflow and the checks CI runs.
- [Releasing](docs/RELEASING.md): how a release builds and tests the packages.
- [Interactive mockup](design/mockup/index.html): open in a browser; keep `store-snapshot.js` next to it.

## Licence
MIT
