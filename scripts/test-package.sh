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
helper=/usr/lib/housekeeper/housekeeper-helper
if [ -e /run/ostree-booted ]; then
  native=rpm
  tool=rpm-ostree
  installed=no
  pkg=$(one "$dist/*.tar.gz")
  tar -xzf "$pkg" -C /tmp
  PATH="$(dirname "$(one "/tmp/housekeeper-*/housekeeper")"):$PATH"
  helper=$(one "/tmp/housekeeper-*/housekeeper-helper")
# The package has no dependencies, so it installs without contacting any repository: no mirror or expired-key
# problem can break the test. Housekeeper's root helper removes it again at the end, the same way.
elif command -v apt-get >/dev/null; then
  native=dpkg
  tool=apt
  pkg=$(one "$dist/*.deb")
  export DEBIAN_FRONTEND=noninteractive
  apt-get install -y -qq "$pkg" >/dev/null
  planned='apt-get remove --yes housekeeper'
elif command -v zypper >/dev/null; then
  native=rpm
  tool=zypper
  pkg=$(one "$dist/*.rpm")
  # --no-remote: zypper would otherwise build its online repositories' caches first (fetching their keys), and a
  # flaky mirror fails the install with exit code 106 although the local package needs nothing from them.
  zypper --non-interactive --no-remote install --allow-unsigned-rpm "$pkg"
  planned='zypper --non-interactive --no-remote remove housekeeper-'
elif command -v dnf >/dev/null; then
  native=rpm
  tool=dnf
  pkg=$(one "$dist/*.rpm")
  dnf install -y --disablerepo='*' "$pkg"
  planned='dnf remove --assumeyes --disablerepo=\* housekeeper-'
elif command -v pacman >/dev/null; then
  native=pacman
  tool=pacman
  pkg=$(one "$dist/*.pkg.tar.zst")
  pacman -U --noconfirm "$pkg"
  planned='pacman --remove --noconfirm housekeeper'
else
  fail "no supported package manager"
fi

# The root helper's request for removing `$1` with this system's package tool.
request() { printf '{"protocol":1,"operation":{"kind":"remove-packages","tool":"%s","packages":["%s"]}}' "$tool" "$1"; }
# Runs the helper on that request (the tests run as root, as pkexec would start it), keeping its reply in
# /tmp/helper.json; prints its exit code.
run_helper() { code=0; request "$1" | "$helper" > /tmp/helper.json || code=$?; echo "$code"; }

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

# The root helper refuses what is not a plain, installed package of this system's tool, and changes nothing.
[ "$(run_helper housekeeper-no-such-package)" = 2 ] \
  && grep -q '"reason":"housekeeper-no-such-package is not installed"' /tmp/helper.json \
  || { cat /tmp/helper.json >&2; fail "the helper did not refuse a package that is not installed"; }
[ "$(run_helper --purge)" = 2 ] && grep -q 'is not a package name' /tmp/helper.json \
  || { cat /tmp/helper.json >&2; fail "the helper did not refuse an option passed as a package name"; }

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
  # `show` lists the package's files through the real database (dpkg-query -L, rpm -ql, pacman's files) and asks it
  # what needs the package.
  housekeeper show housekeeper --files > /tmp/show.txt || { cat /tmp/show.txt >&2; fail "housekeeper show found nothing for itself"; }
  grep -qx '/usr/bin/housekeeper' /tmp/show.txt \
    || { cat /tmp/show.txt >&2; fail "housekeeper show did not list /usr/bin/housekeeper among its files"; }
  grep -q '^Needed by: *nothing installed$' /tmp/show.txt \
    || { cat /tmp/show.txt >&2; fail "housekeeper show could not tell what needs it"; }
  # The removal plan goes through this distribution's own package tool, run by the root helper.
  grep -q "as root, by Housekeeper's helper: $planned" /tmp/show.txt \
    || { cat /tmp/show.txt >&2; fail "housekeeper show did not plan its removal with: $planned"; }
  # The package manager (or the RPM database) was asked what else would go: nothing needs housekeeper.
  grep -q '^ *removes nothing else, says ' /tmp/show.txt \
    || { cat /tmp/show.txt >&2; fail "housekeeper show did not simulate its own removal"; }
  # A dry run prints the same plan with its leftovers and changes nothing; without --dry-run nothing is removed yet.
  mkdir -p "$HOME/.config/housekeeper" && echo 'x' > "$HOME/.config/housekeeper/settings"
  housekeeper remove --dry-run housekeeper > /tmp/dry-run.txt \
    || { cat /tmp/dry-run.txt >&2; fail "housekeeper remove --dry-run found nothing for itself"; }
  grep -q "^Leftovers: *$HOME/.config/housekeeper (settings" /tmp/dry-run.txt \
    || { cat /tmp/dry-run.txt >&2; fail "housekeeper remove --dry-run did not list its settings folder"; }
  grep -q '^Nothing was changed' /tmp/dry-run.txt || fail "housekeeper remove --dry-run did not say it changed nothing"
  refused=0
  housekeeper remove housekeeper 2>/dev/null || refused=$?
  [ "$refused" = 3 ] || fail "housekeeper remove without --dry-run exited with $refused, not 3 (not available yet)"
  command -v housekeeper >/dev/null || fail "housekeeper remove changed something"
  rm -r "$HOME/.config/housekeeper"
  [ -f /usr/share/polkit-1/actions/io.github.mhsiddiqui.housekeeper.policy ] \
    || fail "the package did not install the helper's polkit action"
  # The helper removes the package, under the id the native source gives it, with the command the plan showed.
  housekeeper show housekeeper --json > /tmp/show.json
  id=$(sed -n 's/^    "id": "\(.*\)",$/\1/p' /tmp/show.json | head -n 1)
  [ -n "$id" ] || { cat /tmp/show.json >&2; fail "no package id in housekeeper show --json"; }
  [ "$(run_helper "$id")" = 0 ] && grep -q '"outcome":"done"' /tmp/helper.json \
    || { cat /tmp/helper.json >&2; fail "the helper did not remove $id"; }
  # Check the files themselves: the shell may still remember where the command was.
  [ ! -e /usr/bin/housekeeper ] || fail "/usr/bin/housekeeper is still there after the helper removed the package"
  [ ! -e "$helper" ] || fail "$helper is still there after the helper removed the package"
fi
echo "OK: $(basename "$pkg") on $PRETTY_NAME"
