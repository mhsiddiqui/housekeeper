# AGENTS.md

Instructions for AI coding agents (and humans) working on Housekeeper. The source of truth is the documentation
section, [docs/](docs/README.md); this file says how to use it and how to keep it current.

Housekeeper shows every app installed on a Linux machine, whatever installed it, and (in later phases) removes each one
the way its own installer intended. Rust workspace: `crates/core` (no OS knowledge), `crates/linux` (one `Source` per
way software gets installed), `crates/platform` (picks the OS crate for front ends), `crates/cli` (the `housekeeper`
command).

## Before you start
1. Read [docs/README.md](docs/README.md), then:
   - [docs/architecture/README.md](docs/architecture/README.md): crates, traits and decisions D1–D27. Do not contradict
     a decision; if one must change, change it there in the same branch and say why.
   - [docs/planning/plan.md](docs/planning/plan.md): find the chunk you are working on and its status.
   - The latest records in [docs/changes/](docs/changes/README.md): what was done last and what was left open.
2. Pick one chunk (or one fix). One task = one branch = one pull request = one change record.

## Workflow
1. Branch from an up-to-date `main`: `<type>/<short-description>` (`feat/desktop-entries`, `fix/snap-timeout`,
   `docs/…`, `ci/…`). Do not stack branches: if a task needs unmerged work, wait until that pull request is merged
   into `main`, then branch from the new `main`.
2. Commit in small steps with Conventional Commit subjects (`feat(linux): …`, `fix(cli): …`, `docs: …`). Never commit to
   `main` directly, never force-push shared branches.
3. Push the branch and open a pull request **against `main`**, always. CI runs only for pull requests into `main` (and
   pushes to it), and GitHub blocks the merge until the `ci-ok` check is green, i.e. every CI job passed
   ([repository settings](docs/development/repository-settings.md)). Never ask for the rule to be bypassed.

## Definition of done
A branch is ready for review only when all of these hold:

- [ ] The change works, with tests for behaviour a user would notice (wrong install reason, missed packages, crashes on
      unusual input). No tests for wiring, echoes or tautologies.
- [ ] All checks pass locally (commands below).
- [ ] **A new change record** in `docs/changes/NNNN-short-title.md` from [TEMPLATE.md](docs/changes/TEMPLATE.md), with
      its row added to [docs/changes/README.md](docs/changes/README.md) and commit hashes filled in. Report only what
      was actually run and observed under "How it was verified".
- [ ] [docs/planning/plan.md](docs/planning/plan.md): chunk status updated (⬜ → 🚧 → ✅).
- [ ] [CHANGELOG.md](CHANGELOG.md): entry under **Unreleased** if users would notice the change.
- [ ] [docs/architecture/README.md](docs/architecture/README.md) (and [guide.html](docs/architecture/guide.html) if a
      diagram or walkthrough changes): any design decision added or changed.
- [ ] [README.md](README.md): install or usage changes.
- [ ] Every link you added or moved resolves.

The full "which document to update" table is in [docs/README.md](docs/README.md#which-document-to-update).

## Code rules
- **Composition:** a new way of installing software is **one new file** in `crates/linux/src/sources/` with a type that
  implements `Source` and `LinuxSource` and a `register!(<Type>);` line; `build.rs` lists it, so no list is edited
  (D20). Sources never import from each other; shared code lives outside `sources/`. A new distribution is the
  same: its native package manager's file declares `LinuxSource::NATIVE` (families and `os-release` IDs) and
  implements `OwnerLookup` (see the [guide](docs/architecture/guide.html#new-distro)). A new operating system is a new
  crate implementing `Platform`, registered in `housekeeper-platform` (D27). Core never mentions an OS, distribution
  or package manager, and must build on every OS (Unix-only code behind `cfg(unix)`).
- **Optional abilities** (owner lookup, removal, install, update) are separate traits returned as
  `Option<&dyn …>` from a source, never methods every source must stub.
- **All machine access goes through `Host`**: logical absolute paths (`host.read_to_string`, `host.read_dir`, …) and
  `host.run`/`run_ok` with `Cmd` (argv only, never a shell, always a timeout). No direct `std::fs` or
  `std::process` in sources.
- **Read-only until Phase 4.** Nothing changes the system until the privileged helper exists (decision D4).
- Prefer stable machine interfaces and files over parsing human output (D1, D12). Each source reports its own failure
  instead of failing the scan (D14).
- Rust 1.85 is the minimum (`rust-version`): no features newer than that (for example no let-chains).
- `clippy -D warnings` clean, `rustfmt` (max width 120), no `unwrap`/`expect` outside tests, no dead code, no TODOs.
- New dependencies need a reason in the change record and must pass `cargo deny` (permissive, GPL-compatible
  licences only). Housekeeper itself is GPL-3.0-or-later (D21); never copy code from GPL-incompatible projects.
- No sign-in or telemetry (D15).
- Tests use `housekeeper_core::testing::{Fixture, FakeRunner}`; follow `crates/linux/src/sources/dpkg.rs`.

## Checks
```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo deny check
```
Packages on a real distribution (what CI does for every supported release, x86_64 and aarch64):
```sh
scripts/package.sh                 # needs the musl target and nfpm
docker run --rm -v "$PWD:/w:ro" -w /w fedora:latest sh scripts/test-package.sh dist
```
If `cc` on your `PATH` is not the system C compiler (a wrapper script, for example), prefix Cargo commands with
`CC=/usr/bin/cc CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/cc`.

## Don't
- Don't rewrite merged change records; add a new one that links back.
- Don't commit build output (`target/`, `dist/`).
- Don't mark a chunk done, or write "verified", without having run the check.
