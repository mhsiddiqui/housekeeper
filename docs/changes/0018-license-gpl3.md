# 0018: Licence changed to GPL-3.0-or-later

- **Date:** 2026-11-23
- **Branch / PR:** `chore/license-gpl3`
- **Plan chunks:** 0.1 (licence)
- **Commits:** see the pull request

## What changed
- `LICENSE`: the GNU General Public License v3 text, replacing MIT.
- `Cargo.toml` (`license = "GPL-3.0-or-later"`, inherited by all crates) and `packaging/nfpm.yaml`, so `.deb`, `.rpm`
  and Arch packages declare the new licence.
- `deny.toml`: own crates (unpublished) are skipped by the licence check; dependencies must still be MIT, Apache-2.0
  or Unicode-3.0.
- README licence notice, CONTRIBUTING "Licence of contributions", AGENTS.md dependency rule, decision D21,
  CHANGELOG, plan row 0.1.

## Why
The owner does not want anyone to be able to take Housekeeper into closed source. MIT allows that; the GPL requires
every distributed version to stay open (decision D21). The owner is the only author so far, so the licence can be
changed without asking anyone.

## How it was verified
- The licence text is Ubuntu's `/usr/share/common-licenses/GPL-3` (gnu.org was unreachable); it matches the copy
  published by GitHub's choosealicense.com word for word (674 lines).
- `cargo metadata` reports `GPL-3.0-or-later` for housekeeper-core, -linux and -cli; `cargo deny check` passes
  (advisories, bans, licences, sources ok).

## Known limits and follow-ups
- Copies of earlier commits stay available under MIT to whoever already has them; the GPL applies from this change on.
- Source files do not carry per-file licence headers; `LICENSE`, the crate metadata and the README notice state the
  licence.
