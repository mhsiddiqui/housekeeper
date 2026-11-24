//! Homebrew on Linux: formulae in `<prefix>/Cellar`, casks in `<prefix>/Caskroom`.
//!
//! Read from the files Homebrew writes for every install (`INSTALL_RECEIPT.json`, the `opt/` links), never by running
//! `brew`, which starts Ruby and can take seconds (decision D12).

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use housekeeper_core::{
    Availability, FileType, Host, InstallReason, Package, PackageKind, ScanContext, Scope, Source, SourceError,
    SourceId, SourceInfo, SourceKind,
};
use serde::Deserialize;

use crate::provider::{Context, LinuxSource, register};

pub const ID: SourceId = SourceId::new("homebrew");

/// Where the official installer puts Homebrew on Linux.
const DEFAULT_PREFIX: &str = "/home/linuxbrew/.linuxbrew";

/// Formulae installed from the default tap name it differently in older receipts.
const CORE_TAP: &str = "homebrew/core";

pub struct HomebrewSource;

impl LinuxSource for HomebrewSource {
    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(HomebrewSource);

impl Source for HomebrewSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("homebrew", "Homebrew", SourceKind::Universal)
    }

    fn probe(&self, host: &Host) -> Availability {
        if prefixes(host).is_empty() {
            Availability::unavailable("Homebrew is not installed")
        } else {
            Availability::Available { version: None }
        }
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let prefixes = prefixes(host);
        let qualify = prefixes.len() > 1;
        let mut packages = Vec::new();
        for prefix in &prefixes {
            let scope = if prefix.starts_with(host.home()) {
                Scope::User
            } else {
                Scope::System
            };
            // Ids only need the prefix when two installations exist side by side.
            let id = |kind: &str, name: &str| {
                if qualify {
                    format!("{}:{kind}/{name}", prefix.display())
                } else {
                    format!("{kind}/{name}")
                }
            };
            packages.extend(formulae(host, prefix, scope, &id)?);
            packages.extend(casks(host, prefix, scope, &id)?);
        }
        Ok(packages)
    }
}

/// Installed prefixes: `$HOMEBREW_PREFIX`, the default `/home/linuxbrew/.linuxbrew`, then `~/.linuxbrew`.
fn prefixes(host: &Host) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(prefix) = host.var("HOMEBREW_PREFIX").filter(|p| p.starts_with('/')) {
        candidates.push(PathBuf::from(prefix));
    }
    candidates.push(PathBuf::from(DEFAULT_PREFIX));
    candidates.push(host.home().join(".linuxbrew"));
    let mut found: Vec<PathBuf> = Vec::new();
    for prefix in candidates {
        if host.exists(prefix.join("bin/brew")) && !found.contains(&prefix) {
            found.push(prefix);
        }
    }
    found
}

/// The parts of `INSTALL_RECEIPT.json` Housekeeper uses.
#[derive(Deserialize)]
struct Receipt {
    #[serde(default)]
    installed_on_request: bool,
    #[serde(default)]
    installed_as_dependency: bool,
    time: Option<i64>,
    source: Option<ReceiptSource>,
}

#[derive(Deserialize)]
struct ReceiptSource {
    tap: Option<String>,
}

fn formulae(
    host: &Host,
    prefix: &Path,
    scope: Scope,
    id: &dyn Fn(&str, &str) -> String,
) -> Result<Vec<Package>, SourceError> {
    let mut packages = Vec::new();
    for formula in host.read_dir(prefix.join("Cellar"))? {
        if formula.file_type != Some(FileType::Dir) {
            continue;
        }
        let Some(version) = current_version(host, prefix, &formula.name, &formula.path)? else {
            continue;
        };
        let keg = formula.path.join(&version);
        let mut package = Package::new(ID, id("formula", &formula.name), &formula.name, scope);
        package.version = Some(version);
        package.location = Some(keg.clone());
        package.commands = host
            .read_dir(keg.join("bin"))?
            .into_iter()
            .filter(|entry| matches!(entry.file_type, Some(FileType::File { executable: true })))
            .map(|entry| entry.name)
            .collect();
        package.kind = if package.commands.is_empty() {
            PackageKind::Library
        } else {
            PackageKind::App
        };
        let receipt_path = keg.join("INSTALL_RECEIPT.json");
        match host.read_optional(&receipt_path)? {
            Some(text) => {
                let receipt: Receipt = serde_json::from_str(&text)
                    .map_err(|e| SourceError::parse(receipt_path.display().to_string(), e.to_string()))?;
                package.reason = if receipt.installed_on_request {
                    InstallReason::Explicit
                } else if receipt.installed_as_dependency {
                    InstallReason::Dependency
                } else {
                    InstallReason::Unknown
                };
                package.installed_at = receipt.time;
                package.origin = Some(
                    receipt
                        .source
                        .and_then(|source| source.tap)
                        .unwrap_or_else(|| CORE_TAP.to_string()),
                );
            }
            // Kegs installed by very old Homebrew versions, or half-finished installs.
            None => package.origin = Some(CORE_TAP.to_string()),
        }
        packages.push(package);
    }
    Ok(packages)
}

/// The version `opt/<name>` links to (the one in use), else the highest installed one.
fn current_version(host: &Host, prefix: &Path, name: &str, cellar: &Path) -> Result<Option<String>, SourceError> {
    let linked = host
        .link_target(prefix.join("opt").join(name))
        .filter(|target| target.parent() == Some(cellar))
        .and_then(|target| target.file_name().map(|v| v.to_string_lossy().into_owned()));
    if linked.is_some() {
        return Ok(linked);
    }
    Ok(host
        .read_dir(cellar)?
        .into_iter()
        .filter(|entry| entry.file_type == Some(FileType::Dir))
        .map(|entry| entry.name)
        .max_by(|a, b| compare_versions(a, b)))
}

/// Orders `1.10.0` after `1.9.2`: numeric runs compare as numbers, everything else as text.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> Vec<(u64, String)> {
        s.split(['.', '_', '-'])
            .map(|part| (part.parse().unwrap_or(0), part.to_string()))
            .collect()
    };
    parts(a).cmp(&parts(b))
}

fn casks(
    host: &Host,
    prefix: &Path,
    scope: Scope,
    id: &dyn Fn(&str, &str) -> String,
) -> Result<Vec<Package>, SourceError> {
    let mut packages = Vec::new();
    for cask in host.read_dir(prefix.join("Caskroom"))? {
        if cask.file_type != Some(FileType::Dir) {
            continue;
        }
        // `.metadata` holds Homebrew's own records, not an installed version.
        let versions: Vec<_> = host
            .read_dir(&cask.path)?
            .into_iter()
            .filter(|entry| entry.file_type == Some(FileType::Dir) && !entry.name.starts_with('.'))
            .collect();
        let Some(newest) = versions.iter().max_by(|a, b| compare_versions(&a.name, &b.name)) else {
            continue;
        };
        let mut package = Package::new(ID, id("cask", &cask.name), &cask.name, scope);
        package.version = Some(newest.name.clone());
        package.kind = PackageKind::App;
        package.reason = InstallReason::Explicit;
        package.origin = Some("homebrew/cask".to_string());
        package.location = Some(cask.path.clone());
        package.installed_at = host
            .metadata(&newest.path)
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|age| i64::try_from(age.as_secs()).ok());
        packages.push(package);
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};

    const PREFIX: &str = "/home/linuxbrew/.linuxbrew";
    const ELF: &[u8] = b"\x7fELF";

    /// A receipt as Homebrew 4 writes it (trimmed).
    fn receipt(on_request: bool, tap: &str) -> String {
        format!(
            r#"{{"homebrew_version":"4.4.6","used_options":[],"unused_options":[],"built_as_bottle":true,
"poured_from_bottle":true,"loaded_from_api":true,"installed_as_dependency":{dep},"installed_on_request":{on_request},
"changed_files":[],"time":1733400000,"source_modified_time":1733300000,"compiler":"gcc-11",
"aliases":[],"runtime_dependencies":[],"source":{{"spec":"stable","versions":{{"stable":"1.7.1","head":null,
"version_scheme":0}},"path":"/home/linuxbrew/.linuxbrew/Homebrew/Library/Taps/homebrew/homebrew-core/Formula/j/jq.rb",
"tap":"{tap}","tap_git_head":null}},"arch":"x86_64","built_on":{{"os":"Linux"}}}}"#,
            dep = !on_request
        )
    }

    fn brew(fixture: &Fixture, prefix: &str) {
        fixture.executable(&format!("{prefix}/bin/brew"), "#!/bin/bash\n");
    }

    fn scan(fixture: &Fixture, vars: &[(&str, &str)]) -> Vec<Package> {
        HomebrewSource
            .inventory(
                &fixture.host_with_vars(FakeRunner::new(), vars),
                &ScanContext::default(),
            )
            .unwrap()
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("{id} missing"))
    }

    #[test]
    fn formulae_with_reasons_versions_and_commands() {
        let fixture = Fixture::new();
        brew(&fixture, PREFIX);
        fixture
            .file(
                &format!("{PREFIX}/Cellar/jq/1.7.1/INSTALL_RECEIPT.json"),
                receipt(true, "homebrew/core"),
            )
            .executable(&format!("{PREFIX}/Cellar/jq/1.7.1/bin/jq"), ELF)
            .file(
                &format!("{PREFIX}/Cellar/oniguruma/6.9.9/INSTALL_RECEIPT.json"),
                receipt(false, "homebrew/core"),
            )
            .file(&format!("{PREFIX}/Cellar/oniguruma/6.9.9/lib/libonig.so"), "")
            .symlink(&format!("{PREFIX}/opt/jq"), "../Cellar/jq/1.7.1");
        let packages = scan(&fixture, &[]);

        let jq = find(&packages, "formula/jq");
        assert_eq!(jq.version.as_deref(), Some("1.7.1"));
        assert_eq!((jq.reason, jq.kind), (InstallReason::Explicit, PackageKind::App));
        assert_eq!(jq.commands, ["jq"]);
        assert_eq!(jq.installed_at, Some(1_733_400_000));
        assert_eq!(jq.origin.as_deref(), Some("homebrew/core"));
        assert_eq!(jq.scope, Scope::System);
        assert_eq!(
            jq.location.as_deref(),
            Some(Path::new("/home/linuxbrew/.linuxbrew/Cellar/jq/1.7.1"))
        );

        let onig = find(&packages, "formula/oniguruma");
        assert_eq!(
            (onig.reason, onig.kind),
            (InstallReason::Dependency, PackageKind::Library)
        );
    }

    #[test]
    fn the_linked_version_wins_over_newer_unlinked_ones() {
        let fixture = Fixture::new();
        brew(&fixture, PREFIX);
        fixture
            .file(
                &format!("{PREFIX}/Cellar/node/22.11.0/INSTALL_RECEIPT.json"),
                receipt(true, "homebrew/core"),
            )
            .file(
                &format!("{PREFIX}/Cellar/node/23.3.0/INSTALL_RECEIPT.json"),
                receipt(true, "homebrew/core"),
            )
            .symlink(&format!("{PREFIX}/opt/node"), "../Cellar/node/22.11.0");
        assert_eq!(
            find(&scan(&fixture, &[]), "formula/node").version.as_deref(),
            Some("22.11.0")
        );
    }

    #[test]
    fn without_a_link_the_highest_version_is_used() {
        let fixture = Fixture::new();
        brew(&fixture, PREFIX);
        fixture
            .dir(&format!("{PREFIX}/Cellar/go/1.9.2"))
            .dir(&format!("{PREFIX}/Cellar/go/1.10.0"));
        let go = find(&scan(&fixture, &[]), "formula/go").clone();
        assert_eq!(go.version.as_deref(), Some("1.10.0"));
        assert_eq!(go.reason, InstallReason::Unknown, "no receipt");
    }

    #[test]
    fn third_party_taps_are_the_origin() {
        let fixture = Fixture::new();
        brew(&fixture, PREFIX);
        fixture.file(
            &format!("{PREFIX}/Cellar/k9s/0.32.7/INSTALL_RECEIPT.json"),
            receipt(true, "derailed/k9s"),
        );
        assert_eq!(
            find(&scan(&fixture, &[]), "formula/k9s").origin.as_deref(),
            Some("derailed/k9s")
        );
    }

    #[test]
    fn casks_skip_homebrew_metadata() {
        let fixture = Fixture::new();
        brew(&fixture, PREFIX);
        fixture
            .dir(&format!("{PREFIX}/Caskroom/font-fira-code/6.2"))
            .dir(&format!(
                "{PREFIX}/Caskroom/font-fira-code/.metadata/6.2/20241201000000.000/Casks"
            ));
        let packages = scan(&fixture, &[]);
        assert_eq!(packages.len(), 1);
        let font = find(&packages, "cask/font-fira-code");
        assert_eq!(font.version.as_deref(), Some("6.2"));
        assert_eq!(font.origin.as_deref(), Some("homebrew/cask"));
    }

    #[test]
    fn home_prefix_and_env_override_are_found_and_kept_apart() {
        let fixture = Fixture::new();
        let home_prefix = format!("{HOME}/.linuxbrew");
        brew(&fixture, &home_prefix);
        brew(&fixture, "/opt/brew");
        fixture
            .dir(&format!("{home_prefix}/Cellar/hello/2.12.1"))
            .dir("/opt/brew/Cellar/hello/2.12.1");
        let packages = scan(&fixture, &[("HOMEBREW_PREFIX", "/opt/brew")]);
        assert_eq!(packages.len(), 2);
        let mine = find(&packages, &format!("{home_prefix}:formula/hello"));
        assert_eq!(mine.scope, Scope::User);
        assert_eq!(find(&packages, "/opt/brew:formula/hello").scope, Scope::System);
    }

    #[test]
    fn probe_needs_a_brew_executable() {
        let fixture = Fixture::new();
        fixture.dir(&format!("{PREFIX}/Cellar"));
        let host = fixture.host(FakeRunner::new());
        assert!(matches!(HomebrewSource.probe(&host), Availability::Unavailable { .. }));
        brew(&fixture, PREFIX);
        assert_eq!(HomebrewSource.probe(&host), Availability::Available { version: None });
    }

    #[test]
    fn a_broken_receipt_is_reported() {
        let fixture = Fixture::new();
        brew(&fixture, PREFIX);
        fixture.file(&format!("{PREFIX}/Cellar/bad/1.0/INSTALL_RECEIPT.json"), "{not json");
        let host = fixture.host(FakeRunner::new());
        assert!(matches!(
            HomebrewSource.inventory(&host, &ScanContext::default()),
            Err(SourceError::Parse { .. })
        ));
    }
}
