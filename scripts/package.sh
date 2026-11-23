#!/usr/bin/env bash
# Builds a static housekeeper binary and packages it as .deb, .rpm, Arch .pkg.tar.zst and .tar.gz.
#
#   scripts/package.sh [x86_64|aarch64]      (default: this machine's architecture)
#
# Needs: rustup target <arch>-unknown-linux-musl, nfpm on PATH (or $NFPM).
# Output: dist/ with the four packages and SHA256SUMS. The version comes from Cargo.toml.
set -euo pipefail

cd "$(dirname "$0")/.."

arch="${1:-$(uname -m)}"
case "$arch" in
  x86_64) pkg_arch=amd64 ;;
  aarch64) pkg_arch=arm64 ;;
  *) echo "unsupported architecture: $arch" >&2; exit 2 ;;
esac
target="$arch-unknown-linux-musl"
nfpm="${NFPM:-nfpm}"

version=$(cargo metadata --no-deps --format-version 1 \
  | sed -n 's/.*"name":"housekeeper-cli","version":"\([^"]*\)".*/\1/p')
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }

# Static musl binary: one file that runs on every distribution, whatever its glibc.
cargo build --release --locked --target "$target" -p housekeeper-cli
binary="target/$target/release/housekeeper"

mkdir -p dist target/package
cp "$binary" target/package/housekeeper
export VERSION="$version" ARCH="$pkg_arch"
for packager in deb rpm archlinux; do
  "$nfpm" package --config packaging/nfpm.yaml --packager "$packager" --target dist/
done

tarball="housekeeper-$version-$arch-linux"
staging=$(mktemp -d)
mkdir "$staging/$tarball"
cp "$binary" LICENSE README.md "$staging/$tarball/"
tar -C "$staging" -czf "dist/$tarball.tar.gz" "$tarball"
rm -rf "$staging"

(cd dist && sha256sum -- *.deb *.rpm *.pkg.tar.zst *.tar.gz > SHA256SUMS)
ls -l dist
