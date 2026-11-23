//! Python tools installed with `uv tool install`, read from each tool's `uv-receipt.toml` and venv.

use std::path::{Path, PathBuf};

use housekeeper_core::text::key_values;
use housekeeper_core::{
    Availability, FileType, Host, InstallReason, Package, PackageKind, Scope, Source, SourceError, SourceId,
    SourceInfo, SourceKind,
};
use serde::Deserialize;

use super::pip::{normalize_name, site_packages, split_dist_info};
use crate::provider::{Context, LinuxSource};

pub const ID: SourceId = SourceId::new("uv");

pub struct UvToolSource;

impl LinuxSource for UvToolSource {
    fn create(_: &Context<'_>) -> Self {
        Self
    }
}

impl Source for UvToolSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("uv", "uv tools", SourceKind::Language)
    }

    fn probe(&self, host: &Host) -> Availability {
        if host.is_dir(tools_dir(host)) {
            Availability::Available { version: None }
        } else {
            Availability::unavailable("no uv tools directory found")
        }
    }

    fn inventory(&self, host: &Host) -> Result<Vec<Package>, SourceError> {
        let mut packages = Vec::new();
        for entry in host.read_dir(tools_dir(host))? {
            if entry.file_type == Some(FileType::Dir) && !entry.name.starts_with('.') {
                packages.push(read_tool(host, &entry.name, &entry.path)?);
            }
        }
        Ok(packages)
    }
}

/// `$UV_TOOL_DIR`, defaulting to `$XDG_DATA_HOME/uv/tools`.
fn tools_dir(host: &Host) -> PathBuf {
    match host.var("UV_TOOL_DIR").filter(|dir| dir.starts_with('/')) {
        Some(dir) => PathBuf::from(dir),
        None => host.data_home().join("uv/tools"),
    }
}

#[derive(Deserialize)]
struct Receipt {
    tool: Tool,
}

#[derive(Deserialize)]
struct Tool {
    /// The tool's own package and any `--with` extras.
    #[serde(default)]
    requirements: Vec<Requirement>,
    #[serde(default)]
    entrypoints: Vec<Entrypoint>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Requirement {
    Table { name: String },
    Specifier(String),
}

impl Requirement {
    fn name(&self) -> &str {
        match self {
            Self::Table { name } => name,
            Self::Specifier(spec) => {
                let end = spec
                    .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
                    .unwrap_or(spec.len());
                &spec[..end]
            }
        }
    }
}

#[derive(Deserialize)]
struct Entrypoint {
    name: String,
}

/// The tool directory is named after its package; a missing or broken receipt leaves only that.
fn read_tool(host: &Host, dir_name: &str, dir: &Path) -> Result<Package, SourceError> {
    let receipt = host
        .read_optional(dir.join("uv-receipt.toml"))?
        .and_then(|text| toml::from_str::<Receipt>(&text).ok());
    let normalized = normalize_name(dir_name);
    let name = receipt
        .as_ref()
        .and_then(|r| {
            r.tool
                .requirements
                .iter()
                .find(|req| normalize_name(req.name()) == normalized)
                .or(r.tool.requirements.first())
        })
        .map_or(dir_name, Requirement::name);
    let mut package = Package::new(ID, dir_name, name, Scope::User);
    package.version = installed_version(host, dir, &normalize_name(name))?;
    package.commands = receipt
        .map(|r| r.tool.entrypoints.into_iter().map(|e| e.name).collect())
        .unwrap_or_default();
    package.kind = if package.commands.is_empty() {
        PackageKind::Unknown
    } else {
        PackageKind::App
    };
    package.reason = InstallReason::Explicit;
    package.origin = host
        .read_optional(dir.join("pyvenv.cfg"))?
        .and_then(|cfg| {
            key_values(&cfg)
                .find(|(key, _)| *key == "version_info")
                .map(|(_, value)| value)
        })
        .map(|version| format!("Python {version}"));
    package.location = Some(dir.to_path_buf());
    Ok(package)
}

/// Version from the package's `.dist-info` directory in the tool's venv.
fn installed_version(host: &Host, venv: &Path, normalized: &str) -> Result<Option<String>, SourceError> {
    for (_, site) in site_packages(host, venv)? {
        for entry in host.read_dir(&site)? {
            if let Some((name, version)) = split_dist_info(&entry.name) {
                if normalize_name(name) == normalized {
                    return Ok(Some(version.to_string()));
                }
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const TOOLS: &str = "/home/user/.local/share/uv/tools";

    const GRAPHIFYY_RECEIPT: &str = r#"[tool]
requirements = [{ name = "graphifyy" }]
entrypoints = [
    { name = "graphify", install-path = "/home/user/.local/bin/graphify", from = "graphifyy" },
    { name = "graphify-mcp", install-path = "/home/user/.local/bin/graphify-mcp", from = "graphifyy" },
]
"#;

    const RUAMEL_RECEIPT: &str = r#"[tool]
requirements = [
    { name = "mypy-extensions" },
    { name = "Ruamel.Yaml.Cmd", specifier = ">=0.6" },
]
python = "3.12"
entrypoints = [
    { name = "yaml", install-path = "/home/user/.local/bin/yaml", from = "ruamel-yaml-cmd" },
]

[tool.options]
exclude-newer-package = {}
"#;

    const PYVENV: &str = "home = /home/user/.local/share/uv/python/cpython-3.13-linux-x86_64-gnu/bin\n\
implementation = CPython\nuv = 0.11.9\nversion_info = 3.13.13\ninclude-system-site-packages = false\n";

    fn fixture() -> Fixture {
        let fixture = Fixture::new();
        fixture
            .file(&format!("{TOOLS}/graphifyy/uv-receipt.toml"), GRAPHIFYY_RECEIPT)
            .file(&format!("{TOOLS}/graphifyy/pyvenv.cfg"), PYVENV)
            .dir(&format!("{TOOLS}/graphifyy/lib/python3.13/site-packages/graphify"))
            .dir(&format!(
                "{TOOLS}/graphifyy/lib/python3.13/site-packages/graphifyy-0.9.100.dist-info"
            ))
            .dir(&format!(
                "{TOOLS}/graphifyy/lib/python3.13/site-packages/networkx-3.5.dist-info"
            ))
            .file(&format!("{TOOLS}/ruamel-yaml-cmd/uv-receipt.toml"), RUAMEL_RECEIPT)
            .dir(&format!(
                "{TOOLS}/ruamel-yaml-cmd/lib/python3.12/site-packages/mypy_extensions-1.1.0.dist-info"
            ))
            .dir(&format!(
                "{TOOLS}/ruamel-yaml-cmd/lib/python3.12/site-packages/ruamel.yaml.cmd-0.6.5.dist-info"
            ))
            .file(&format!("{TOOLS}/.lock"), "");
        fixture
    }

    #[test]
    fn reads_receipts_and_venv_versions() {
        let packages = UvToolSource.inventory(&fixture().host(FakeRunner::new())).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["graphifyy", "ruamel-yaml-cmd"]);

        let graphifyy = &packages[0];
        assert_eq!(graphifyy.name, "graphifyy");
        assert_eq!(graphifyy.version.as_deref(), Some("0.9.100"));
        assert_eq!(graphifyy.commands, ["graphify", "graphify-mcp"]);
        assert_eq!(graphifyy.kind, PackageKind::App);
        assert_eq!(graphifyy.reason, InstallReason::Explicit);
        assert_eq!(graphifyy.scope, Scope::User);
        assert_eq!(graphifyy.origin.as_deref(), Some("Python 3.13.13"));
        assert_eq!(
            graphifyy.location.as_deref(),
            Some(Path::new(TOOLS).join("graphifyy").as_path())
        );
    }

    #[test]
    fn main_requirement_is_matched_by_normalized_name() {
        let packages = UvToolSource.inventory(&fixture().host(FakeRunner::new())).unwrap();
        let ruamel = &packages[1];
        assert_eq!(ruamel.name, "Ruamel.Yaml.Cmd");
        assert_eq!(ruamel.version.as_deref(), Some("0.6.5"));
        assert_eq!(ruamel.commands, ["yaml"]);
        assert_eq!(ruamel.origin, None);
    }

    #[test]
    fn tool_dir_override_and_broken_receipt() {
        let fixture = Fixture::new();
        fixture
            .file("/opt/uv-tools/ruff/uv-receipt.toml", "[tool\nrequirements = ")
            .dir("/opt/uv-tools/ruff/lib/python3.12/site-packages/ruff-0.12.11.dist-info")
            .file(&format!("{TOOLS}/graphifyy/uv-receipt.toml"), GRAPHIFYY_RECEIPT);
        let host = fixture.host_with_vars(FakeRunner::new(), &[("UV_TOOL_DIR", "/opt/uv-tools")]);
        let packages = UvToolSource.inventory(&host).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].id, "ruff");
        assert_eq!(packages[0].version.as_deref(), Some("0.12.11"));
        assert!(packages[0].commands.is_empty());
        assert_eq!(packages[0].kind, PackageKind::Unknown);
    }

    #[test]
    fn plain_string_requirements() {
        let receipt: Receipt = toml::from_str("[tool]\nrequirements = [\"black[d]>=24.1\"]\n").unwrap();
        assert_eq!(receipt.tool.requirements[0].name(), "black");
    }

    #[test]
    fn probe_needs_the_tools_directory() {
        assert!(matches!(
            UvToolSource.probe(&Fixture::new().host(FakeRunner::new())),
            Availability::Unavailable { .. }
        ));
        assert_eq!(
            UvToolSource.probe(&fixture().host(FakeRunner::new())),
            Availability::Available { version: None }
        );
    }
}
