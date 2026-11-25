//! Debian packages (Debian, Ubuntu, Mint, Pop!_OS…), read through `dpkg-query`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use housekeeper_core::{
    Availability, Cmd, Dependants, FileList, Host, InstallReason, OwnerLookup, Package, PackageKind, Remover,
    ScanContext, Scope, Source, SourceError, SourceId, SourceInfo, SourceKind,
};

use super::version_line;
use crate::kernel::protect_running_kernel;
use crate::owner::{OwnerQuery, args};
use crate::provider::{Context, Family, LinuxSource, Native, PackageTool, register};

pub const ID: SourceId = SourceId::new("dpkg");

/// One line per package, tab-separated. `\t` and `\n` are expanded by dpkg-query itself.
const FORMAT: &str = "--showformat=${binary:Package}\\t${Package}\\t${Architecture}\\t${Version}\\t${db:Status-Abbrev}\\t\
${Installed-Size}\\t${Section}\\t${Priority}\\t${Essential}\\t${Protected}\\t${binary:Summary}\\n";

/// Per package: id, name, status, and the relations that make it need other packages, tab-separated.
const RELATIONS_FORMAT: &str =
    "--showformat=${binary:Package}\\t${Package}\\t${db:Status-Abbrev}\\t${Provides}\\t${Depends}\\t${Pre-Depends}\\n";

/// Packages apt marked as installed automatically (as dependencies).
const APT_EXTENDED_STATES: &str = "/var/lib/apt/extended_states";

/// Reads the dpkg database; removing goes through the system's package tool (apt, decision D24).
#[derive(Default)]
pub struct DpkgSource {
    tool: Option<Arc<dyn PackageTool>>,
}

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

    fn create(context: &Context<'_>) -> Self {
        Self {
            tool: context.tool.cloned(),
        }
    }
}
register!(DpkgSource);

impl Source for DpkgSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("dpkg", "Debian packages", SourceKind::System)
            .evidence("dpkg's database, through `dpkg-query --show`")
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

    fn inventory(&self, host: &Host, scan: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let listing = host.run_ok(&Cmd::new("dpkg-query").args(["--show", FORMAT]))?;
        let auto = host
            .read_optional(APT_EXTENDED_STATES)?
            .map(|text| auto_installed(&text))
            .unwrap_or_default();
        let mut packages = parse_listing(&listing, &auto)?;
        // dpkg names the owner by its id (`binary:Package`); a kernel's version is part of its package name.
        protect_running_kernel(host, scan, &mut packages, |images| {
            Ok(self.owners(host, images)?.into_values().flatten().collect())
        });
        Ok(packages)
    }

    fn owner_lookup(&self) -> Option<&dyn OwnerLookup> {
        Some(self)
    }

    fn file_list(&self) -> Option<&dyn FileList> {
        Some(self)
    }

    fn dependant_lookup(&self) -> Option<&dyn Dependants> {
        Some(self)
    }

    fn remover(&self) -> Option<&dyn Remover> {
        self.tool.as_deref().and_then(PackageTool::remover)
    }
}

impl FileList for DpkgSource {
    fn files(&self, host: &Host, package: &Package) -> Result<Vec<PathBuf>, SourceError> {
        let stdout = host.run_ok(&Cmd::new("dpkg-query").args(["--listfiles", package.id.as_str()]))?;
        Ok(listed_files(&stdout))
    }
}

impl Dependants for DpkgSource {
    fn dependants(&self, host: &Host, package: &Package) -> Result<Vec<String>, SourceError> {
        let listing = host.run_ok(&Cmd::new("dpkg-query").args(["--show", RELATIONS_FORMAT]))?;
        dependants_of(&listing, package)
    }
}

/// Every absolute path dpkg lists, without the `/.` root entry and the diversion notes (`diverted by …`,
/// `package diverts others to: …`, `locally diverted to: …`), which don't start with `/`.
fn listed_files(stdout: &str) -> Vec<PathBuf> {
    stdout
        .lines()
        .filter(|line| line.starts_with('/') && *line != "/.")
        .map(PathBuf::from)
        .collect()
}

/// Second letter of the abbreviated status: n = not installed, c = only config files left.
fn is_installed(status: &str) -> bool {
    !matches!(status.chars().nth(1), Some('n' | 'c') | None)
}

/// The package name a relation atom names: `libc6:any (>= 2.34) [amd64] <!nocheck>` → `libc6`.
fn atom_name(atom: &str) -> &str {
    let name = atom
        .trim()
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | '[' | '<'))
        .next()
        .unwrap_or_default();
    name.split(':').next().unwrap_or_default()
}

/// Every package name in a comma-separated relation field, alternatives (`a | b`) included.
fn relation_names(field: &str) -> impl Iterator<Item = &str> {
    field.split([',', '|']).map(atom_name).filter(|name| !name.is_empty())
}

fn dependants_of(listing: &str, target: &Package) -> Result<Vec<String>, SourceError> {
    let mut rows = Vec::new();
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [id, _name, status, provides, depends, pre_depends] = fields[..] else {
            return Err(SourceError::parse(
                "dpkg-query output",
                format!("expected 6 fields: {line:?}"),
            ));
        };
        if is_installed(status) {
            rows.push((id, provides, depends, pre_depends));
        }
    }
    let mut names: HashSet<&str> = HashSet::from([target.name.as_str()]);
    if let Some((_, provides, _, _)) = rows.iter().find(|(id, ..)| *id == target.id) {
        names.extend(relation_names(provides));
    }
    let mut found: Vec<String> = rows
        .iter()
        .filter(|(id, ..)| *id != target.id)
        .filter(|(_, _, depends, pre_depends)| {
            relation_names(depends)
                .chain(relation_names(pre_depends))
                .any(|name| names.contains(name))
        })
        .map(|(id, ..)| id.to_string())
        .collect();
    found.sort();
    found.dedup();
    Ok(found)
}

impl OwnerLookup for DpkgSource {
    fn owners(&self, host: &Host, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<String>>, SourceError> {
        OwnerQuery {
            command: |paths| {
                Cmd::new("dpkg-query")
                    .arg("--search")
                    .args(args(paths).map(|p| escape_glob(&p)))
            },
            // dpkg-query exits 1 when some paths are unowned and 2 on real errors.
            failed: |output| !matches!(output.code, Some(0 | 1)),
            parse: owners,
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

/// Lines are `pkg[, pkg2]: /path` (a package may carry `:arch`), plus `diversion by pkg from: /path` and
/// `diversion by pkg to: /path` for diverted files, which say nothing about ownership.
fn owners(stdout: &str) -> Vec<(&str, &str)> {
    stdout
        .lines()
        .filter(|line| !line.starts_with("diversion by "))
        .filter_map(|line| line.split_once(": "))
        .flat_map(|(packages, path)| packages.split(", ").map(move |package| (path, package)))
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
            protected,
            summary,
        ] = fields[..]
        else {
            return Err(SourceError::parse(
                "dpkg-query output",
                format!("expected 11 fields: {line:?}"),
            ));
        };
        if !is_installed(status) {
            continue;
        }
        let mut package = Package::new(ID, id, name, Scope::System);
        package.version = Some(version.to_string()).filter(|v| !v.is_empty());
        package.summary = Some(summary.to_string()).filter(|s| !s.is_empty());
        package.size_bytes = size_kib.parse::<u64>().ok().map(|kib| kib * 1024);
        package.protected = protection(essential, protected, priority);
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

/// Why dpkg protects a package: apt refuses to remove Essential and Protected packages without being told the exact
/// phrase, and Priority required marks what a minimal system needs.
fn protection(essential: &str, protected: &str, priority: &str) -> Option<String> {
    let why = if essential == "yes" {
        "dpkg marks it Essential"
    } else if protected == "yes" {
        "dpkg marks it Protected"
    } else if priority == "required" {
        "its priority is required: a minimal system needs it"
    } else {
        return None;
    };
    Some(why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const LISTING: &str = "\
firefox\tfirefox\tamd64\t1:1snap1-0ubuntu5\tii \t260\tweb\toptional\tno\tno\tTransitional package - firefox -> firefox snap
libc6:amd64\tlibc6\tamd64\t2.39-0ubuntu8.3\tii \t13456\tlibs\toptional\tno\tno\tGNU C Library: Shared libraries
libc6:i386\tlibc6\ti386\t2.39-0ubuntu8.3\tii \t12960\tlibs\toptional\tno\tno\tGNU C Library: Shared libraries
bash\tbash\tamd64\t5.2.21-2ubuntu4\tii \t1868\tshells\trequired\tyes\tno\tGNU Bourne Again SHell
gimp\tgimp\tamd64\t2.10.36-3\trc \t\tgraphics\toptional\tno\tno\tGNU Image Manipulation Program
libfoo-dev\tlibfoo-dev\tamd64\t1.0\tii \t40\tuniverse/libdevel\toptional\tno\tno\tfoo headers
init\tinit\tamd64\t1.66ubuntu1\tii \t29\tmetapackages\timportant\tno\tyes\tSystem-V-like init utilities - metapackage
mawk\tmawk\tamd64\t1.3.4.20240123-1build1\tii \t207\tinterpreters\trequired\tno\t\tPattern scanning and text processing language
linux-image-6.8.0-45-generic\tlinux-image-6.8.0-45-generic\tamd64\t6.8.0-45.45\tii \t15000\tkernel\toptional\tno\tno\tSigned kernel image generic
linux-image-6.8.0-40-generic\tlinux-image-6.8.0-40-generic\tamd64\t6.8.0-40.40\tii \t15000\tkernel\toptional\tno\tno\tSigned kernel image generic
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
        DpkgSource::default()
            .inventory(&fixture.host(runner), &ScanContext::default())
            .unwrap()
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn lists_installed_packages_and_skips_removed_ones() {
        let packages = scan();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "firefox",
                "libc6:amd64",
                "libc6:i386",
                "bash",
                "libfoo-dev",
                "init",
                "mawk",
                "linux-image-6.8.0-45-generic",
                "linux-image-6.8.0-40-generic"
            ]
        );
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
    fn essential_protected_and_required_packages_are_protected_with_the_reason() {
        let packages = scan();
        let why = |id: &str| find(&packages, id).protected.clone();
        assert_eq!(why("bash").as_deref(), Some("dpkg marks it Essential"));
        assert_eq!(why("init").as_deref(), Some("dpkg marks it Protected"));
        assert!(
            why("mawk").is_some_and(|why| why.contains("required")),
            "older dpkg prints no Protected field"
        );
        assert_eq!(why("firefox"), None);
    }

    #[test]
    fn the_running_kernel_is_protected_and_older_kernels_are_not() {
        let fixture = Fixture::new();
        fixture
            .file("/proc/sys/kernel/osrelease", "6.8.0-45-generic\n")
            .file("/boot/vmlinuz-6.8.0-45-generic", "");
        let runner = FakeRunner::new()
            .respond(&format!("dpkg-query --show {FORMAT}"), LISTING)
            .respond(
                "dpkg-query --search /boot/vmlinuz-6.8.0-45-generic",
                "linux-image-6.8.0-45-generic: /boot/vmlinuz-6.8.0-45-generic\n",
            );
        let packages = DpkgSource::default()
            .inventory(&fixture.host(runner), &ScanContext::default())
            .unwrap();
        assert_eq!(
            find(&packages, "linux-image-6.8.0-45-generic").protected.as_deref(),
            Some("it is the running kernel")
        );
        assert_eq!(find(&packages, "linux-image-6.8.0-40-generic").protected, None);
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
        let packages = DpkgSource::default()
            .inventory(&fixture.host(runner), &ScanContext::default())
            .unwrap();
        assert_eq!(find(&packages, "libc6:i386").reason, InstallReason::Explicit);
    }

    #[test]
    fn malformed_output_is_an_error() {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(&format!("dpkg-query --show {FORMAT}"), "garbage\n");
        assert!(matches!(
            DpkgSource::default().inventory(&fixture.host(runner), &ScanContext::default()),
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
            DpkgSource::default().probe(&fixture.host(runner)),
            Availability::Available {
                version: Some("1.22.6".into())
            }
        );
    }

    mod owner {
        use super::*;
        use crate::owner::tests::{exit, owned, paths, sorted};

        #[test]
        fn owners_of_the_paths_asked_about() {
            let runner = FakeRunner::new().respond_with(
                "dpkg-query --search /opt /opt/brave.com /opt/linear /usr/local/bin/ngrok /usr/local/bin/words",
                exit(
                    1,
                    "diversion by dictionaries-common from: /usr/local/bin/words\n\
                     diversion by dictionaries-common to: /usr/local/bin/words.pre\n\
                     t3code, freelens, zoom: /opt\n\
                     t3code: /opt/T3 Code (Beta)\n\
                     brave-browser: /opt/brave.com\n\
                     ngrok:amd64: /usr/local/bin/ngrok\n",
                    "dpkg-query: no path found matching pattern /opt/linear\n",
                ),
            );
            let host = Fixture::new().host(runner);
            let asked = paths(&[
                "/opt",
                "/opt/brave.com",
                "/opt/linear",
                "/usr/local/bin/ngrok",
                "/usr/local/bin/words",
            ]);
            assert_eq!(
                sorted(DpkgSource::default().owners(&host, &asked).unwrap()),
                [
                    owned("/opt", &["t3code", "freelens", "zoom"]),
                    owned("/opt/brave.com", &["brave-browser"]),
                    owned("/usr/local/bin/ngrok", &["ngrok:amd64"]),
                ]
            );
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
                DpkgSource::default().owners(&host, &paths(&["/opt/x"])),
                Err(SourceError::CommandFailed { code: Some(2), .. })
            ));
        }
    }

    mod show {
        use super::*;

        const RELATIONS: &str = "\
libc6:amd64\tlibc6\tii \t\tlibgcc-s1\t
libc6:i386\tlibc6\tii \t\tlibgcc-s1\t
dpkg\tdpkg\tii \t\ttar (>= 1.28-1)\tlibbz2-1.0, libc6 (>= 2.34), liblzma5 (>= 5.4.0), zlib1g (>= 1:1.1.4)
postfix\tpostfix\tii \tdefault-mta (= 3.8.6-1build2), mail-transport-agent\tlibc6 (>= 2.38), postfix-common\t
bsd-mailx\tbsd-mailx\tii \t\tlibc6 (>= 2.34), liblockfile1 (>= 1.01), default-mta | mail-transport-agent\t
logwatch\tlogwatch\tii \t\tperl:any, postfix:any (>= 3.0) [linux-any] <!nocheck>\t
mailutils\tmailutils\tii \t\tlibc6 (>= 2.34), exim4-daemon-light | postfix\t
mutt\tmutt\tii \t\tlibc6 (>= 2.38), libgpgme11t64 (>= 1.4.1)\t
old-mailer\told-mailer\trc \t\tlibc6 (>= 2.17), mail-transport-agent\t
";

        fn dependants(id: &str, name: &str) -> Vec<String> {
            let runner = FakeRunner::new().respond(&format!("dpkg-query --show {RELATIONS_FORMAT}"), RELATIONS);
            let host = Fixture::new().host(runner);
            Dependants::dependants(
                &DpkgSource::default(),
                &host,
                &Package::new(ID, id, name, Scope::System),
            )
            .unwrap()
        }

        #[test]
        fn file_list_drops_the_root_and_diversion_notes() {
            let runner = FakeRunner::new().respond(
                "dpkg-query --listfiles libc6:amd64",
                "/.\n/etc\n/etc/ld.so.conf.d\n/usr/lib/x86_64-linux-gnu/libc.so.6\n\
                 diverted by libc6-hack to: /usr/lib/x86_64-linux-gnu/libc.so.6.real\n\
                 package diverts others to: /usr/bin/ldd.distrib\n\
                 locally diverted to: /etc/ld.so.conf.d/local.conf\n\
                 /usr/bin/ldd\n",
            );
            let host = Fixture::new().host(runner);
            let package = Package::new(ID, "libc6:amd64", "libc6", Scope::System);
            assert_eq!(
                DpkgSource::default().files(&host, &package).unwrap(),
                [
                    "/etc",
                    "/etc/ld.so.conf.d",
                    "/usr/lib/x86_64-linux-gnu/libc.so.6",
                    "/usr/bin/ldd"
                ]
                .map(PathBuf::from)
            );
        }

        #[test]
        fn dependants_through_the_name_with_versions_and_pre_depends() {
            // libc6:i386 shares the name but does not depend on itself; old-mailer is only config files.
            assert_eq!(
                dependants("libc6:amd64", "libc6"),
                ["bsd-mailx", "dpkg", "mailutils", "mutt", "postfix"]
            );
        }

        #[test]
        fn dependants_through_provides_alternatives_and_arch_qualifiers() {
            // bsd-mailx needs `default-mta | mail-transport-agent`, both provided by postfix; mailutils names postfix
            // as its second alternative; logwatch needs `postfix:any`. old-mailer is removed, and packages that merely
            // recommend postfix (ubuntu-server, say) carry no Depends on it, so they never show up.
            assert_eq!(dependants("postfix", "postfix"), ["bsd-mailx", "logwatch", "mailutils"]);
        }

        #[test]
        fn a_package_nothing_needs_has_no_dependants() {
            assert!(dependants("mutt", "mutt").is_empty());
        }
    }
}
