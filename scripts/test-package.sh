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

# The scan keeps its record of items seen in the user's state folder; nothing has been changed yet.
grep -q '"key":"'"$native"':' "${XDG_STATE_HOME:-$HOME/.local/state}/housekeeper/seen.jsonl" \
  || fail "the scan did not record the $native packages it saw"
housekeeper history | grep -q '^Housekeeper has not changed anything' || fail "housekeeper history is not empty"

# The root helper refuses what is not a plain, installed package of this system's tool, and changes nothing.
[ "$(run_helper housekeeper-no-such-package)" = 2 ] \
  && grep -q '"reason":"housekeeper-no-such-package is not installed"' /tmp/helper.json \
  || { cat /tmp/helper.json >&2; fail "the helper did not refuse a package that is not installed"; }
[ "$(run_helper --purge)" = 2 ] && grep -q 'is not a package name' /tmp/helper.json \
  || { cat /tmp/helper.json >&2; fail "the helper did not refuse an option passed as a package name"; }
# It installs a package file only from its own copy, and only when the copy has the checksum the file was inspected
# with; anything else is refused before a package tool runs.
printf '{"protocol":1,"operation":{"kind":"install-package-file","tool":"%s","file":"%s","sha256":"%064d"}}' \
  "$tool" "$pkg" 0 | "$helper" > /tmp/helper.json && fail "the helper installed a file whose checksum differs"
grep -q 'changed since it was inspected' /tmp/helper.json \
  || { cat /tmp/helper.json >&2; fail "the helper did not refuse a file whose checksum differs"; }

# A script is not run: `install` refuses it, says why, and changes nothing (exit 3).
printf '#!/bin/sh\ntouch /tmp/script-ran\n' > /tmp/setup.sh
code=0
housekeeper install --yes /tmp/setup.sh > /tmp/script.txt 2>&1 || code=$?
[ "$code" = 3 ] && grep -q 'does not run it' /tmp/script.txt && [ ! -e /tmp/script-ran ] \
  || { cat /tmp/script.txt >&2; fail "housekeeper install of a script exited with $code, not a refusal"; }

if [ "$installed" = yes ]; then
  # The package just installed is this system's own format, read with the format's tool (dpkg-deb, rpm, bsdtar),
  # built for this processor. Its install plan goes through the package tool and knows this version is installed
  # already; rpm refuses to install it again, so on RPM systems the plan is blocked (exit 1).
  version=$(housekeeper --version | cut -d' ' -f2)
  code=0
  housekeeper inspect "$pkg" > /tmp/inspect.txt || code=$?
  [ "$code" -le 1 ] || { cat /tmp/inspect.txt >&2; fail "housekeeper inspect exited with $code for $pkg"; }
  grep -q '^Fits: *yes$' /tmp/inspect.txt \
    || { cat /tmp/inspect.txt >&2; fail "housekeeper inspect says $pkg does not fit this system"; }
  grep -q "^Package: *housekeeper $version" /tmp/inspect.txt \
    || { cat /tmp/inspect.txt >&2; fail "housekeeper inspect did not read the name and version inside $pkg"; }
  grep -q "^Install: *as root, by Housekeeper's helper: $tool" /tmp/inspect.txt \
    || { cat /tmp/inspect.txt >&2; fail "housekeeper inspect did not plan installing $pkg with $tool"; }
  grep -q 'this version is installed already' /tmp/inspect.txt \
    || { cat /tmp/inspect.txt >&2; fail "housekeeper inspect did not see that $pkg is installed already"; }
  grep -q "^SHA-256: *$(sha256sum "$pkg" | cut -d' ' -f1)$" /tmp/inspect.txt \
    || { cat /tmp/inspect.txt >&2; fail "housekeeper inspect did not give the checksum sha256sum gives for $pkg"; }

  # Installing it again goes through the root helper (run directly, as the tests run as root), which installs its own
  # copy of the file. apt finds it installed already and pacman reinstalls it; rpm refuses the version installed, so
  # on RPM systems the plan is blocked and nothing changes (exit 3). Either way the history says so.
  code=0
  housekeeper install --yes "$pkg" > /tmp/install.txt 2>&1 || code=$?
  if [ "$native" = rpm ]; then
    [ "$code" = 3 ] && grep -q 'housekeeper is not installed: the plan is blocked' /tmp/install.txt \
      || { cat /tmp/install.txt >&2; fail "housekeeper install of the installed rpm exited with $code, not 3"; }
  else
    [ "$code" = 0 ] && grep -q "^done: *as root, by Housekeeper's helper: $tool" /tmp/install.txt \
      || { cat /tmp/install.txt >&2; fail "housekeeper install did not install $pkg through the helper"; }
    grep -q '"change":"install".*"outcome":"done"' "${XDG_STATE_HOME:-$HOME/.local/state}/housekeeper/history.jsonl" \
      || fail "the history does not record the install"
  fi
  [ -z "$(ls -A /var/cache/housekeeper 2>/dev/null)" ] || fail "the helper left its copy in /var/cache/housekeeper"

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
  # Housekeeper's own state folder (its record of items seen) is listed too, so the settings folder is not
  # necessarily the first line.
  grep -q "$HOME/.config/housekeeper (settings" /tmp/dry-run.txt \
    || { cat /tmp/dry-run.txt >&2; fail "housekeeper remove --dry-run did not list its settings folder"; }
  grep -q '^Nothing was changed' /tmp/dry-run.txt || fail "housekeeper remove --dry-run did not say it changed nothing"
  # Without a terminal to ask on, removing needs --yes; nothing changes without it.
  refused=0
  housekeeper remove housekeeper < /dev/null 2>/dev/null || refused=$?
  [ "$refused" = 3 ] || fail "housekeeper remove without --yes exited with $refused, not 3 (nothing done)"
  command -v housekeeper >/dev/null || fail "housekeeper remove without --yes changed something"
  [ -f /usr/share/polkit-1/actions/io.github.mhsiddiqui.housekeeper.policy ] \
    || fail "the package did not install the helper's polkit action"
  # Housekeeper removes itself the way its plan says: the package through the root helper (run directly, as the
  # tests run as root), then every leftover folder to the Trash, then a line in the history.
  housekeeper remove --yes --leftovers all housekeeper > /tmp/remove.txt 2>&1 \
    || { cat /tmp/remove.txt >&2; fail "housekeeper remove did not remove its own package"; }
  grep -q "^done: *as root, by Housekeeper's helper: $planned" /tmp/remove.txt \
    || { cat /tmp/remove.txt >&2; fail "housekeeper remove did not run the planned command through the helper"; }
  # Check the files themselves: the shell may still remember where the command was.
  [ ! -e /usr/bin/housekeeper ] || fail "/usr/bin/housekeeper is still there after housekeeper remove"
  [ ! -e "$helper" ] || fail "$helper is still there after housekeeper remove"
  [ ! -e "$HOME/.config/housekeeper" ] || fail "the settings folder was not moved to the Trash"
  grep -qx "Path=$HOME/.config/housekeeper" "$HOME"/.local/share/Trash/info/*.trashinfo \
    || fail "the Trash does not say where the settings folder came from"
  history="${XDG_STATE_HOME:-$HOME/.local/state}/housekeeper/history.jsonl"
  grep -q '"item":"'"$native"':housekeeper.*"outcome":"done"' "$history" \
    || fail "the history does not record the removal"
fi
echo "OK: $(basename "$pkg") on $PRETTY_NAME"
