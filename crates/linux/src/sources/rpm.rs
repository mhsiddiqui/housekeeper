//! RPM packages (Fedora, RHEL and rebuilds, openSUSE, Silverblue), read through `rpm --queryformat`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, Cmd, Host, InstallReason, OwnerLookup, Package, PackageKind, Scope, Source, SourceError, SourceId,
    SourceInfo, SourceKind,
};
use serde::Deserialize;

use super::version_line;
use crate::owner::{OwnerQuery, args};
use crate::provider::{register, Context, Family, LinuxSource, Native};

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

/// Packages whose removal leaves the system unbootable or unmanageable, whatever the distribution configured.
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
    // The kernel itself: Fedora/RHEL and openSUSE names.
    "kernel-core",
    "kernel-default",
    // Package managers; without them nothing can be repaired.
    "rpm",
    "dnf",
    "dnf5",
    "zypper",
    "libzypp",
    "rpm-ostree",
];

/// Reads RPM packages. On image-based systems (Silverblue and friends) the install reason comes from `rpm-ostree`.
pub struct RpmSource {
    image_based: bool,
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

    fn create(context: &Context<'_>) -> Self {
        Self::new(context.distro.image_based)
    }
}
register!(RpmSource);

impl RpmSource {
    pub fn new(image_based: bool) -> Self {
        Self { image_based }
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

    fn inventory(&self, host: &Host) -> Result<Vec<Package>, SourceError> {
        let listing = host.run_ok(&Cmd::new("rpm").args(["-qa", "--queryformat", FORMAT]))?;
        let reasons = self.reasons(host)?;
        let mut protected: HashSet<String> = CORE_PACKAGES.iter().map(|name| name.to_string()).collect();
        for dir in PROTECTED_DIRS {
            for entry in host.read_dir(dir)? {
                if entry.name.ends_with(".conf") {
                    protected.extend(names(&host.read_to_string(&entry.path)?));
                }
            }
        }
        parse_listing(&listing, &reasons, &protected)
    }

    fn owner_lookup(&self) -> Option<&dyn OwnerLookup> {
        Some(self)
    }
}

impl OwnerLookup for RpmSource {
    fn owned_paths(&self, host: &Host, paths: &[PathBuf]) -> Result<HashSet<PathBuf>, SourceError> {
        OwnerQuery {
            // The owning packages' file lists: unlike `%{NAME}`, they say which argument was owned even when an
            // argument has several owners or none (missing files print nothing on stdout).
            command: |paths| {
                Cmd::new("rpm")
                    .args(["-qf", "--queryformat", "[%{FILENAMES}\\n]"])
                    .args(args(paths))
            },
            // Unowned or missing paths make rpm exit non-zero; only a killed rpm is a failure.
            failed: |output| output.code.is_none(),
            parse: owned,
        }
        .run(host, paths)
    }
}

/// One file of an owning package per line; "file … is not owned by any package" lines never equal a path.
fn owned(stdout: &str, wanted: &HashSet<&Path>) -> HashSet<PathBuf> {
    stdout
        .lines()
        .map(Path::new)
        .filter(|path| wanted.contains(path))
        .map(Path::to_path_buf)
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

fn parse_listing(listing: &str, reasons: &Reasons, protected: &HashSet<String>) -> Result<Vec<Package>, SourceError> {
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
        let evr = match epoch {
            "(none)" | "" => format!("{version}-{release}"),
            epoch => format!("{epoch}:{version}-{release}"),
        };
        // Full NEVRA: `name.arch` alone repeats for install-only packages (several kernels side by side).
        let id = match arch {
            "(none)" | "" => format!("{name}-{evr}"),
            arch => format!("{name}-{evr}.{arch}"),
        };
        let mut package = Package::new(ID, id, name, Scope::System);
        package.version = Some(evr);
        package.summary = Some(summary.to_string()).filter(|s| !s.is_empty() && s != "(none)");
        package.size_bytes = size.parse().ok();
        package.installed_at = install_time.parse().ok();
        package.reason = reasons.of(name);
        package.protected = protected.contains(name);
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
        RpmSource::new(image_based).inventory(&fixture.host(runner)).unwrap()
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
    fn core_and_dnf_protected_packages_are_protected() {
        let fixture = Fixture::new();
        fixture.file("/etc/dnf/protected.d/sudo.conf", "# keep root access\nsudo\n");
        fixture.file("/etc/dnf/protected.d/README", "firefox\n");
        let packages = scan(&fixture, FakeRunner::new(), false);
        assert!(find(&packages, "sudo-1.9.15-5.p5.fc41.x86_64").protected);
        assert!(find(&packages, "glibc-2.40-9.fc41.i686").protected);
        assert!(find(&packages, "kernel-core-6.11.6-300.fc41.x86_64").protected);
        assert!(!find(&packages, "firefox-132.0.1-1.fc41.x86_64").protected);
        assert!(!find(&packages, "vim-enhanced-2:9.1.825-1.fc41.x86_64").protected);
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
            RpmSource::new(false).inventory(&Fixture::new().host(runner)),
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
    fn owner_lookup_matches_paths_in_owner_file_lists() {
        use crate::owner::tests::{exit, paths, sorted};
        let stdout = "\
/opt/google
/opt/google/chrome
/opt/google/chrome/chrome
/usr/bin/google-chrome-stable
file /opt/manual is not owned by any package
";
        let runner = FakeRunner::new().respond_with(
            "rpm -qf --queryformat [%{FILENAMES}\\n] /opt/google /opt/manual /opt/gone",
            exit(2, stdout, "error: file /opt/gone: No such file or directory\n"),
        );
        let host = Fixture::new().host(runner);
        let owned = RpmSource::new(false)
            .owned_paths(&host, &paths(&["/opt/google", "/opt/manual", "/opt/gone"]))
            .unwrap();
        assert_eq!(sorted(owned), paths(&["/opt/google"]));
    }
}
