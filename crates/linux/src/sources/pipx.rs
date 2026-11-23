//! Python apps installed with pipx, read from each venv's `pipx_metadata.json`.

use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, FileType, Host, InstallReason, Package, PackageKind, Scope, Source, SourceError, SourceId,
    SourceInfo, SourceKind, ScanContext,
};
use serde::Deserialize;

use crate::provider::{Context, LinuxSource};

pub const ID: SourceId = SourceId::new("pipx");

pub struct PipxSource;

impl LinuxSource for PipxSource {
    fn create(_: &Context<'_>) -> Self {
        Self
    }
}

impl Source for PipxSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("pipx", "pipx", SourceKind::Language)
    }

    fn probe(&self, host: &Host) -> Availability {
        if host.is_dir(venvs_dir(host)) {
            Availability::Available { version: None }
        } else {
            Availability::unavailable("no pipx venvs directory found")
        }
    }

    fn install_roots(&self, host: &Host) -> Vec<PathBuf> {
        vec![venvs_dir(host)]
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> { let mut packages = Vec::new();
    for entry in host.read_dir(venvs_dir(host))? {
        if entry.file_type != Some(FileType::Dir) || entry.name.starts_with('.') {
            continue;
        }
        let metadata = host
            .read_optional(entry.path.join("pipx_metadata.json"))?
            .and_then(|text| serde_json::from_str::<Metadata>(&text).ok());
        packages.push(to_package(&entry.name, &entry.path, metadata));
    }
    Ok(packages) }
}

/// `$PIPX_HOME`, else the XDG location pipx 1.3+ uses, else the legacy `~/.local/pipx`.
fn venvs_dir(host: &Host) -> PathBuf {
    let home = match host.var("PIPX_HOME").filter(|dir| dir.starts_with('/')) {
        Some(dir) => PathBuf::from(dir),
        None => {
            let xdg = host.data_home().join("pipx");
            if host.is_dir(&xdg) {
                xdg
            } else {
                host.home().join(".local/pipx")
            }
        }
    };
    home.join("venvs")
}

#[derive(Deserialize)]
struct Metadata {
    main_package: MainPackage,
    python_version: Option<String>,
}

#[derive(Deserialize)]
struct MainPackage {
    package: String,
    package_version: Option<String>,
    #[serde(default)]
    apps: Vec<String>,
    #[serde(default)]
    apps_of_dependencies: Vec<String>,
    #[serde(default)]
    include_dependencies: bool,
    /// Appended to the venv and every exposed command (`pipx install --suffix`).
    #[serde(default)]
    suffix: String,
}

/// The venv name is the handle pipx acts on; without readable metadata it is all we know.
fn to_package(venv: &str, dir: &Path, metadata: Option<Metadata>) -> Package {
    let name = metadata.as_ref().map_or(venv, |m| m.main_package.package.as_str());
    let mut package = Package::new(ID, venv, name, Scope::User);
    if let Some(Metadata {
        main_package: main,
        python_version,
    }) = metadata
    {
        package.version = main.package_version;
        package.origin = python_version;
        let dependency_apps = main.apps_of_dependencies.iter().filter(|_| main.include_dependencies);
        package.commands = main
            .apps
            .iter()
            .chain(dependency_apps)
            .map(|app| format!("{app}{}", main.suffix))
            .collect();
    }
    package.kind = if package.commands.is_empty() {
        PackageKind::Unknown
    } else {
        PackageKind::App
    };
    package.reason = InstallReason::Explicit;
    package.location = Some(dir.to_path_buf());
    package
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const VENVS: &str = "/home/user/.local/share/pipx/venvs";

    /// Trimmed from a real pipx 1.7 venv.
    const ACTIVITY_SIM: &str = r#"{
    "injected_packages": {},
    "main_package": {
        "app_paths": [{"__Path__": "/home/user/.local/share/pipx/venvs/activity-sim/bin/activity-sim", "__type__": "Path"}],
        "apps": ["activity-sim"],
        "apps_of_dependencies": ["typer", "markdown-it", "pygmentize"],
        "include_apps": true,
        "include_dependencies": false,
        "package": "activity-sim",
        "package_or_url": "/home/user/Projects/activity-sim",
        "package_version": "0.1.0",
        "pinned": false,
        "pip_args": ["--editable"],
        "suffix": ""
    },
    "pipx_metadata_version": "0.5",
    "python_version": "Python 3.14.4",
    "source_interpreter": {"__Path__": "/usr/bin/python3.14", "__type__": "Path"},
    "venv_args": []
}"#;

    const BLACK_SUFFIXED: &str = r#"{
    "injected_packages": {"click": {"package": "click"}},
    "main_package": {
        "apps": ["black", "blackd"],
        "apps_of_dependencies": ["dmypy"],
        "include_dependencies": true,
        "package": "black",
        "package_version": "24.8.0",
        "suffix": "@24"
    },
    "pipx_metadata_version": "0.5",
    "python_version": "Python 3.11.5"
}"#;

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn reads_venv_metadata() {
        let fixture = Fixture::new();
        fixture
            .file(&format!("{VENVS}/activity-sim/pipx_metadata.json"), ACTIVITY_SIM)
            .file(&format!("{VENVS}/black@24/pipx_metadata.json"), BLACK_SUFFIXED);
        let packages = PipxSource.inventory(&fixture.host(FakeRunner::new()), &ScanContext::default()).unwrap();
        assert_eq!(packages.len(), 2);

        let sim = find(&packages, "activity-sim");
        assert_eq!(sim.name, "activity-sim");
        assert_eq!(sim.version.as_deref(), Some("0.1.0"));
        assert_eq!(sim.origin.as_deref(), Some("Python 3.14.4"));
        assert_eq!(sim.commands, ["activity-sim"]);
        assert_eq!(sim.kind, PackageKind::App);
        assert_eq!(sim.reason, InstallReason::Explicit);
        assert_eq!(sim.scope, Scope::User);
        assert_eq!(
            sim.location.as_deref(),
            Some(Path::new(VENVS).join("activity-sim").as_path())
        );

        let black = find(&packages, "black@24");
        assert_eq!(black.name, "black");
        assert_eq!(black.commands, ["black@24", "blackd@24", "dmypy@24"]);
    }

    #[test]
    fn pipx_home_overrides_and_legacy_location() {
        let fixture = Fixture::new();
        fixture
            .file("/opt/pipx/venvs/activity-sim/pipx_metadata.json", ACTIVITY_SIM)
            .file(
                "/home/user/.local/pipx/venvs/black@24/pipx_metadata.json",
                BLACK_SUFFIXED,
            );
        let host = fixture.host_with_vars(FakeRunner::new(), &[("PIPX_HOME", "/opt/pipx")]);
        let ids: Vec<_> = PipxSource.inventory(&host, &ScanContext::default()).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["activity-sim"]);

        let legacy: Vec<_> = PipxSource.inventory(&fixture.host(FakeRunner::new()), &ScanContext::default())
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(legacy, ["black@24"]);
    }

    #[test]
    fn venv_with_broken_metadata_is_still_listed() {
        let fixture = Fixture::new();
        fixture
            .file(&format!("{VENVS}/httpie/pipx_metadata.json"), "{\"main_package\": ")
            .dir(&format!("{VENVS}/ruff/bin"));
        let packages = PipxSource.inventory(&fixture.host(FakeRunner::new()), &ScanContext::default()).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["httpie", "ruff"]);
        assert_eq!(packages[0].name, "httpie");
        assert_eq!(packages[0].version, None);
        assert_eq!(packages[0].kind, PackageKind::Unknown);
    }

    #[test]
    fn probe_needs_the_venvs_directory() {
        let fixture = Fixture::new();
        assert!(matches!(
            PipxSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Unavailable { .. }
        ));
        fixture.dir(VENVS);
        assert_eq!(
            PipxSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Available { version: None }
        );
    }
}
