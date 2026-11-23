#!/bin/sh
# Checks the Homebrew source against a real Homebrew. Run as root inside the official image:
#
#   docker run --rm --user root -v "$PWD:/w" -w /w homebrew/brew:latest sh scripts/test-homebrew.sh dist
#
# Installs jq with brew (as the image's linuxbrew user), then checks that housekeeper lists jq as installed on purpose
# and its dependency oniguruma as a dependency.
set -eu

dist=$(cd "${1:?usage: test-homebrew.sh <dist-dir>}" && pwd)
prefix=/home/linuxbrew/.linuxbrew

fail() { echo "FAIL: $*" >&2; exit 1; }

set -- "$dist"/housekeeper-*-x86_64-linux.tar.gz
[ -e "$1" ] || fail "no x86_64 tarball in $dist"
tar -xzf "$1" -C /tmp
set -- /tmp/housekeeper-*/housekeeper
hk=$1

su linuxbrew -c "HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 $prefix/bin/brew install --quiet jq"

"$hk" sources
"$hk" list --all --source homebrew
"$hk" list --all --source homebrew --json > /tmp/brew.json
grep -q '"state": "ok"' /tmp/brew.json || fail "the homebrew source did not read the installation"

# jq itself (installed just now by brew) reads the scan.
jq="$prefix/bin/jq"
"$jq" -e '.packages[] | select(.id == "formula/jq") | .reason == "explicit" and .kind == "app"' /tmp/brew.json \
  >/dev/null || fail "jq is not listed as an app installed on purpose"
"$jq" -e '.packages[] | select(.id == "formula/oniguruma") | .reason == "dependency"' /tmp/brew.json \
  >/dev/null || fail "oniguruma is not listed as a dependency"
"$hk" list --source homebrew | grep -q '^jq ' || fail "jq is missing from the default list"
echo "OK: Homebrew formulae and their install reasons are read correctly"
