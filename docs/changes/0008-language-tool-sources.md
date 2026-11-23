# 0008: npm, pipx, uv tool, pip --user and cargo sources

- **Date:** 2026-11-23
- **Branch / PR:** committed directly to `main`
- **Plan chunks:** 1.5
- **Commits:** `803befd`

## What changed
- `NpmSource`: global packages of system Node, `$NPM_CONFIG_PREFIX`, `~/.npm-global`, every nvm and fnm version;
  scoped packages; bundled `npm`/`corepack` skipped; ids unique per Node install.
- `PipxSource`: venv `pipx_metadata.json` (suffix installs, apps as commands, Python version as origin).
- `UvToolSource`: `uv-receipt.toml` receipts, versions from the tool venv's dist-info.
- `PipUserSource`: user-site dist-info per Python version; `REQUESTED` marks explicit installs (PEP 376).
- `CargoSource`: `.crates2.json`, falling back to `.crates.toml`; registry, git and path origins.

## Why
Developer machines install many command-line tools this way, and no other app manager lists them.

## How it was verified
70 Linux-crate tests in total. Real run on Ubuntu 26.04: 14 npm globals across nvm v22.18.0 and v22.19.0,
pipx `activity-sim`, uv `graphifyy`, cargo `rmux` and `cargo-packager`, 21 pip user packages across Python 3.8, 3.11
and 3.14.

## Known limits and follow-ups
- Read from metadata files only; no `node`/`python`/`cargo` process is started (decision D12).
- bun and pnpm globals are not covered yet.
