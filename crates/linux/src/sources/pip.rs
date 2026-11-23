//! Python packages installed with `pip install --user`, read from their `.dist-info` directories (PEP 376, PEP 566).

use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, FileType, Host, InstallReason, Package, PackageKind, Scope, Source, SourceError, SourceId,
    SourceInfo, SourceKind, ScanContext,
};

use crate::provider::{Context, LinuxSource};

pub const ID: SourceId = SourceId::new("pip");

pub struct PipUserSource;

impl LinuxSource for PipUserSource {
    fn create(_: &Context<'_>) -> Self {
        Self
    }
}

impl Source for PipUserSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("pip", "pip (user)", SourceKind::Language)
    }

    fn probe(&self, host: &Host) -> Availability {
        match site_packages(host, &user_base(host)) {
            Ok(dirs) if !dirs.is_empty() => Availability::Available { version: None },
            _ => Availability::unavailable("no user site-packages directory found"),
        }
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> { let mut packages = Vec::new();
    for (python, dir) in site_packages(host, &user_base(host))? {
        for entry in host.read_dir(&dir)? {
            if entry.file_type != Some(FileType::Dir) {
                continue;
            }
            if let Some((name, version)) = split_dist_info(&entry.name) {
                packages.push(read_dist_info(host, &python, &entry.path, name, version)?);
            }
        }
    }
    Ok(packages) }
}

/// `$PYTHONUSERBASE`, defaulting to `~/.local`.
fn user_base(host: &Host) -> PathBuf {
    host.dir_from_var("PYTHONUSERBASE", ".local")
}

/// `(pythonX.Y, <prefix>/lib/pythonX.Y/site-packages)` for each Python 3 that has one.
pub(super) fn site_packages(host: &Host, prefix: &Path) -> Result<Vec<(String, PathBuf)>, SourceError> {
    Ok(host
        .read_dir(prefix.join("lib"))?
        .into_iter()
        .filter(|entry| entry.name.starts_with("python3.") && entry.file_type == Some(FileType::Dir))
        .map(|entry| (entry.name, entry.path.join("site-packages")))
        .filter(|(_, dir)| host.is_dir(dir))
        .collect())
}

/// `(name, version)` from a `<name>-<version>.dist-info` directory name. Versions never contain `-`.
pub(super) fn split_dist_info(dir_name: &str) -> Option<(&str, &str)> {
    let (name, version) = dir_name.strip_suffix(".dist-info")?.rsplit_once('-')?;
    (!name.is_empty() && !version.is_empty()).then_some((name, version))
}

/// PEP 503 normalized form: lowercase, runs of `-`, `_` and `.` collapsed to `-`.
pub(super) fn normalize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

fn read_dist_info(
    host: &Host,
    python: &str,
    dir: &Path,
    dir_name: &str,
    dir_version: &str,
) -> Result<Package, SourceError> {
    let metadata = host.read_optional(dir.join("METADATA"))?.unwrap_or_default();
    let headers = headers(&metadata);
    let header = |key: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| *v)
            .filter(|v| !v.is_empty() && *v != "UNKNOWN")
    };
    let name = header("Name").unwrap_or(dir_name);
    let mut package = Package::new(ID, format!("{python}/{name}"), name, Scope::User);
    package.version = Some(header("Version").unwrap_or(dir_version).to_string());
    package.summary = header("Summary").map(str::to_string);
    package.commands = host
        .read_optional(dir.join("entry_points.txt"))?
        .map(|text| scripts(&text))
        .unwrap_or_default();
    package.kind = if package.commands.is_empty() {
        PackageKind::Library
    } else {
        PackageKind::App
    };
    // PEP 376: installers write REQUESTED only for what the user asked for.
    package.reason = if host.exists(dir.join("REQUESTED")) {
        InstallReason::Explicit
    } else {
        InstallReason::Dependency
    };
    let python_label = python.replacen("python", "Python ", 1);
    let installer = host
        .read_optional(dir.join("INSTALLER"))?
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    package.origin = Some(match installer {
        Some(installer) => format!("{installer}, {python_label}"),
        None => python_label,
    });
    package.location = Some(dir.to_path_buf());
    Ok(package)
}

/// Header fields of a core-metadata file; the body after the first blank line is the description.
fn headers(metadata: &str) -> Vec<(&str, &str)> {
    metadata
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect()
}

/// Script names from the `console_scripts` and `gui_scripts` sections of `entry_points.txt`.
fn scripts(entry_points: &str) -> Vec<String> {
    let mut in_scripts = false;
    let mut names = Vec::new();
    for line in entry_points.lines().map(str::trim) {
        if line.starts_with('[') {
            in_scripts = matches!(line, "[console_scripts]" | "[gui_scripts]");
            continue;
        }
        if !in_scripts || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            names.push(name.trim().to_string());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const SITE_311: &str = "/home/user/.local/lib/python3.11/site-packages";
    const SITE_312: &str = "/home/user/.local/lib/python3.12/site-packages";

    const COVERAGE_METADATA: &str = "\
Metadata-Version: 2.4
Name: coverage
Version: 7.14.0
Summary: Code coverage measurement for Python
Home-page: https://github.com/coveragepy/coveragepy
Classifier: Development Status :: 5 - Production/Stable
Requires-Python: >=3.10
Description-Content-Type: text/x-rst

Summary: not a header, part of the description
";

    const COVERAGE_ENTRY_POINTS: &str = "\
[console_scripts]
coverage = coverage.cmdline:main
coverage-3.11 = coverage.cmdline:main_deprecated

[pytest11]
cov = pytest_cov.plugin
";

    fn fixture() -> Fixture {
        let fixture = Fixture::new();
        let coverage = format!("{SITE_311}/coverage-7.14.0.dist-info");
        let markupsafe = format!("{SITE_311}/markupsafe-3.0.3.dist-info");
        let mako = format!("{SITE_312}/Mako-1.3.10.dist-info");
        fixture
            .file(&format!("{coverage}/METADATA"), COVERAGE_METADATA)
            .file(&format!("{coverage}/entry_points.txt"), COVERAGE_ENTRY_POINTS)
            .file(&format!("{coverage}/INSTALLER"), "pip\n")
            .file(&format!("{coverage}/REQUESTED"), "")
            .file(
                &format!("{markupsafe}/METADATA"),
                "Metadata-Version: 2.1\nName: MarkupSafe\nVersion: 3.0.3\nSummary: Safely add untrusted strings to HTML/XML markup.\n",
            )
            .file(&format!("{markupsafe}/INSTALLER"), "uv")
            .dir(&format!("{SITE_311}/coverage"))
            .file(&format!("{SITE_311}/a1_coverage.pth"), "")
            .file(
                &format!("{mako}/METADATA"),
                "Metadata-Version: 2.1\nName: Mako\nVersion: 1.3.10\nSummary: UNKNOWN\n",
            )
            .file(&format!("{mako}/REQUESTED"), "")
            .file(&format!("{mako}/entry_points.txt"), "[console_scripts]\nmako-render = mako.cmd:cmdline\n");
        fixture
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn lists_dist_info_per_python_version() {
        let packages = PipUserSource.inventory(&fixture().host(FakeRunner::new()), &ScanContext::default()).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["python3.11/coverage", "python3.11/MarkupSafe", "python3.12/Mako"]);
        let coverage = find(&packages, "python3.11/coverage");
        assert_eq!(coverage.version.as_deref(), Some("7.14.0"));
        assert_eq!(
            coverage.summary.as_deref(),
            Some("Code coverage measurement for Python")
        );
        assert_eq!(coverage.origin.as_deref(), Some("pip, Python 3.11"));
        assert_eq!(coverage.scope, Scope::User);
        assert_eq!(
            coverage.location.as_deref(),
            Some(Path::new(SITE_311).join("coverage-7.14.0.dist-info").as_path())
        );
        assert_eq!(
            find(&packages, "python3.11/MarkupSafe").origin.as_deref(),
            Some("uv, Python 3.11")
        );
        assert_eq!(find(&packages, "python3.12/Mako").summary, None);
    }

    #[test]
    fn requested_marker_decides_the_reason() {
        let packages = PipUserSource.inventory(&fixture().host(FakeRunner::new()), &ScanContext::default()).unwrap();
        assert_eq!(find(&packages, "python3.11/coverage").reason, InstallReason::Explicit);
        assert_eq!(
            find(&packages, "python3.11/MarkupSafe").reason,
            InstallReason::Dependency
        );
    }

    #[test]
    fn console_scripts_are_commands() {
        let packages = PipUserSource.inventory(&fixture().host(FakeRunner::new()), &ScanContext::default()).unwrap();
        let coverage = find(&packages, "python3.11/coverage");
        assert_eq!(coverage.commands, ["coverage", "coverage-3.11"]);
        assert_eq!(coverage.kind, PackageKind::App);
        let markupsafe = find(&packages, "python3.11/MarkupSafe");
        assert!(markupsafe.commands.is_empty());
        assert_eq!(markupsafe.kind, PackageKind::Library);
    }

    #[test]
    fn falls_back_to_the_directory_name_without_metadata() {
        let fixture = Fixture::new();
        fixture.dir("/srv/pybase/lib/python3.13/site-packages/requests-2.32.5.dist-info");
        let host = fixture.host_with_vars(FakeRunner::new(), &[("PYTHONUSERBASE", "/srv/pybase")]);
        let packages = PipUserSource.inventory(&host, &ScanContext::default()).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].id, "python3.13/requests");
        assert_eq!(packages[0].version.as_deref(), Some("2.32.5"));
        assert_eq!(packages[0].origin.as_deref(), Some("Python 3.13"));
        assert_eq!(packages[0].reason, InstallReason::Dependency);
    }

    #[test]
    fn probe_needs_a_user_site_packages() {
        let fixture = Fixture::new();
        fixture.dir("/home/user/.local/lib/python2.7/site-packages");
        assert!(matches!(
            PipUserSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Unavailable { .. }
        ));
        fixture.dir(SITE_312);
        assert_eq!(
            PipUserSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Available { version: None }
        );
    }

    #[test]
    fn name_helpers() {
        assert_eq!(
            split_dist_info("zope.interface-7.2.dist-info"),
            Some(("zope.interface", "7.2"))
        );
        assert_eq!(
            split_dist_info("graphifyy-0.9.100.dist-info"),
            Some(("graphifyy", "0.9.100"))
        );
        assert_eq!(split_dist_info("noversion.dist-info"), None);
        assert_eq!(split_dist_info("foo-1.0.egg-info"), None);
        assert_eq!(normalize_name("Zope.Interface"), "zope-interface");
        assert_eq!(normalize_name("ruamel__yaml"), "ruamel-yaml");
    }
}
