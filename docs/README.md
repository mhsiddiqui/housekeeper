# Housekeeper documentation

Everything about the project, in one place. Agents and contributors start here; [AGENTS.md](../AGENTS.md) explains
how these documents are kept up to date.

| Section | Document | What it answers |
|---|---|---|
| **Product** | [product/spec.md](product/spec.md) | What Housekeeper does, for whom, and why; supported distributions; how uninstall works |
| | [design/mockup/index.html](design/mockup/index.html) | Interactive mockup of the desktop app (open in a browser; keep `store-snapshot.js` next to it) |
| **Architecture** | [architecture/README.md](architecture/README.md) | Crates, traits and every design decision (D1–D44) |
| | [architecture/guide.html](architecture/guide.html) | Illustrated guide: composition, and how to add a distribution or operating system (open in a browser) |
| **Planning** | [planning/plan.md](planning/plan.md) | Phases and chunks, with status |
| **Development** | [CONTRIBUTING.md](../CONTRIBUTING.md) | Branch-per-task workflow, checks, testing on real distributions |
| | [development/releasing.md](development/releasing.md) | How a release builds, tests and publishes packages |
| | [development/repository-settings.md](development/repository-settings.md) | GitHub settings: the ruleset that blocks merging until CI is green |
| **History** | [changes/](changes/README.md) | One record per change: what, why, how it was verified, follow-ups |
| | [CHANGELOG.md](../CHANGELOG.md) | User-facing changes per release |

## Which document to update
| You changed… | Update |
|---|---|
| Anything (every branch) | a new record in [changes/](changes/README.md) |
| Something a user notices | [CHANGELOG.md](../CHANGELOG.md), under **Unreleased** |
| A chunk's status | [planning/plan.md](planning/plan.md) |
| How the code is organised, or made a design decision | [architecture/README.md](architecture/README.md), and the guide if a diagram or walkthrough changes |
| What the product does or supports | [product/spec.md](product/spec.md) |
| Install, usage or commands | [README.md](../README.md) |
| Workflow, checks, release process or GitHub settings | [CONTRIBUTING.md](../CONTRIBUTING.md), [development/releasing.md](development/releasing.md), [development/repository-settings.md](development/repository-settings.md) |
