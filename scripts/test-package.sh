#!/bin/sh
# Installs the package that fits this distribution, checks housekeeper against the real package database,
# then removes the package again. Run as root inside a distribution container:
#
#   scripts/test-package.sh <dist-dir>
#
# On image-based systems (/run/ostree-booted exists), packages are layered with rpm-ostree and need a reboot, so the
# release tarball is checked instead.
set -eu

# Absolute path: apt-get treats `dist/x.deb` (no leading ./ or /) as a package name, not a file.
dist=$(cd "${1:?usage: test-package.sh <dist-dir>}" && pwd)

fail() { echo "FAIL: $*" >&2; exit 1; }
one() { set -- $1; [ -e "$1" ] || fail "no file matching $1"; echo "$1"; }

. /etc/os-release
echo "== $PRETTY_NAME ($(uname -m))"

installed=yes
if [ -e /run/ostree-booted ]; then
  native=rpm
  installed=no
  pkg=$(one "$dist/*.tar.gz")
  tar -xzf "$pkg" -C /tmp
  PATH="$(dirname "$(one "/tmp/housekeeper-*/housekeeper")"):$PATH"
# The package has no dependencies, so it installs without contacting any repository: no mirror or expired-key
# problem can break the test.
elif command -v apt-get >/dev/null; then
  native=dpkg
  pkg=$(one "$dist/*.deb")
  export DEBIAN_FRONTEND=noninteractive
  apt-get install -y -qq "$pkg" >/dev/null
  remove() { apt-get remove -y -qq housekeeper >/dev/null; }
elif command -v zypper >/dev/null; then
  native=rpm
  pkg=$(one "$dist/*.rpm")
  zypper --non-interactive --no-refresh install --allow-unsigned-rpm "$pkg"
  remove() { zypper --non-interactive --no-refresh remove housekeeper; }
elif command -v dnf >/dev/null; then
  native=rpm
  pkg=$(one "$dist/*.rpm")
  dnf install -y --disablerepo='*' "$pkg"
  remove() { dnf remove -y --disablerepo='*' housekeeper; }
elif command -v pacman >/dev/null; then
  native=pacman
  pkg=$(one "$dist/*.pkg.tar.zst")
  pacman -U --noconfirm "$pkg"
  remove() { pacman -R --noconfirm housekeeper; }
else
  fail "no supported package manager"
fi

command -v housekeeper >/dev/null || fail "housekeeper is not on PATH after installing $pkg"
housekeeper --version
housekeeper sources

# The native source works on this release's real database.
housekeeper list --all --source "$native" --json > /tmp/native.json
grep -q '"state": "ok"' /tmp/native.json || fail "$native source did not read the package database"
count=$(housekeeper list --all --source "$native" | wc -l)
[ "$count" -gt 50 ] || fail "only $count lines from $native"

# No source fails, including ones this system lacks.
housekeeper list --all --json > /tmp/all.json
if grep -q '"state": "failed"' /tmp/all.json; then
  grep -B3 -A3 '"state": "failed"' /tmp/all.json >&2
  fail "a source failed"
fi

if [ "$installed" = yes ]; then
  # Housekeeper sees itself, installed by the native package manager.
  housekeeper list --all --source "$native" --search housekeeper | grep -q '^housekeeper ' \
    || fail "housekeeper does not list its own package"
  # Its command, found through the batched ownership query, makes it an app the default list shows.
  housekeeper list --source "$native" --search housekeeper | grep -q '^housekeeper ' \
    || fail "the default list leaves out housekeeper, though its package puts a command on the PATH"
  grep -q '"kind": "component"' /tmp/native.json \
    || fail "no $native package was told apart as a component (no command, no launcher)"
  # `who` traces the command to its package through the real ownership query (dpkg-query, rpm -qf, pacman -Qo).
  housekeeper who housekeeper > /tmp/who.txt || { cat /tmp/who.txt >&2; fail "housekeeper who found no owner for itself"; }
  grep -q 'lists /usr/bin/housekeeper as a file of housekeeper$' /tmp/who.txt \
    || { cat /tmp/who.txt >&2; fail "housekeeper who did not name its own package"; }
  remove
  # Check the file itself: the shell may still remember where the command was.
  [ ! -e /usr/bin/housekeeper ] || fail "/usr/bin/housekeeper is still there after removing the package"
fi
echo "OK: $(basename "$pkg") on $PRETTY_NAME"
