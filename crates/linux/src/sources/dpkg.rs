//! Debian packages (Debian, Ubuntu, Mint, Pop!_OS…), read through `dpkg-query`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, Cmd, Host, InstallReason, OwnerLookup, Package, PackageKind, Scope, Source, SourceError, SourceId,
    SourceInfo, SourceKind, ScanContext,
};

use super::version_line;
use crate::owner::{OwnerQuery, args};
use crate::provider::{Context, Family, LinuxSource, Native};

pub const ID: SourceId = SourceId::new("dpkg");

/// One line per package, tab-separated. `\t` and `\n` are expanded by dpkg-query itself.
const FORMAT: &str = "--showformat=${binary:Package}\\t${Package}\\t${Architecture}\\t${Version}\\t${db:Status-Abbrev}\\t\
${Installed-Size}\\t${Section}\\t${Priority}\\t${Essential}\\t${binary:Summary}\\n";

/// Packages apt marked as installed automatically (as dependencies).
const APT_EXTENDED_STATES: &str = "/var/lib/apt/extended_states";

pub struct DpkgSource;

impl LinuxSource for DpkgSource {
    const NATIVE: Option<Native> = Some(Native {
        families: &[Family {
            name: "debian",
            distro_ids: &[
                "debian",
                "ubuntu",
                "linuxmint",
                "pop",
                "elementary",
                "zorin",
                "kali",
                "raspbian",
                "neon",
                "devuan",
                "mx",
                "deepin",
                "pureos",
            ],
        }],
        program: "dpkg-query",
    });

    fn create(_: &Context<'_>) -> Self {
        Self
    }
}

impl Source for DpkgSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("dpkg", "Debian packages", SourceKind::System)
    }

    fn probe(&self, host: &Host) -> Availability {
        if !host.has_program("dpkg-query") {
            return Availability::unavailable("dpkg-query is not installed");
        }
        let version = host
            .run_ok(&Cmd::new("dpkg-query").arg("--version"))
            .ok()
            .and_then(|out| version_line(&out))
            .and_then(|line| line.split(" version ").nth(1).map(str::to_string))
            .and_then(|rest| rest.split_whitespace().next().map(str::to_string));
        Availability::Available { version }
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> { let listing = host.run_ok(&Cmd::new("dpkg-query").args(["--show", FORMAT]))?;
    let auto = host
        .read_optional(APT_EXTENDED_STATES)?
        .map(|text| auto_installed(&text))
        .unwrap_or_default();
    parse_listing(&listing, &auto) }

    fn owner_lookup(&self) -> Option<&dyn OwnerLookup> {
        Some(self)
    }
}

impl OwnerLookup for DpkgSource {
    fn owned_paths(&self, host: &Host, paths: &[PathBuf]) -> Result<HashSet<PathBuf>, SourceError> {
        OwnerQuery {
            command: |paths| {
                Cmd::new("dpkg-query")
                    .arg("--search")
                    .args(args(paths).map(|p| escape_glob(&p)))
            },
            // dpkg-query exits 1 when some paths are unowned and 2 on real errors.
            failed: |output| !matches!(output.code, Some(0 | 1)),
            parse: owned,
        }
        .run(host, paths)
    }
}

/// dpkg-query treats its arguments as glob patterns; escape the wildcard characters so paths match literally.
fn escape_glob(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '*' | '?' | '[' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Lines are `pkg[, pkg2]: /path`, plus `diversion by pkg from: /path` and `diversion by pkg to: /path` for diverted
/// files, which say nothing about ownership.
fn owned(stdout: &str, wanted: &HashSet<&Path>) -> HashSet<PathBuf> {
    stdout
        .lines()
        .filter(|line| !line.starts_with("diversion by "))
        .filter_map(|line| line.split_once(": ").map(|(_, path)| Path::new(path)))
        .filter(|path| wanted.contains(path))
        .map(Path::to_path_buf)
        .collect()
}

/// `(package, architecture)` pairs apt installed as dependencies.
fn auto_installed(extended_states: &str) -> HashSet<(String, String)> {
    let mut auto = HashSet::new();
    for stanza in extended_states.split("\n\n") {
        let (mut name, mut arch, mut is_auto) = (None, None, false);
        for line in stanza.lines() {
            match line.split_once(':') {
                Some(("Package", value)) => name = Some(value.trim().to_string()),
                Some(("Architecture", value)) => arch = Some(value.trim().to_string()),
                Some(("Auto-Installed", value)) => is_auto = value.trim() == "1",
                _ => {}
            }
        }
        if let (Some(name), Some(arch), true) = (name, arch, is_auto) {
            auto.insert((name, arch));
        }
    }
    auto
}

fn parse_listing(listing: &str, auto: &HashSet<(String, String)>) -> Result<Vec<Package>, SourceError> {
    let mut packages = Vec::new();
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [
            id,
            name,
            arch,
            version,
            status,
            size_kib,
            section,
            priority,
            essential,
            summary,
        ] = fields[..]
        else {
            return Err(SourceError::parse(
                "dpkg-query output",
                format!("expected 10 fields: {line:?}"),
            ));
        };
        // Second letter of the abbreviated status: n = not installed, c = only config files left.
        if matches!(status.chars().nth(1), Some('n' | 'c') | None) {
            continue;
        }
        let mut package = Package::new(ID, id, name, Scope::System);
        package.version = Some(version.to_string()).filter(|v| !v.is_empty());
        package.summary = Some(summary.to_string()).filter(|s| !s.is_empty());
        package.size_bytes = size_kib.parse::<u64>().ok().map(|kib| kib * 1024);
        package.protected = essential == "yes" || priority == "required";
        package.reason = if matches!(priority, "required" | "important") {
            InstallReason::System
        } else if auto.contains(&(name.to_string(), arch.to_string())) {
            InstallReason::Dependency
        } else {
            InstallReason::Explicit
        };
        let section = section.rsplit('/').next().unwrap_or(section);
        if matches!(section, "libs" | "oldlibs" | "libdevel") {
            package.kind = PackageKind::Library;
        }
        packages.push(package);
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const LISTING: &str = "\
firefox\tfirefox\tamd64\t1:1snap1-0ubuntu5\tii \t260\tweb\toptional\tno\tTransitional package - firefox -> firefox snap
libc6:amd64\tlibc6\tamd64\t2.39-0ubuntu8.3\tii \t13456\tlibs\toptional\tno\tGNU C Library: Shared libraries
libc6:i386\tlibc6\ti386\t2.39-0ubuntu8.3\tii \t12960\tlibs\toptional\tno\tGNU C Library: Shared libraries
bash\tbash\tamd64\t5.2.21-2ubuntu4\tii \t1868\tshells\trequired\tyes\tGNU Bourne Again SHell
gimp\tgimp\tamd64\t2.10.36-3\trc \t\tgraphics\toptional\tno\tGNU Image Manipulation Program
libfoo-dev\tlibfoo-dev\tamd64\t1.0\tii \t40\tuniverse/libdevel\toptional\tno\tfoo headers
";

    const EXTENDED_STATES: &str = "\
Package: libc6
Architecture: i386
Auto-Installed: 1

Package: libfoo-dev
Architecture: amd64
Auto-Installed: 0
";

    fn scan() -> Vec<Package> {
        let fixture = Fixture::new();
        fixture.file(APT_EXTENDED_STATES, EXTENDED_STATES);
        let runner = FakeRunner::new().respond(&format!("dpkg-query --show {FORMAT}"), LISTING);
        DpkgSource.inventory(&fixture.host(runner), &ScanContext::default()).unwrap()
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn lists_installed_packages_and_skips_removed_ones() {
        let packages = scan();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["firefox", "libc6:amd64", "libc6:i386", "bash", "libfoo-dev"]);
        let firefox = find(&packages, "firefox");
        assert_eq!(firefox.version.as_deref(), Some("1:1snap1-0ubuntu5"));
        assert_eq!(firefox.size_bytes, Some(260 * 1024));
        assert_eq!(firefox.scope, Scope::System);
    }

    #[test]
    fn install_reason_follows_apt_marks_per_architecture() {
        let packages = scan();
        assert_eq!(find(&packages, "firefox").reason, InstallReason::Explicit);
        assert_eq!(find(&packages, "libc6:amd64").reason, InstallReason::Explicit);
        assert_eq!(find(&packages, "libc6:i386").reason, InstallReason::Dependency);
        assert_eq!(find(&packages, "libfoo-dev").reason, InstallReason::Explicit);
        assert_eq!(find(&packages, "bash").reason, InstallReason::System);
    }

    #[test]
    fn essential_and_required_packages_are_protected() {
        let packages = scan();
        assert!(find(&packages, "bash").protected);
        assert!(!find(&packages, "firefox").protected);
    }

    #[test]
    fn library_sections_are_libraries() {
        let packages = scan();
        assert_eq!(find(&packages, "libfoo-dev").kind, PackageKind::Library);
        assert_eq!(find(&packages, "libc6:amd64").kind, PackageKind::Library);
        assert_eq!(find(&packages, "firefox").kind, PackageKind::Unknown);
    }

    #[test]
    fn works_without_apt() {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(&format!("dpkg-query --show {FORMAT}"), LISTING);
        let packages = DpkgSource.inventory(&fixture.host(runner), &ScanContext::default()).unwrap();
        assert_eq!(find(&packages, "libc6:i386").reason, InstallReason::Explicit);
    }

    #[test]
    fn malformed_output_is_an_error() {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(&format!("dpkg-query --show {FORMAT}"), "garbage\n");
        assert!(matches!(
            DpkgSource.inventory(&fixture.host(runner), &ScanContext::default()),
            Err(SourceError::Parse { .. })
        ));
    }

    #[test]
    fn probe_reads_the_version() {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(
            "dpkg-query --version",
            "Debian dpkg-query package management program version 1.22.6 (amd64).\n",
        );
        assert_eq!(
            DpkgSource.probe(&fixture.host(runner)),
            Availability::Available {
                version: Some("1.22.6".into())
            }
        );
    }

    mod owner {
        use super::*;
        use crate::owner::tests::{exit, paths, sorted};

        #[test]
        fn partial_matches_are_not_an_error() {
            let runner = FakeRunner::new().respond_with(
                "dpkg-query --search /opt/brave.com /opt/linear /usr/local/bin/ngrok",
                exit(
                    1,
                    "brave-browser: /opt/brave.com\nngrok: /usr/local/bin/ngrok\n",
                    "dpkg-query: no path found matching pattern /opt/linear\n",
                ),
            );
            let host = Fixture::new().host(runner);
            let owned = DpkgSource
                .owned_paths(&host, &paths(&["/opt/brave.com", "/opt/linear", "/usr/local/bin/ngrok"]))
                .unwrap();
            assert_eq!(sorted(owned), paths(&["/opt/brave.com", "/usr/local/bin/ngrok"]));
        }

        #[test]
        fn diversions_and_paths_not_asked_about_are_ignored() {
            let stdout = "\
diversion by dictionaries-common from: /usr/local/bin/words
diversion by dictionaries-common to: /usr/local/bin/words.pre
t3code, freelens, zoom: /opt
t3code: /opt/T3 Code (Beta)
";
            let wanted_paths = paths(&["/usr/local/bin/words", "/opt/T3 Code (Beta)"]);
            let wanted: HashSet<&Path> = wanted_paths.iter().map(PathBuf::as_path).collect();
            assert_eq!(sorted(owned(stdout, &wanted)), paths(&["/opt/T3 Code (Beta)"]));
        }

        #[test]
        fn patterns_are_escaped() {
            assert_eq!(escape_glob("/opt/a*b?[c]\\d"), "/opt/a\\*b\\?\\[c]\\\\d");
        }

        #[test]
        fn fatal_errors_fail() {
            let runner = FakeRunner::new().respond_with(
                "dpkg-query --search /opt/x",
                exit(2, "", "dpkg-query: error: parsing file '/var/lib/dpkg/status'"),
            );
            let host = Fixture::new().host(runner);
            assert!(matches!(
                DpkgSource.owned_paths(&host, &paths(&["/opt/x"])),
                Err(SourceError::CommandFailed { code: Some(2), .. })
            ));
        }
    }
}
