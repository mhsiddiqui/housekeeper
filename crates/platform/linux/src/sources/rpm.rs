//! RPM packages (Fedora, RHEL and rebuilds, openSUSE, Silverblue), read through `rpm --queryformat`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use housekeeper_core::{
    Availability, Cmd, Dependants, FileInstaller, FileList, FilePackage, Host, InstallReason, OwnerLookup, Package,
    PackageKind, Remover, ScanContext, Scope, Source, SourceError, SourceId, SourceInfo, SourceKind,
};
use serde::Deserialize;

use super::version_line;
use crate::formats::{path_arg, tab_package};
use crate::kernel::protect_running_kernel;
use crate::owner::{OwnerQuery, args};
use crate::provider::{Context, Family, FileFormat, LinuxSource, Native, PackageTool, register};
use crate::rpmdb::{ID_FORMAT, direct_dependants};

pub const ID: SourceId = SourceId::new("rpm");

/// One line per package, tab-separated; rpm expands `\t` and `\n` itself. `LONGSIZE` falls back to `SIZE` and, unlike
/// it, does not overflow for packages over 4 GiB.
const FORMAT: &str =
    "%{NAME}\\t%{EPOCH}\\t%{VERSION}\\t%{RELEASE}\\t%{ARCH}\\t%{LONGSIZE}\\t%{INSTALLTIME}\\t%{SUMMARY}\\n";

/// Packages zypper installed as dependencies (openSUSE).
const ZYPP_AUTO_INSTALLED: &str = "/var/lib/zypp/AutoInstalled";

/// Directories of dnf's protected package lists. dnf4 reads both; dnf5 (`protected_packages` default
/// `dnf5,glob:/etc/dnf/protected.d/*.conf`) only the first.
const PROTECTED_DIRS: [&str; 2] = ["/etc/dnf/protected.d", "/etc/yum/protected.d"];

/// dnf's configuration, whose `protected_packages` can name packages too.
const DNF_CONF: &str = "/etc/dnf/dnf.conf";

/// Packages whose removal leaves the system unbootable or unmanageable, whatever the distribution configured. Kernels
/// are not listed: only the running one is protected, as dnf does, so older kernels can go.
const CORE_PACKAGES: &[&str] = &[
    // C library every program links against.
    "glibc",
    // Init system and service manager.
    "systemd",
    // Directory layout and the base /etc files (passwd, group, shells).
    "filesystem",
    "setup",
    // /bin/sh and the basic commands scripts rely on.
    "bash",
    "coreutils",
    // Package managers; without them nothing can be repaired.
    "rpm",
    "dnf",
    "dnf5",
    "zypper",
    "libzypp",
    "rpm-ostree",
];

/// Reads RPM packages. On image-based systems (Silverblue and friends) the install reason comes from `rpm-ostree`.
/// Removing goes through the system's package tool (dnf, zypper or rpm-ostree, decision D24).
pub struct RpmSource {
    image_based: bool,
    tool: Option<Arc<dyn PackageTool>>,
}

impl LinuxSource for RpmSource {
    const NATIVE: Option<Native> = Some(Native {
        families: &[
            Family {
                name: "fedora",
                distro_ids: &[
                    "fedora",
                    "rhel",
                    "centos",
                    "rocky",
                    "almalinux",
                    "ol",
                    "nobara",
                    "ultramarine",
                    "amzn",
                    "bazzite",
                ],
            },
            Family {
                name: "suse",
                distro_ids: &[
                    "opensuse",
                    "opensuse-leap",
                    "opensuse-tumbleweed",
                    "opensuse-microos",
                    "sles",
                    "sled",
                    "suse",
                ],
            },
        ],
        program: "rpm",
    });

    const FILES: &'static [FileFormat] = &[FileFormat {
        id: "rpm",
        title: "RPM package",
        suffixes: &[".rpm"],
        matches: is_rpm,
        evidence: "RPM's lead (`ED AB EE DB`), marked as a binary package",
        read: read_rpm,
    }];

    fn create(context: &Context<'_>) -> Self {
        Self {
            tool: context.tool.cloned(),
            ..Self::new(context.distro.image_based)
        }
    }
}
register!(RpmSource);

/// An `.rpm` starts with RPM's lead: its magic, the format version, then the type, 0 for a binary package (1 is a
/// source package, which installs nothing).
fn is_rpm(head: &[u8]) -> bool {
    head.starts_with(b"\xed\xab\xee\xdb") && head.get(6..8) == Some(&[0, 0][..])
}

/// Name, `[epoch:]version-release` and processor from an `.rpm`'s header, tab-separated.
const RPM_FILE_FORMAT: &str = "%{NAME}\\t%|EPOCH?{%{EPOCH}:}|%{VERSION}-%{RELEASE}\\t%{ARCH}\\n";

/// The lead's name field: `name-version-release`, NUL-padded.
const LEAD_NAME: std::ops::Range<usize> = 10..76;

/// What an `.rpm` holds, from its header through `rpm` where there is one (it is on many Debian systems too), else
/// the name and version its lead spells out.
fn read_rpm(host: &Host, path: &Path, head: &[u8]) -> Result<Option<FilePackage>, SourceError> {
    if !host.has_program("rpm") {
        return Ok(lead_package(head));
    }
    let output = host.run_ok(
        &Cmd::new("rpm")
            .args(["-qp", "--nosignature", "--nodigest", "--queryformat", RPM_FILE_FORMAT])
            .arg(path_arg(host, path)),
    )?;
    Ok(Some(tab_package(&output)))
}

/// `name-version-release` from the lead; the name may hold `-`, the version and release cannot.
fn lead_package(head: &[u8]) -> Option<FilePackage> {
    let field = head.get(LEAD_NAME)?;
    let text = std::str::from_utf8(&field[..field.iter().position(|&b| b == 0).unwrap_or(field.len())]).ok()?;
    let mut parts = text.rsplitn(3, '-');
    let (release, version, name) = (parts.next()?, parts.next()?, parts.next()?);
    Some(FilePackage {
        name: Some(name.to_string()),
        version: Some(format!("{version}-{release}")),
        ..FilePackage::default()
    })
}

impl RpmSource {
    pub fn new(image_based: bool) -> Self {
        Self {
            image_based,
            tool: None,
        }
    }

    fn reasons(&self, host: &Host) -> Result<Reasons, SourceError> {
        if self.image_based {
            // rpm-ostree talks to its daemon; if that fails the listing is still worth showing.
            return Ok(host
                .run_ok(&Cmd::new("rpm-ostree").args(["status", "--json"]))
                .ok()
                .and_then(|json| layered_packages(&json).ok())
                .map_or(Reasons::Unknown, Reasons::Layered));
        }
        Ok(match host.read_optional(ZYPP_AUTO_INSTALLED)? {
            Some(text) => Reasons::Zypp(names(&text)),
            None => Reasons::Unknown,
        })
    }
}

impl Source for RpmSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("rpm", "RPM packages", SourceKind::System)
            .evidence("the RPM database, through `rpm -qa` (and `rpm-ostree status` on image-based systems)")
    }

    fn probe(&self, host: &Host) -> Availability {
        if !host.has_program("rpm") {
            return Availability::unavailable("rpm is not installed");
        }
        let version = host
            .run_ok(&Cmd::new("rpm").arg("--version"))
            .ok()
            .and_then(|out| version_line(&out))
            .and_then(|line| line.split(" version ").nth(1).map(str::to_string))
            .and_then(|rest| rest.split_whitespace().next().map(str::to_string));
        Availability::Available { version }
    }

    fn inventory(&self, host: &Host, scan: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let listing = host.run_ok(&Cmd::new("rpm").args(["-qa", "--queryformat", FORMAT]))?;
        let reasons = self.reasons(host)?;
        let mut protected: HashMap<String, String> = HashMap::new();
        for dir in PROTECTED_DIRS {
            for entry in host.read_dir(dir)? {
                if entry.name.ends_with(".conf") {
                    let why = format!("listed in dnf's protected packages ({})", entry.path.display());
                    for name in names(&host.read_to_string(&entry.path)?) {
                        protected.entry(name).or_insert_with(|| why.clone());
                    }
                }
            }
        }
        if let Some(conf) = host.read_optional(DNF_CONF)? {
            for name in dnf_protected_packages(&conf) {
                protected
                    .entry(name)
                    .or_insert_with(|| format!("listed in dnf's protected_packages ({DNF_CONF})"));
            }
        }
        for name in CORE_PACKAGES {
            protected
                .entry(name.to_string())
                .or_insert_with(|| "the system cannot start or be repaired without it".to_string());
        }
        let mut packages = parse_listing(&listing, &reasons, &protected)?;
        protect_running_kernel(host, scan, &mut packages, |images| {
            // rpm names owners by name alone, which several installed kernels share; ask for the exact install.
            let cmd = Cmd::new("rpm")
                .args(["-qf", "--queryformat", ID_FORMAT])
                .args(args(images));
            // "file … is not owned by any package" lines contain spaces; ids never do.
            Ok(host
                .run(&cmd)?
                .stdout
                .lines()
                .filter(|line| !line.is_empty() && !line.contains(' '))
                .map(str::to_string)
                .collect())
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

    fn file_installer(&self) -> Option<&dyn FileInstaller> {
        self.tool.as_deref().and_then(PackageTool::file_installer)
    }
}

impl FileList for RpmSource {
    fn files(&self, host: &Host, package: &Package) -> Result<Vec<PathBuf>, SourceError> {
        let listing = host.run_ok(&Cmd::new("rpm").args(["-ql", package.id.as_str()]))?;
        // Empty packages print `(contains no files)`.
        Ok(listing
            .lines()
            .filter(|line| line.starts_with('/'))
            .map(PathBuf::from)
            .collect())
    }
}

impl Dependants for RpmSource {
    fn dependants(&self, host: &Host, package: &Package) -> Result<Vec<String>, SourceError> {
        Ok(direct_dependants(host, &[package.id.as_str()])?.into_iter().collect())
    }
}

impl OwnerLookup for RpmSource {
    fn owners(&self, host: &Host, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<String>>, SourceError> {
        OwnerQuery {
            // Each owning package's name beside every file it owns: unlike plain `%{NAME}`, this says which argument
            // was owned even when an argument has several owners or none (missing files print nothing on stdout).
            // rpm prints the whole file list once per argument, so many arguments would repeat big packages over and
            // over (651 programs in a minimal Fedora: 129,000 lines); listing every package's files once is smaller.
            command: |paths| {
                let cmd = Cmd::new("rpm");
                if paths.len() > WHOLE_DATABASE_FROM {
                    cmd.args(["-qa", "--queryformat", "[%{=NAME}\\t%{FILENAMES}\\n]"])
                } else {
                    cmd.args(["-qf", "--queryformat", "[%{=NAME}\\t%{FILENAMES}\\n]"])
                        .args(args(paths))
                }
            },
            // Unowned or missing paths make rpm exit non-zero; only a killed rpm is a failure.
            failed: |output| output.code.is_none(),
            parse: owners,
        }
        .run(host, paths)
    }
}

/// Above this many paths, owners are found in a listing of every installed file (`rpm -qa`), whose size does not grow
/// with the number of paths.
const WHOLE_DATABASE_FROM: usize = 64;

/// `name<TAB>file` for every file of an owning package; "file … is not owned by any package" lines have no tab.
fn owners(stdout: &str) -> Vec<(&str, &str)> {
    stdout
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(package, path)| (path, package))
        .collect()
}

/// Where install reasons come from on this system.
#[derive(Debug, PartialEq)]
enum Reasons {
    /// zypper's auto-installed names: listed are dependencies, the rest explicit.
    Zypp(HashSet<String>),
    /// Names layered on an image: listed are explicit, the rest part of the image.
    Layered(HashSet<String>),
    /// dnf systems (history database not read yet) and image-based systems where rpm-ostree failed.
    Unknown,
}

impl Reasons {
    fn of(&self, name: &str) -> InstallReason {
        match self {
            Self::Zypp(auto) if auto.contains(name) => InstallReason::Dependency,
            Self::Zypp(_) => InstallReason::Explicit,
            Self::Layered(requested) if requested.contains(name) => InstallReason::Explicit,
            Self::Layered(_) => InstallReason::System,
            Self::Unknown => InstallReason::Unknown,
        }
    }
}

/// One name per line; blank lines and `#` comments skipped. Used by zypp's AutoInstalled and dnf's protected.d.
fn names(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// The names in `protected_packages` of dnf.conf's `[main]` section (comma or space separated); `glob:` entries name
/// files, which are the protected.d directories already read.
fn dnf_protected_packages(conf: &str) -> Vec<String> {
    let mut in_main = false;
    let mut found = Vec::new();
    for line in conf.lines().map(str::trim) {
        if line.starts_with('[') {
            in_main = line == "[main]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if in_main && key.trim() == "protected_packages" {
            found = value
                .split([',', ' ', '\t'])
                .map(str::trim)
                .filter(|name| !name.is_empty() && !name.starts_with("glob:"))
                .map(str::to_string)
                .collect();
        }
    }
    found
}

/// `(evr, id)`: the version with its epoch when there is one, and the full NEVRA that tells one install from others of
/// the same name (several kernels side by side).
fn nevra(name: &str, epoch: &str, version: &str, release: &str, arch: &str) -> (String, String) {
    let evr = match epoch {
        "(none)" | "" => format!("{version}-{release}"),
        epoch => format!("{epoch}:{version}-{release}"),
    };
    let id = match arch {
        "(none)" | "" => format!("{name}-{evr}"),
        arch => format!("{name}-{evr}.{arch}"),
    };
    (evr, id)
}

#[derive(Deserialize)]
struct OstreeStatus {
    deployments: Vec<Deployment>,
}

#[derive(Deserialize)]
struct Deployment {
    #[serde(default)]
    booted: bool,
    #[serde(rename = "requested-packages", default)]
    requested: Vec<String>,
    /// NEVRAs of local RPM files layered on the image.
    #[serde(rename = "requested-local-packages", default)]
    requested_local: Vec<String>,
    #[serde(rename = "requested-local-fileoverride-packages", default)]
    requested_local_fileoverride: Vec<String>,
}

/// Names the user layered on the booted deployment, from `rpm-ostree status --json`.
fn layered_packages(json: &str) -> Result<HashSet<String>, SourceError> {
    let status: OstreeStatus =
        serde_json::from_str(json).map_err(|e| SourceError::parse("rpm-ostree status", e.to_string()))?;
    let booted = status
        .deployments
        .into_iter()
        .find(|deployment| deployment.booted)
        .ok_or_else(|| SourceError::parse("rpm-ostree status", "no booted deployment"))?;
    let local = booted
        .requested_local
        .iter()
        .chain(&booted.requested_local_fileoverride)
        .filter_map(|nevra| name_of_nevra(nevra))
        .map(str::to_string);
    Ok(booted.requested.into_iter().chain(local).collect())
}

/// `foo-bar-1:1.2-3.fc41.x86_64` → `foo-bar`. Version and release never contain `-`; the name may.
fn name_of_nevra(nevra: &str) -> Option<&str> {
    let mut parts = nevra.rsplitn(3, '-');
    let (_release_arch, _version, name) = (parts.next()?, parts.next()?, parts.next()?);
    Some(name)
}

fn parse_listing(
    listing: &str,
    reasons: &Reasons,
    protected: &HashMap<String, String>,
) -> Result<Vec<Package>, SourceError> {
    let mut packages = Vec::new();
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.splitn(8, '\t').collect();
        let [name, epoch, version, release, arch, size, install_time, summary] = fields[..] else {
            return Err(SourceError::parse("rpm output", format!("expected 8 fields: {line:?}")));
        };
        // Imported signing keys are stored as pseudo-packages.
        if name == "gpg-pubkey" {
            continue;
        }
        let (evr, id) = nevra(name, epoch, version, release, arch);
        let mut package = Package::new(ID, id, name, Scope::System);
        package.version = Some(evr);
        package.summary = Some(summary.to_string()).filter(|s| !s.is_empty() && s != "(none)");
        package.size_bytes = size.parse().ok();
        package.installed_at = install_time.parse().ok();
        package.reason = reasons.of(name);
        package.protected = protected.get(name).cloned();
        if is_library(name) {
            package.kind = PackageKind::Library;
        }
        packages.push(package);
    }
    Ok(packages)
}

/// Fedora and openSUSE naming conventions. `libre*` (LibreOffice, Libreswan, LibreCAD) are applications.
fn is_library(name: &str) -> bool {
    name.ends_with("-libs") || name.ends_with("-devel") || (name.starts_with("lib") && !name.starts_with("libre"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpmdb::OFFERS_FORMAT;
    use housekeeper_core::Output;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const LISTING: &str = "\
gpg-pubkey\t(none)\te99d6ad1\t64d2612e\t(none)\t0\t1715000000\tFedora (41) <fedora-41-primary@fedoraproject.org> public key
firefox\t(none)\t132.0.1\t1.fc41\tx86_64\t260321456\t1731400000\tMozilla Firefox Web browser
glibc\t(none)\t2.40\t9.fc41\tx86_64\t6824312\t1715000100\tThe GNU libc libraries
glibc\t(none)\t2.40\t9.fc41\ti686\t6213100\t1715000200\tThe GNU libc libraries
NetworkManager\t1\t1.50.0\t1.fc41\tx86_64\t6213004\t1715000300\tNetwork connection manager and user applications
kernel-core\t(none)\t6.11.6\t300.fc41\tx86_64\t72000000\t1731000000\tThe Linux kernel
kernel-core\t(none)\t6.11.8\t300.fc41\tx86_64\t72100000\t1731500000\tThe Linux kernel
libX11\t(none)\t1.8.10\t2.fc41\tx86_64\t1309380\t1715000400\tCore X11 protocol client library
openssl-devel\t1\t3.2.2\t9.fc41\tx86_64\t4400000\t1715000500\tFiles for development of applications which will use OpenSSL
libreoffice-writer\t1\t24.8.3.2\t1.fc41\tx86_64\t33000000\t1731300000\tLibreOffice Word Processor
sudo\t(none)\t1.9.15\t5.p5.fc41\tx86_64\t4100000\t1715000600\tAllows restricted root access for specified users
vim-enhanced\t2\t9.1.825\t1.fc41\tx86_64\t4100000\t1731300100\tA version of the VIM editor which includes recent enhancements\twith a tab
";

    fn command() -> String {
        format!("rpm -qa --queryformat {FORMAT}")
    }

    fn scan(fixture: &Fixture, runner: FakeRunner, image_based: bool) -> Vec<Package> {
        let runner = runner.respond(&command(), LISTING);
        RpmSource::new(image_based)
            .inventory(&fixture.host(runner), &ScanContext::default())
            .unwrap()
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn lists_packages_with_unique_nevra_ids_and_skips_signing_keys() {
        let packages = scan(&Fixture::new(), FakeRunner::new(), false);
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "firefox-132.0.1-1.fc41.x86_64",
                "glibc-2.40-9.fc41.x86_64",
                "glibc-2.40-9.fc41.i686",
                "NetworkManager-1:1.50.0-1.fc41.x86_64",
                "kernel-core-6.11.6-300.fc41.x86_64",
                "kernel-core-6.11.8-300.fc41.x86_64",
                "libX11-1.8.10-2.fc41.x86_64",
                "openssl-devel-1:3.2.2-9.fc41.x86_64",
                "libreoffice-writer-1:24.8.3.2-1.fc41.x86_64",
                "sudo-1.9.15-5.p5.fc41.x86_64",
                "vim-enhanced-2:9.1.825-1.fc41.x86_64",
            ]
        );
        let firefox = find(&packages, "firefox-132.0.1-1.fc41.x86_64");
        assert_eq!(firefox.name, "firefox");
        assert_eq!(firefox.version.as_deref(), Some("132.0.1-1.fc41"));
        assert_eq!(firefox.size_bytes, Some(260_321_456));
        assert_eq!(firefox.installed_at, Some(1_731_400_000));
        assert_eq!(firefox.summary.as_deref(), Some("Mozilla Firefox Web browser"));
        let nm = find(&packages, "NetworkManager-1:1.50.0-1.fc41.x86_64");
        assert_eq!(nm.version.as_deref(), Some("1:1.50.0-1.fc41"));
        let vim = find(&packages, "vim-enhanced-2:9.1.825-1.fc41.x86_64");
        assert_eq!(
            vim.summary.as_deref(),
            Some("A version of the VIM editor which includes recent enhancements\twith a tab")
        );
    }

    #[test]
    fn dnf_systems_have_unknown_reasons() {
        let packages = scan(&Fixture::new(), FakeRunner::new(), false);
        assert!(packages.iter().all(|p| p.reason == InstallReason::Unknown));
    }

    #[test]
    fn zypp_auto_installed_marks_dependencies() {
        let fixture = Fixture::new();
        fixture.file(
            ZYPP_AUTO_INSTALLED,
            "# AutoInstalled 1.0\n# This file is maintained by libzypp.\nglibc\nlibX11\n\n",
        );
        let packages = scan(&fixture, FakeRunner::new(), false);
        assert_eq!(
            find(&packages, "glibc-2.40-9.fc41.i686").reason,
            InstallReason::Dependency
        );
        assert_eq!(
            find(&packages, "libX11-1.8.10-2.fc41.x86_64").reason,
            InstallReason::Dependency
        );
        assert_eq!(
            find(&packages, "firefox-132.0.1-1.fc41.x86_64").reason,
            InstallReason::Explicit
        );
    }

    const OSTREE_STATUS: &str = r#"{
  "deployments": [
    {
      "id": "fedora-8a0c3e...0",
      "osname": "fedora",
      "booted": false,
      "staged": true,
      "requested-packages": ["vim-enhanced", "sudo"],
      "requested-local-packages": [],
      "packages": ["vim-enhanced", "sudo"]
    },
    {
      "id": "fedora-5b1d9f...0",
      "osname": "fedora",
      "booted": true,
      "requested-packages": ["vim-enhanced"],
      "requested-local-packages": ["firefox-132.0.1-1.fc41.x86_64"],
      "requested-base-removals": [],
      "packages": ["vim-enhanced"]
    }
  ],
  "transaction": null,
  "cached-update": null,
  "update-driver": null
}"#;

    #[test]
    fn image_based_systems_take_layered_packages_from_the_booted_deployment() {
        let runner = FakeRunner::new().respond("rpm-ostree status --json", OSTREE_STATUS);
        let packages = scan(&Fixture::new(), runner, true);
        assert_eq!(
            find(&packages, "vim-enhanced-2:9.1.825-1.fc41.x86_64").reason,
            InstallReason::Explicit
        );
        assert_eq!(
            find(&packages, "firefox-132.0.1-1.fc41.x86_64").reason,
            InstallReason::Explicit
        );
        // Only layered in the staged, not yet booted, deployment.
        assert_eq!(
            find(&packages, "sudo-1.9.15-5.p5.fc41.x86_64").reason,
            InstallReason::System
        );
        assert_eq!(
            find(&packages, "glibc-2.40-9.fc41.x86_64").reason,
            InstallReason::System
        );
    }

    #[test]
    fn failing_rpm_ostree_leaves_reasons_unknown() {
        let runner = FakeRunner::new().respond_with(
            "rpm-ostree status --json",
            Output {
                code: Some(1),
                stdout: String::new(),
                stderr: "error: Couldn't start daemon".into(),
            },
        );
        let packages = scan(&Fixture::new(), runner, true);
        assert_eq!(packages.len(), 11);
        assert!(packages.iter().all(|p| p.reason == InstallReason::Unknown));
    }

    #[test]
    fn nevra_names_keep_dashes() {
        assert_eq!(
            name_of_nevra("rpm-ostree-devel-1:2021.1-2.fc33.x86_64"),
            Some("rpm-ostree-devel")
        );
        assert_eq!(name_of_nevra("foo"), None);
    }

    #[test]
    fn core_and_dnf_protected_packages_are_protected_with_the_reason() {
        let fixture = Fixture::new();
        fixture.file("/etc/dnf/protected.d/sudo.conf", "# keep root access\nsudo\n");
        fixture.file("/etc/dnf/protected.d/README", "firefox\n");
        fixture.file(
            "/etc/dnf/dnf.conf",
            "[main]\ngpgcheck=True\nprotected_packages=dnf, NetworkManager glob:/etc/dnf/protected.d/*.conf\n",
        );
        let packages = scan(&fixture, FakeRunner::new(), false);
        let why = |id: &str| find(&packages, id).protected.clone();
        assert_eq!(
            why("sudo-1.9.15-5.p5.fc41.x86_64").as_deref(),
            Some("listed in dnf's protected packages (/etc/dnf/protected.d/sudo.conf)")
        );
        assert_eq!(
            why("NetworkManager-1:1.50.0-1.fc41.x86_64").as_deref(),
            Some("listed in dnf's protected_packages (/etc/dnf/dnf.conf)")
        );
        assert!(why("glibc-2.40-9.fc41.i686").is_some());
        assert_eq!(why("firefox-132.0.1-1.fc41.x86_64"), None, "README is not a .conf file");
        assert_eq!(why("vim-enhanced-2:9.1.825-1.fc41.x86_64"), None);
    }

    #[test]
    fn only_the_running_kernel_is_protected() {
        let fixture = Fixture::new();
        fixture
            .file("/proc/sys/kernel/osrelease", "6.11.8-300.fc41.x86_64\n")
            .file("/usr/lib/modules/6.11.8-300.fc41.x86_64/vmlinuz", "");
        let runner = FakeRunner::new().respond(
            &format!("rpm -qf --queryformat {ID_FORMAT} /usr/lib/modules/6.11.8-300.fc41.x86_64/vmlinuz"),
            "kernel-core-6.11.8-300.fc41.x86_64\n",
        );
        let packages = scan(&fixture, runner, false);
        assert_eq!(
            find(&packages, "kernel-core-6.11.8-300.fc41.x86_64")
                .protected
                .as_deref(),
            Some("it is the running kernel")
        );
        assert_eq!(find(&packages, "kernel-core-6.11.6-300.fc41.x86_64").protected, None);
    }

    #[test]
    fn library_names_are_libraries() {
        let packages = scan(&Fixture::new(), FakeRunner::new(), false);
        assert_eq!(
            find(&packages, "libX11-1.8.10-2.fc41.x86_64").kind,
            PackageKind::Library
        );
        assert_eq!(
            find(&packages, "openssl-devel-1:3.2.2-9.fc41.x86_64").kind,
            PackageKind::Library
        );
        assert_eq!(
            find(&packages, "libreoffice-writer-1:24.8.3.2-1.fc41.x86_64").kind,
            PackageKind::Unknown
        );
        assert_eq!(
            find(&packages, "firefox-132.0.1-1.fc41.x86_64").kind,
            PackageKind::Unknown
        );
    }

    #[test]
    fn malformed_output_is_an_error() {
        let runner = FakeRunner::new().respond(&command(), "garbage\n");
        assert!(matches!(
            RpmSource::new(false).inventory(&Fixture::new().host(runner), &ScanContext::default()),
            Err(SourceError::Parse { .. })
        ));
    }

    #[test]
    fn probe_reads_the_version() {
        let runner = FakeRunner::new().respond("rpm --version", "RPM version 4.20.0\n");
        assert_eq!(
            RpmSource::new(false).probe(&Fixture::new().host(runner)),
            Availability::Available {
                version: Some("4.20.0".into())
            }
        );
    }

    #[test]
    fn owner_lookup_names_every_package_that_lists_a_path() {
        use crate::owner::tests::{exit, owned, paths, sorted};
        // The shape `rpm -qf --queryformat '[%{=NAME}\t%{FILENAMES}\n]'` prints on Fedora.
        let stdout = "\
google-chrome-stable\t/opt/google
google-chrome-stable\t/opt/google/chrome
google-chrome-stable\t/usr/bin/google-chrome-stable
google-earth-pro-stable\t/opt/google
google-earth-pro-stable\t/opt/google/earth
file /opt/manual is not owned by any package
";
        let runner = FakeRunner::new().respond_with(
            "rpm -qf --queryformat [%{=NAME}\\t%{FILENAMES}\\n] /opt/google /opt/manual /opt/gone",
            exit(2, stdout, "error: file /opt/gone: No such file or directory\n"),
        );
        let host = Fixture::new().host(runner);
        let owners = RpmSource::new(false)
            .owners(&host, &paths(&["/opt/google", "/opt/manual", "/opt/gone"]))
            .unwrap();
        assert_eq!(
            sorted(owners),
            [owned(
                "/opt/google",
                &["google-chrome-stable", "google-earth-pro-stable"]
            )]
        );
    }

    #[test]
    fn many_paths_are_looked_up_in_one_listing_of_every_file() {
        use crate::owner::tests::{owned, sorted};
        let stdout = "\
bash\t/usr/bin/bash
bash\t/usr/share/doc/bash
coreutils\t/usr/bin/ls
filesystem\t/usr/bin
";
        let runner = FakeRunner::new().respond("rpm -qa --queryformat [%{=NAME}\\t%{FILENAMES}\\n]", stdout);
        let host = Fixture::new().host(runner);
        let mut asked: Vec<PathBuf> = (0..WHOLE_DATABASE_FROM)
            .map(|i| PathBuf::from(format!("/usr/local/bin/tool{i}")))
            .collect();
        asked.extend([PathBuf::from("/usr/bin/ls"), PathBuf::from("/usr/bin/bash")]);
        let owners = RpmSource::new(false).owners(&host, &asked).unwrap();
        assert_eq!(
            sorted(owners),
            [owned("/usr/bin/bash", &["bash"]), owned("/usr/bin/ls", &["coreutils"])]
        );
    }

    fn installed(id: &str, name: &str) -> Package {
        Package::new(ID, id, name, Scope::System)
    }

    #[test]
    fn files_skip_the_empty_package_marker() {
        let runner = FakeRunner::new()
            .respond(
                "rpm -ql sudo-1.9.15-5.p5.fc41.x86_64",
                "/etc/sudoers\n/usr/bin/sudo\n/usr/share/doc/sudo\n",
            )
            .respond("rpm -ql fedora-release-common-41-27.noarch", "(contains no files)\n");
        let host = Fixture::new().host(runner);
        let source = RpmSource::new(false);
        let sudo = installed("sudo-1.9.15-5.p5.fc41.x86_64", "sudo");
        assert_eq!(
            source.files(&host, &sudo).unwrap(),
            [
                PathBuf::from("/etc/sudoers"),
                PathBuf::from("/usr/bin/sudo"),
                PathBuf::from("/usr/share/doc/sudo")
            ]
        );
        let empty = installed("fedora-release-common-41-27.noarch", "fedora-release-common");
        assert!(source.files(&host, &empty).unwrap().is_empty());
    }

    #[test]
    fn dependants_need_the_name_a_provided_capability_or_a_program() {
        let target = "glibc-2.40-9.fc41.x86_64";
        let offers = "\
config(glibc)
glibc
glibc(x86-64)
libc.so.6()(64bit)
/usr/lib64/libc.so.6
/usr/sbin/ldconfig
";
        // The shape rpm prints: requirers per capability, in argument order, `no package requires` when none.
        let requirers = format!(
            "{target}\n\
NetworkManager-1:1.50.0-1.fc41.x86_64\n\
glibc-langpack-en-2.40-9.fc41.x86_64\n\
no package requires glibc(x86-64)\n\
NetworkManager-1:1.50.0-1.fc41.x86_64\n\
sudo-1.9.15-5.p5.fc41.x86_64\n"
        );
        let runner = FakeRunner::new()
            .respond(&format!("rpm -q --queryformat {OFFERS_FORMAT} {target}"), offers)
            .respond_with(
                &format!(
                    "rpm -q --whatrequires /usr/sbin/ldconfig config(glibc) glibc glibc(x86-64) libc.so.6()(64bit) \
                     --queryformat {ID_FORMAT}"
                ),
                crate::owner::tests::exit(1, &requirers, ""),
            );
        let host = Fixture::new().host(runner);
        let ids = Dependants::dependants(&RpmSource::new(false), &host, &installed(target, "glibc")).unwrap();
        assert_eq!(
            ids,
            [
                "NetworkManager-1:1.50.0-1.fc41.x86_64",
                "glibc-langpack-en-2.40-9.fc41.x86_64",
                "sudo-1.9.15-5.p5.fc41.x86_64"
            ]
        );
    }

    mod files {
        use super::*;
        use crate::formats::test_support::inspect;
        use housekeeper_core::Fit;

        /// RPM's lead: magic, version 3.0, `kind` (0 binary, 1 source), architecture 1, then the name field.
        fn lead(kind: u8, name: &str) -> Vec<u8> {
            let mut lead = vec![0xed, 0xab, 0xee, 0xdb, 3, 0, 0, kind, 0, 1];
            let mut field = name.as_bytes().to_vec();
            field.resize(66, 0);
            lead.extend(field);
            lead.resize(96, 0);
            lead
        }

        #[test]
        fn without_rpm_the_lead_names_the_package_of_another_system() {
            let file = lead(0, "google-chrome-stable-131.0.6778.85-1");
            let rpm = inspect("ID=ubuntu\nID_LIKE=debian\n", "chrome.rpm", &file, |_| {
                FakeRunner::new()
            })
            .unwrap();
            assert_eq!(
                rpm.fit,
                Fit::OtherSystem {
                    families: vec!["fedora".into(), "suse".into()]
                }
            );
            let package = rpm.package.unwrap();
            assert_eq!(package.name.as_deref(), Some("google-chrome-stable"));
            assert_eq!(package.version.as_deref(), Some("131.0.6778.85-1"));
        }

        #[test]
        fn on_fedora_dnf_installs_it_and_the_rpm_database_says_what_it_needs() {
            let file = lead(0, "vim-enhanced-9.1.900-1.fc41");
            let inspection = inspect("ID=fedora\n", "vim.rpm", &file, |real| {
                FakeRunner::new()
                    .respond("rpm --version", "RPM version 4.20.0\n")
                    .respond(&command(), LISTING)
                    .respond(
                        &format!("rpm -qp --nosignature --nodigest --queryformat {RPM_FILE_FORMAT} {real}"),
                        "vim-enhanced\t2:9.1.900-1.fc41\tnoarch\n",
                    )
                    .respond_with(
                        &format!("rpm -U --test --nosignature --nodigest {real}"),
                        Output {
                            code: Some(1),
                            stdout: String::new(),
                            stderr: "error: Failed dependencies:\n\
                                     \tvim-common = 2:9.1.900-1.fc41 is needed by vim-enhanced-2:9.1.900-1.fc41.x86_64\n\
                                     \tlibgpm.so.2()(64bit) is needed by vim-enhanced-2:9.1.900-1.fc41.x86_64\n"
                                .into(),
                        },
                    )
            })
            .unwrap();
            assert_eq!(inspection.fit, housekeeper_core::Fit::Fits);
            let plan = inspection.install.unwrap();
            assert_eq!(plan.steps.len(), 1);
            assert_eq!(plan.steps[0].by, housekeeper_core::Actor::Helper);
            let housekeeper_core::Action::InstallPackageFiles { tool, argv, .. } = &plan.steps[0].action else {
                panic!("{:?}", plan.steps[0].action);
            };
            assert_eq!(tool, "dnf");
            assert_eq!(&argv[..3], ["dnf", "install", "--assumeyes"]);
            assert_eq!(plan.replaces.as_deref(), Some("2:9.1.825-1.fc41"));
            assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
            let simulation = plan.simulation.unwrap();
            assert_eq!(
                simulation.needs,
                ["vim-common = 2:9.1.900-1.fc41", "libgpm.so.2()(64bit)"]
            );
            assert_eq!(simulation.installs, ["vim-enhanced 2:9.1.900-1.fc41"]);
        }

        #[test]
        fn an_installed_package_the_new_version_would_break_blocks_it() {
            let file = lead(0, "glibc-2.41-1.fc41");
            let inspection = inspect("ID=fedora\n", "glibc.rpm", &file, |real| {
                FakeRunner::new()
                    .respond("rpm --version", "RPM version 4.20.0\n")
                    .respond(&command(), LISTING)
                    .respond(
                        &format!("rpm -qp --nosignature --nodigest --queryformat {RPM_FILE_FORMAT} {real}"),
                        "glibc\t2.41-1.fc41\tnoarch\n",
                    )
                    .respond_with(
                        &format!("rpm -U --test --nosignature --nodigest {real}"),
                        Output {
                            code: Some(1),
                            stdout: String::new(),
                            stderr: "error: Failed dependencies:\n\
                                     \tglibc = 2.40-9.fc41 is needed by (installed) glibc-langpack-en-2.40-9.fc41.x86_64\n"
                                .into(),
                        },
                    )
            })
            .unwrap();
            let plan = inspection.install.unwrap();
            assert_eq!(
                plan.blockers,
                [
                    "the package manager refuses: glibc = 2.40-9.fc41 is needed by (installed) \
                  glibc-langpack-en-2.40-9.fc41.x86_64"
                ]
            );
        }

        #[test]
        fn a_source_package_is_not_an_installer() {
            let file = lead(1, "hello-2.12.2-2.fc43");
            assert_eq!(
                inspect("ID=fedora\n", "hello.src.rpm", &file, |_| FakeRunner::new()),
                None
            );
        }
    }
}
