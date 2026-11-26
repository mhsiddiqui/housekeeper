//! Queries on the RPM database shared by the rpm source and the tools that change it (dnf, zypper).

use std::collections::BTreeSet;

use housekeeper_core::{Cmd, Host, SourceError};

/// A package's id as the rpm source builds it: `name-[epoch:]version-release[.arch]`.
pub(crate) const ID_FORMAT: &str = "%{NAME}-%|EPOCH?{%{EPOCH}:}|%{VERSION}-%{RELEASE}%|ARCH?{.%{ARCH}}|\\n";

/// What a package offers that others can require: each capability it provides, then each of its files.
pub(crate) const OFFERS_FORMAT: &str = "[%{PROVIDES}\\n][%{FILENAMES}\\n]";

/// The ids of the installed packages that directly require anything the packages `ids` provide, or one of their
/// programs (`/usr/bin/sed`, `/bin/sh`; files elsewhere are rarely required and would make the query huge), `ids` left
/// out.
pub(crate) fn direct_dependants(host: &Host, ids: &[&str]) -> Result<BTreeSet<String>, SourceError> {
    let offers = host.run_ok(
        &Cmd::new("rpm")
            .args(["-q", "--queryformat", OFFERS_FORMAT])
            .args(ids.iter().copied()),
    )?;
    let mut capabilities: Vec<&str> = offers
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && (!line.starts_with('/') || is_program(line)))
        .collect();
    capabilities.sort_unstable();
    capabilities.dedup();
    if capabilities.is_empty() {
        return Ok(BTreeSet::new());
    }
    let cmd = Cmd::new("rpm")
        .args(["-q", "--whatrequires"])
        .args(capabilities)
        .args(["--queryformat", ID_FORMAT]);
    let output = host.run(&cmd)?;
    // rpm exits non-zero when some capability has no requirer; only a killed rpm is a failure.
    if output.code.is_none() {
        return Err(SourceError::CommandFailed {
            command: cmd.to_string(),
            code: output.code,
            stderr: output.stderr.trim().to_string(),
        });
    }
    // `no package requires <cap>` lines contain spaces; ids never do.
    Ok(output
        .stdout
        .lines()
        .filter(|line| !line.is_empty() && !line.contains(' ') && !ids.contains(line))
        .map(str::to_string)
        .collect())
}

/// A file directly in a program folder (`/usr/bin/sed`, `/usr/sbin/useradd`).
fn is_program(path: &str) -> bool {
    path.rsplit_once('/')
        .is_some_and(|(dir, name)| !name.is_empty() && (dir.ends_with("/bin") || dir.ends_with("/sbin")))
}

/// `id` and every installed package that needs it, directly or through others: what dnf and zypper remove together
/// with it. Asked level by level, two rpm calls per level.
pub(crate) fn removal_closure(host: &Host, id: &str) -> Result<Vec<String>, SourceError> {
    let mut all = vec![id.to_string()];
    let mut seen: BTreeSet<String> = BTreeSet::from([id.to_string()]);
    let mut frontier = vec![id.to_string()];
    while !frontier.is_empty() {
        let ids: Vec<&str> = frontier.iter().map(String::as_str).collect();
        let next: Vec<String> = direct_dependants(host, &ids)?
            .into_iter()
            .filter(|dependant| seen.insert(dependant.clone()))
            .collect();
        all.extend(next.iter().cloned());
        frontier = next;
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    fn whatrequires(capabilities: &str) -> String {
        format!("rpm -q --whatrequires {capabilities} --queryformat {ID_FORMAT}")
    }

    fn offers(ids: &str) -> String {
        format!("rpm -q --queryformat {OFFERS_FORMAT} {ids}")
    }

    #[test]
    fn the_closure_follows_dependants_of_dependants_and_their_programs_once_each() {
        let runner = FakeRunner::new()
            .respond(
                &offers("libfoo-1-1.x86_64"),
                "libfoo\nlibfoo.so.1()(64bit)\n/usr/lib64/libfoo.so.1\n",
            )
            .respond(
                &whatrequires("libfoo libfoo.so.1()(64bit)"),
                "foo-tools-1-1.x86_64\nfoo-gui-1-1.x86_64\nfoo-tools-1-1.x86_64\n",
            )
            .respond(
                &offers("foo-gui-1-1.x86_64 foo-tools-1-1.x86_64"),
                "foo-gui\nfoo-tools\n/usr/bin/foo\n/usr/share/doc/foo-tools/README\n",
            )
            .respond(
                &whatrequires("/usr/bin/foo foo-gui foo-tools"),
                "foo-plugin-1-1.noarch\nfoo-gui-1-1.x86_64\n",
            )
            .respond(&offers("foo-plugin-1-1.noarch"), "foo-plugin\n")
            .respond(&whatrequires("foo-plugin"), "no package requires foo-plugin\n");
        let host = Fixture::new().host(runner);
        assert_eq!(
            removal_closure(&host, "libfoo-1-1.x86_64").unwrap(),
            [
                "libfoo-1-1.x86_64",
                "foo-gui-1-1.x86_64",
                "foo-tools-1-1.x86_64",
                "foo-plugin-1-1.noarch"
            ]
        );
    }
}
