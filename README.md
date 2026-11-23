# Housekeeper

*Every app, however it got here.*

Housekeeper shows every app installed on a Linux machine, whatever installed it (apt/dnf/zypper/pacman, Flatpak, Snap,
AppImage, npm, pipx, uv, cargo, or a script), and removes each one the way its own installer intended.

It is a front door to the native tools, not a replacement for them.

> **Status:** early development. See the [build plan](docs/PLAN.md) for what works today.

## Try it
```sh
cargo run -p housekeeper-cli -- sources          # which sources exist on this machine
cargo run -p housekeeper-cli -- list             # apps installed on purpose
cargo run -p housekeeper-cli -- list --all       # plus dependencies, libraries, runtimes
cargo run -p housekeeper-cli -- list --source flatpak --search godot
cargo run -p housekeeper-cli -- list --json      # machine-readable scan (schema 1)
```
Nothing is changed on the system: this phase only reads.

## Documents
- [Product spec](docs/SPEC.md): what Housekeeper does and why.
- [Architecture](docs/ARCHITECTURE.md): how the code is organised and the design decisions.
- [Build plan](docs/PLAN.md): phases and chunks.
- [Interactive mockup](design/mockup/index.html): open in a browser; keep `store-snapshot.js` next to it.

## Licence
MIT
