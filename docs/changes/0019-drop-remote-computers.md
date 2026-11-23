# 0019: Drop managing other computers over SSH

- **Date:** 2026-11-23
- **Branch / PR:** `docs/drop-remote-computers`, #9
- **Plan chunks:** Phase 12 (scope)
- **Commits:** 0cd4e18

## What changed
- Decision D22: managing other computers over SSH is not planned. `Host` and `CommandRunner` stay local; the runner is
  swapped only for tests.
- Mockup (`docs/design/mockup/index.html`): removed the sample machines `build-box` (Fedora) and `media-pc` (Arch), their
  "· SSH" sidebar groups, the "Other computers (SSH)" Settings card, the "Connected over SSH" callout, the remote
  terminal prompt, the "Manage your other Linux computers" blog entry and the `--host build-box` CLI example. The
  sidebar now has one "This computer" group.
- Spec: §15 is now "Command line" only (no `--host`); the sidebar, Settings and roadmap no longer mention SSH machines.
- Plan: Phase 12 renamed "Backup and other platforms" (Gist backup, macOS and Windows crates).
- Architecture README and guide: the `Host` section and the composition diagram no longer promise an SSH runner;
  decision count D1–D22 updated in AGENTS.md, `docs/README.md` and the guide.
- `crates/core/src/command.rs`: the `CommandRunner` doc comment no longer mentions an SSH runner.

## Why
The owner decided not to add support for other computers for now. The documents also disagreed on how it would work:
the architecture said "swap the runner", the plan said "run `housekeeper --json` remotely", the spec said "nothing is
installed on them". Swapping the runner alone could not work, because `Host` reads files locally and the Snap source
talks to a local socket. Removing the feature removes the contradiction; bringing it back needs a new decision that
picks one design.

## How it was verified
- The edited mockup and the copy on `main` were rendered with headless Chromium (`chrome-headless-shell --dump-dom`).
  Edited: no console errors; sidebar shows only "This computer · Ubuntu" with Installed 38, the same per-source counts
  and Updates 9 as on `main`; the terminal prompt is `muhammad-hassan@ubuntu`; `build-box`, `media-pc`,
  `Other computers (SSH)`, `over SSH` and `--host` no longer appear anywhere in the page (10, 10, 2, 4 and 2 times on
  `main`).
- `cargo fmt --all --check` and `RUSTDOCFLAGS="-D warnings" cargo doc -p housekeeper-core --no-deps` pass.
- The distro picker and the click handlers after the change were not exercised in a browser (omp's WebKit crashes on
  this page, on `main` too).

## Known limits and follow-ups
- Export/import of the app list to set up a new computer (spec §7 and §19) is unaffected; it is a file, not a remote
  connection.
