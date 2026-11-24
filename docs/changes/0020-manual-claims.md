# 0020: Manual source leaves out what other sources claim

- **Date:** 2026-11-23
- **Branch / PR:** `fix/manual-claims`, #13
- **Plan chunks:** 1.6 (manual source), 2.3 (owner lookup)
- **Commits:** eefb002, 08962c4

## What changed
- **Core:** `Source::inventory(&self, host, scan: &ScanContext)`. `ScanContext` lists every source of the platform and
  collects warnings; `SourceStatus::Ok` gains `warnings` (left out of the JSON when empty, so scan schema 1 still
  reads the same). New `Source::install_roots(host)`, default empty: directories a source's programs live in.
- **Sources:** pipx (venvs directory), uv (tools directory), cargo (`CARGO_HOME`), npm (every global `node_modules`,
  the nvm and fnm directories), Snap (`/snap`) and Flatpak (system and user `exports`) declare their install roots.
- **Manual source:** its hard-coded `managed_roots()` list is gone; it uses the other sources' `install_roots()`
  and every `owner_lookup()` it finds in the `ScanContext`. When an owner lookup fails, the `/opt` and
  `/usr/local/bin` entries it could not check are left out and a warning is reported; the rest is still listed.
- **Linux platform:** `Context` no longer carries the native source; `LinuxPlatform::assemble` is simpler.
- **CLI:** `housekeeper list` prints a source's warnings on stderr, like failures.
- **Docs:** decision D23, the `Source` trait and assembly in the architecture README and guide, CHANGELOG.

## Why
Architecture review finding H1: the manual source hard-coded the directories of pipx, uv, cargo, nvm, fnm, Snap and
Flatpak. A new source whose programs link into `~/.local/bin` would compile, pass, and be listed twice, once as a
hand install; npm's own `/usr/local/lib/node_modules` was already missing from that list, so global npm packages
under system Node were listed twice. A failed or slow `dpkg-query --search` also failed the whole manual source.
Sources now reach each other at scan time through the `ScanContext` (D23), so this knowledge lives in each source.

## How it was verified
- New and changed tests in `crates/linux/src/sources/manual.rs`:
  - `executables_other_sources_list_are_skipped` now gets pipx, uv, cargo, npm and Snap as real sources and adds
    `/usr/local/bin/tsc` → `/usr/local/lib/node_modules/typescript/bin/tsc`; it also checks that without those
    sources each of their programs would be listed (the claims are what keeps them out).
  - `a_failed_owner_lookup_hides_what_it_could_not_check_and_warns`: a `dpkg-query --search` that fails to run
    gives one warning, leaves `/opt` and `/usr/local/bin` entries out and still lists `~/.local/bin/mine`.
  - `script_installers_are_detected_and_absorb_their_links` runs with npm present: the nvm installer still gets
    `node`, so script installers keep precedence over install roots.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features` (98 tests in housekeeper-linux), `RUSTDOCFLAGS="-D warnings" cargo doc`
  pass. `cargo deny check` was not run (not installed locally); no dependency changed.
- On this Ubuntu machine, `housekeeper list --all --json` before and after the change: identical package lists
  (37 manual items), identical stderr, no warnings.

## Known limits and follow-ups
- Homebrew declares no install roots: the manual source's Homebrew installer item still collects commands linked
  into a Homebrew prefix, as before.
- Script installers (`INSTALLERS` in `manual.rs`) are still a Rust table; moving their detection next to their
  removal recipes is decision D26 (Phase 3.4).
