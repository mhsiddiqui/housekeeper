# Change records

One record per change (normally one per branch / pull request), numbered in order. They are the project's track
record: what changed, why, how it was verified, and what was left for later. User-facing release notes live in
[CHANGELOG.md](../../CHANGELOG.md); these records are for the people and agents working on the code.

## Adding a record
1. Copy [TEMPLATE.md](TEMPLATE.md) to `NNNN-short-title.md`, using the next number below.
2. Fill it in on the branch as the work happens; add the commit hashes before merging.
3. Add a row to the table below.

Records are not rewritten after merging. If a later change undoes or fixes something, the new record says so and
links back.

| # | Change | Plan chunks | Branch / PR |
|---|---|---|---|
| [0001](0001-product-spec-and-plan.md) | Product spec, architecture, build plan and mockup | 0.1 | `main` |
| [0002](0002-workspace-ci-and-core.md) | Workspace, CI and the OS-independent core crate | 0.1, 0.2, 1.1 | `main` |
| [0003](0003-linux-platform-and-dpkg.md) | Distro detection, Linux platform and dpkg source | 1.2 | `main` |
| [0004](0004-cli.md) | `housekeeper sources` and `housekeeper list` | 1.7 | `main` |
| [0005](0005-flatpak-and-snap.md) | Flatpak and Snap sources | 1.4 | `main` |
| [0006](0006-rpm-and-pacman.md) | rpm and pacman sources | 1.3 | `main` |
| [0007](0007-ci-distro-smoke-test.md) | Smoke test in real distro containers | 1.8 | `main` |
| [0008](0008-language-tool-sources.md) | npm, pipx, uv tool, pip --user and cargo sources | 1.5 | `main` |
| [0009](0009-appimage-and-manual-sources.md) | AppImage and manual-install sources | 1.6 | `main` |
| [0010](0010-test-and-release-pipeline.md) | Full test pipeline and release packages | 0.3, 0.4 | #1 |
| [0011](0011-architecture-guide.md) | Illustrated architecture guide | none | with 0012 |
| [0012](0012-docs-section-and-agents.md) | Documentation section, change records and AGENTS.md | none | #2 |
| [0013](0013-merge-gate.md) | Block merging until every CI job is green | 0.5 | #3 |
| [0014](0014-move-design-into-docs.md) | Move the design mockup into the docs section | none | #4 (into a branch), reaches `main` with 0016 |
| [0015](0015-homebrew-and-one-file-providers.md) | Homebrew on Linux, and providers that are one file | 1.9, 2.3 (part) | #5 (into a branch), reaches `main` with 0016 |
| [0016](0016-ci-for-main-only.md) | CI only for `main`, and bringing stacked work into `main` | 0.5 | #6 |
| [0017](0017-merge-gate-active.md) | Merge gate active on `main` | 0.5 | #7 |
| [0018](0018-license-gpl3.md) | Licence changed to GPL-3.0-or-later | 0.1 | `chore/license-gpl3` |
| [0019](0019-drop-remote-computers.md) | Drop managing other computers over SSH | 12 (scope) | #9 |
| [0020](0020-manual-claims.md) | Manual source leaves out what other sources claim | 1.6, 2.3 | #13 |
| [0021](0021-review-decisions.md) | Decisions for package tools, removal privileges and script-installer recipes | 3.1, 3.4, 4.1 | #10 |
| [0022](0022-source-registration.md) | Register sources with `register!` instead of scanning their text | none | #12 |
| [0023](0023-platform-crate.md) | Core builds on any OS; `housekeeper-platform` picks the platform | none | #15 |
| [0024](0024-scan-progress.md) | Report each source's result as soon as it finishes | none (prepares 6.3) | #14 |
| [0025](0025-spec-architecture-link.md) | Spec points to the architecture instead of its own outdated design | none | #11 |
| [0026](0026-architecture-diagram.md) | Redraw the architecture diagrams in the guide | none | #16 |
| [0027](0027-readme.md) | Complete the README for the first release | 0.4 | #17 |
| [0028](0028-format-sources.md) | Format and check the source files cargo fmt never reached | none | #18 |
