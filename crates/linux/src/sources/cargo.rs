//! Rust programs installed with `cargo install`, read from cargo's install tracking files.

use std::collections::BTreeMap;
use std::path::PathBuf;

use housekeeper_core::{
    Availability, Host, InstallReason, Package, PackageKind, Scope, Source, SourceError, SourceId, SourceInfo,
    SourceKind,
};
use serde::Deserialize;

pub const ID: SourceId = SourceId::new("cargo");

const CRATES_IO: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

pub struct CargoSource;

impl Source for CargoSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("cargo", "cargo install", SourceKind::Language)
    }

    fn probe(&self, host: &Host) -> Availability {
        if host.is_dir(cargo_home(host)) {
            Availability::Available { version: None }
        } else {
            Availability::unavailable("no cargo home directory found")
        }
    }

    fn inventory(&self, host: &Host) -> Result<Vec<Package>, SourceError> {
        let home = cargo_home(host);
        let installs = if let Some(json) = host.read_optional(home.join(".crates2.json"))? {
            parse_crates2(&json)?
        } else if let Some(toml) = host.read_optional(home.join(".crates.toml"))? {
            parse_crates_v1(&toml)?
        } else {
            return Ok(Vec::new());
        };
        let bin = home.join("bin");
        let scope = if bin.starts_with(host.home()) {
            Scope::User
        } else {
            Scope::System
        };
        installs
            .into_iter()
            .map(|(key, bins)| {
                let (name, version, source) = split_key(&key)?;
                let mut package = Package::new(ID, name, name, scope);
                package.version = Some(version.to_string());
                package.origin = Some(origin(source));
                package.location = Some(bin.clone());
                package.commands = bins;
                package.kind = PackageKind::App;
                package.reason = InstallReason::Explicit;
                Ok(package)
            })
            .collect()
    }
}

/// `$CARGO_HOME`, defaulting to `~/.cargo`.
fn cargo_home(host: &Host) -> PathBuf {
    host.dir_from_var("CARGO_HOME", ".cargo")
}

#[derive(Deserialize)]
struct Crates2 {
    #[serde(default)]
    installs: BTreeMap<String, Install>,
}

#[derive(Deserialize)]
struct Install {
    #[serde(default)]
    bins: Vec<String>,
}

#[derive(Deserialize)]
struct CratesV1 {
    #[serde(default)]
    v1: BTreeMap<String, Vec<String>>,
}

/// Package key → binaries, from `.crates2.json` (cargo 1.41+).
fn parse_crates2(json: &str) -> Result<BTreeMap<String, Vec<String>>, SourceError> {
    let crates: Crates2 = serde_json::from_str(json).map_err(|e| SourceError::parse(".crates2.json", e.to_string()))?;
    Ok(crates
        .installs
        .into_iter()
        .map(|(key, install)| (key, install.bins))
        .collect())
}

/// Package key → binaries, from the older `.crates.toml`.
fn parse_crates_v1(text: &str) -> Result<BTreeMap<String, Vec<String>>, SourceError> {
    let crates: CratesV1 = toml::from_str(text).map_err(|e| SourceError::parse(".crates.toml", e.to_string()))?;
    Ok(crates.v1)
}

/// `(name, version, source)` from a package key such as `rmux 0.8.0 (registry+https://…)`.
fn split_key(key: &str) -> Result<(&str, &str, &str), SourceError> {
    let parsed = key.split_once(' ').and_then(|(name, rest)| {
        let (version, source) = rest.split_once(' ')?;
        let source = source.strip_prefix('(')?.strip_suffix(')')?;
        Some((name, version, source))
    });
    parsed.ok_or_else(|| SourceError::parse("cargo install record", format!("unexpected package key {key:?}")))
}

/// `crates.io`, the repository URL of a git install, `local path`, or the URL of another registry.
fn origin(source: &str) -> String {
    if CRATES_IO.contains(&source) {
        return "crates.io".to_string();
    }
    if source.starts_with("path+") {
        return "local path".to_string();
    }
    let (_, url) = source.split_once('+').unwrap_or(("", source));
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use housekeeper_core::testing::{FakeRunner, Fixture};

    const CRATES2: &str = r#"{"installs":{
"cargo-packager 0.11.8 (registry+https://github.com/rust-lang/crates.io-index)":{"version_req":null,"bins":["cargo-packager"],"features":[],"all_features":false,"no_default_features":false,"profile":"release","target":"x86_64-unknown-linux-gnu","rustc":"rustc 1.91.1 (ed61e7d7e 2025-11-07)"},
"rmux 0.8.0 (sparse+https://index.crates.io/)":{"version_req":null,"bins":["rmux","rmux-daemon"],"features":[],"all_features":false,"no_default_features":false,"profile":"release","target":"x86_64-unknown-linux-gnu","rustc":"rustc 1.91.1"},
"zellij 0.44.0 (git+https://github.com/zellij-org/zellij?branch=main#a1b2c3d4e5)":{"bins":["zellij"]},
"mytool 0.1.0 (path+file:///home/user/src/mytool)":{"bins":["mytool"]}
}}"#;

    const CRATES_V1: &str = r#"[v1]
"cargo-packager 0.11.8 (registry+https://github.com/rust-lang/crates.io-index)" = ["cargo-packager"]
"rmux 0.8.0 (registry+https://github.com/rust-lang/crates.io-index)" = [
    "rmux",
    "rmux-daemon",
]
"#;

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn reads_crates2_json() {
        let fixture = Fixture::new();
        fixture.file("/home/user/.cargo/.crates2.json", CRATES2).file(
            "/home/user/.cargo/.crates.toml",
            "[v1]\n\"stale 0.1.0 (path+file:///x)\" = [\"stale\"]\n",
        );
        let packages = CargoSource.inventory(&fixture.host(FakeRunner::new())).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["cargo-packager", "mytool", "rmux", "zellij"]);

        let rmux = find(&packages, "rmux");
        assert_eq!(rmux.version.as_deref(), Some("0.8.0"));
        assert_eq!(rmux.commands, ["rmux", "rmux-daemon"]);
        assert_eq!(rmux.origin.as_deref(), Some("crates.io"));
        assert_eq!(rmux.kind, PackageKind::App);
        assert_eq!(rmux.reason, InstallReason::Explicit);
        assert_eq!(rmux.scope, Scope::User);
        assert_eq!(rmux.location.as_deref(), Some(Path::new("/home/user/.cargo/bin")));
        assert_eq!(find(&packages, "cargo-packager").origin.as_deref(), Some("crates.io"));
        assert_eq!(
            find(&packages, "zellij").origin.as_deref(),
            Some("https://github.com/zellij-org/zellij")
        );
        assert_eq!(find(&packages, "mytool").origin.as_deref(), Some("local path"));
    }

    #[test]
    fn falls_back_to_crates_toml_under_cargo_home() {
        let fixture = Fixture::new();
        fixture.file("/opt/cargo/.crates.toml", CRATES_V1);
        let host = fixture.host_with_vars(FakeRunner::new(), &[("CARGO_HOME", "/opt/cargo")]);
        let packages = CargoSource.inventory(&host).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["cargo-packager", "rmux"]);
        assert_eq!(find(&packages, "rmux").commands, ["rmux", "rmux-daemon"]);
        assert_eq!(find(&packages, "rmux").scope, Scope::System);
        assert_eq!(
            find(&packages, "rmux").location.as_deref(),
            Some(Path::new("/opt/cargo/bin"))
        );
    }

    #[test]
    fn nothing_installed_is_empty() {
        let fixture = Fixture::new();
        fixture.dir("/home/user/.cargo/bin");
        assert!(
            CargoSource
                .inventory(&fixture.host(FakeRunner::new()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_records_are_errors() {
        let fixture = Fixture::new();
        fixture.file("/home/user/.cargo/.crates2.json", "{\"installs\": [");
        assert!(matches!(
            CargoSource.inventory(&fixture.host(FakeRunner::new())),
            Err(SourceError::Parse { .. })
        ));
        fixture.file(
            "/home/user/.cargo/.crates2.json",
            r#"{"installs":{"just-a-name":{"bins":[]}}}"#,
        );
        assert!(matches!(
            CargoSource.inventory(&fixture.host(FakeRunner::new())),
            Err(SourceError::Parse { .. })
        ));
    }

    #[test]
    fn probe_needs_cargo_home() {
        let fixture = Fixture::new();
        assert!(matches!(
            CargoSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Unavailable { .. }
        ));
        fixture.dir("/home/user/.cargo");
        assert_eq!(
            CargoSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Available { version: None }
        );
    }
}
