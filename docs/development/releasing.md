# Releasing

A release is a GitHub release with a `vX.Y.Z` tag. Publishing it runs [release.yml](../../.github/workflows/release.yml),
which:

1. checks that the tag matches `version` in the root `Cargo.toml`;
2. builds static binaries for x86_64 and aarch64 and packages them ([packages.yml](../../.github/workflows/packages.yml));
3. installs, runs and removes the packages on every supported distribution (same tests as CI);
4. attaches the packages and `SHA256SUMS` to the release.

If any distribution fails, nothing is attached; fix it, then re-run the workflow from the Actions tab.

## Steps
1. On a branch `chore/release-X.Y.Z`:
   - set `version = "X.Y.Z"` under `[workspace.package]` in `Cargo.toml` and run `cargo check` to update `Cargo.lock`;
   - in `CHANGELOG.md`, rename **Unreleased** to `X.Y.Z (YYYY-MM-DD)` and add a new empty **Unreleased** section;
   - add a change record in [docs/changes/](../changes/README.md) for the release.
2. Open a pull request, wait for CI, merge. Note the merge commit: the release is built from it, even if other pull
   requests are merged after it.
3. Tag that commit and push the tag (pushing a tag runs no workflow):

   ```sh
   git fetch origin
   git tag -a vX.Y.Z <merge-commit> -m "Housekeeper X.Y.Z"
   git push origin vX.Y.Z
   ```

   Do not let GitHub create the tag: it tags the tip of `main`, which may already hold work the changelog lists under
   **Unreleased** (0.2.0 was first published that way and withdrawn, change 0052).
4. On GitHub: **Releases → Draft a new release → Choose a tag →** the existing `vX.Y.Z`. Title `Housekeeper X.Y.Z`,
   the changelog section as the description, **Publish release**.
5. Watch the **Release** workflow; the assets appear on the release when it finishes. Then download them, check
   `sha256sum -c SHA256SUMS` and the binary's `--version`, and record the release in a change record.

If a published release turns out to be wrong, delete the release on GitHub (with its assets), then delete and recreate
the tag (`git push origin :refs/tags/vX.Y.Z`), and publish again. Anyone who downloaded in between has the old build,
so say so in the change record.

Versions follow [Semantic Versioning](https://semver.org/). Before 1.0, minor versions may change the JSON output
(its `schema` field is bumped when they do).

## What gets attached
| File | For |
|---|---|
| `housekeeper_X.Y.Z-1_amd64.deb`, `…_arm64.deb` | Debian, Ubuntu and derivatives |
| `housekeeper-X.Y.Z-1.x86_64.rpm`, `….aarch64.rpm` | Fedora, RHEL/AlmaLinux/Rocky, openSUSE |
| `housekeeper-X.Y.Z-1-x86_64.pkg.tar.zst` | Arch Linux and derivatives |
| `housekeeper-X.Y.Z-<arch>-linux.tar.gz` | Anything else, including image-based systems (Silverblue) |
| `SHA256SUMS` | Checksums of all of the above |

Every binary is statically linked (musl), so one build runs on all distributions regardless of their glibc.

## Not yet automated
- Package signing (GPG): needs a signing key stored as a repository secret.
- Publishing to the AUR, a Fedora COPR or an apt repository: packages are attached to the release only.
