# Housekeeper

*Every app, however it got here.*

Housekeeper shows every app installed on a Linux machine, whatever installed it (apt/dnf/zypper/pacman, Flatpak, Snap,
AppImage, npm, pipx, uv, cargo, or a script), and removes each one the way its own installer intended.

It is a front door to the native tools, not a replacement for them.

> **Status:** early development. See the [build plan](docs/planning/plan.md) for what works today.

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

## Documentation
All documentation lives in [docs/](docs/README.md), grouped by section:

- **Product**: [spec](docs/product/spec.md), [interactive mockup](design/mockup/index.html) (open in a browser).
- **Architecture**: [overview and decisions](docs/architecture/README.md),
  [illustrated guide](docs/architecture/guide.html) (how to add a distribution or operating system).
- **Planning**: [build plan](docs/planning/plan.md), phases and chunks with status.
- **Development**: [contributing](CONTRIBUTING.md), [releasing](docs/development/releasing.md).
- **History**: [change records](docs/changes/README.md) (one per change), [changelog](CHANGELOG.md) (per release).

Working on the code with an AI agent? It follows [AGENTS.md](AGENTS.md).

## Licence
MIT
