//! Global npm packages, read from each Node installation's `lib/node_modules`.

use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, FileType, Host, InstallReason, Package, PackageKind, Scope, Source, SourceError, SourceId,
    SourceInfo, SourceKind,
};
use serde_json::Value;

use crate::provider::{register, Context, LinuxSource};

pub const ID: SourceId = SourceId::new("npm");

/// Shipped with Node itself rather than installed by the user.
const BUNDLED: [&str; 2] = ["npm", "corepack"];

pub struct NpmSource;

impl LinuxSource for NpmSource {
    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(NpmSource);

impl Source for NpmSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("npm", "npm (global)", SourceKind::Language)
    }

    fn probe(&self, host: &Host) -> Availability {
        match roots(host) {
            Ok(roots) if roots.iter().any(|root| host.is_dir(&root.dir)) => Availability::Available { version: None },
            _ => Availability::unavailable("no global node_modules directory found"),
        }
    }

    fn inventory(&self, host: &Host) -> Result<Vec<Package>, SourceError> {
        let mut packages = Vec::new();
        for root in roots(host)? {
            for dir in package_dirs(host, &root.dir)? {
                if let Some(package) = read_package(host, &root, &dir)? {
                    packages.push(package);
                }
            }
        }
        Ok(packages)
    }
}

/// A global `node_modules` directory and the Node installation it belongs to.
struct Root {
    dir: PathBuf,
    /// Human-readable Node installation, e.g. "Node v22.18.0 (nvm)".
    label: String,
    /// Prefix that keeps package ids unique across installations, e.g. `nvm-v22.18.0`.
    slug: String,
}

impl Root {
    fn new(dir: impl Into<PathBuf>, label: impl Into<String>, slug: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            label: label.into(),
            slug: slug.into(),
        }
    }
}

/// Every place global packages may live, without duplicates.
fn roots(host: &Host) -> Result<Vec<Root>, SourceError> {
    let mut roots = vec![
        Root::new("/usr/lib/node_modules", "system Node", "system"),
        Root::new("/usr/local/lib/node_modules", "system Node", "usr-local"),
    ];
    if let Some(prefix) = host.var("NPM_CONFIG_PREFIX").filter(|p| p.starts_with('/')) {
        roots.push(Root::new(
            Path::new(prefix).join("lib/node_modules"),
            format!("npm prefix {prefix}"),
            "prefix",
        ));
    }
    let npm_global = host.home().join(".npm-global");
    roots.push(Root::new(
        npm_global.join("lib/node_modules"),
        format!("npm prefix {}", npm_global.display()),
        "npm-global",
    ));

    let nvm = host.dir_from_var("NVM_DIR", ".nvm").join("versions/node");
    for version in version_dirs(host, &nvm)? {
        roots.push(Root::new(
            nvm.join(&version).join("lib/node_modules"),
            format!("Node {version} (nvm)"),
            format!("nvm-{version}"),
        ));
    }
    let fnm = match host.var("FNM_DIR").filter(|dir| dir.starts_with('/')) {
        Some(dir) => PathBuf::from(dir),
        None => host.data_home().join("fnm"),
    }
    .join("node-versions");
    for version in version_dirs(host, &fnm)? {
        roots.push(Root::new(
            fnm.join(&version).join("installation/lib/node_modules"),
            format!("Node {version} (fnm)"),
            format!("fnm-{version}"),
        ));
    }

    let mut unique: Vec<Root> = Vec::with_capacity(roots.len());
    for root in roots {
        if !unique.iter().any(|seen| seen.dir == root.dir) {
            unique.push(root);
        }
    }
    Ok(unique)
}

/// Names of the per-version directories of a Node version manager.
fn version_dirs(host: &Host, dir: &Path) -> Result<Vec<String>, SourceError> {
    Ok(host
        .read_dir(dir)?
        .into_iter()
        .filter(|entry| is_visible_dir(&entry.name, entry.file_type))
        .map(|entry| entry.name)
        .collect())
}

/// Package directories in a `node_modules`, including `@scope/name` ones.
fn package_dirs(host: &Host, node_modules: &Path) -> Result<Vec<PathBuf>, SourceError> {
    let mut dirs = Vec::new();
    for entry in host.read_dir(node_modules)? {
        if !is_visible_dir(&entry.name, entry.file_type) {
            continue;
        }
        if entry.name.starts_with('@') {
            for scoped in host.read_dir(&entry.path)? {
                if is_visible_dir(&scoped.name, scoped.file_type) {
                    dirs.push(scoped.path);
                }
            }
        } else {
            dirs.push(entry.path);
        }
    }
    Ok(dirs)
}

/// Directories npm and version managers create for their own bookkeeping start with a dot (`.bin`, `.staging`).
fn is_visible_dir(name: &str, file_type: Option<FileType>) -> bool {
    !name.starts_with('.') && file_type == Some(FileType::Dir)
}

/// A package from its `package.json`; one without readable metadata is listed under its directory name.
fn read_package(host: &Host, root: &Root, dir: &Path) -> Result<Option<Package>, SourceError> {
    let manifest = host
        .read_optional(dir.join("package.json"))?
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let dir_name = dir_package_name(&root.dir, dir);
    let name = manifest
        .as_ref()
        .and_then(|m| m["name"].as_str())
        .unwrap_or(&dir_name)
        .to_string();
    if BUNDLED.contains(&name.as_str()) {
        return Ok(None);
    }
    let scope = if dir.starts_with(host.home()) {
        Scope::User
    } else {
        Scope::System
    };
    let mut package = Package::new(ID, format!("{}/{name}", root.slug), name.as_str(), scope);
    if let Some(manifest) = &manifest {
        package.version = manifest["version"].as_str().map(str::to_string);
        package.summary = manifest["description"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        package.commands = commands(&name, &manifest["bin"]);
    }
    package.kind = if package.commands.is_empty() {
        PackageKind::Library
    } else {
        PackageKind::App
    };
    package.reason = InstallReason::Explicit;
    package.origin = Some(root.label.clone());
    package.location = Some(dir.to_path_buf());
    Ok(Some(package))
}

/// `scope/name` relative to `node_modules`, written the way npm does (`@scope/name`).
fn dir_package_name(node_modules: &Path, dir: &Path) -> String {
    dir.strip_prefix(node_modules)
        .unwrap_or(dir)
        .to_string_lossy()
        .into_owned()
}

/// Commands from the `bin` field: a single path linked under the unscoped package name, or a name → path map.
fn commands(name: &str, bin: &Value) -> Vec<String> {
    match bin {
        Value::String(_) => vec![name.rsplit('/').next().unwrap_or(name).to_string()],
        Value::Object(map) => map.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};

    const NVM: &str = "/home/user/.nvm/versions/node";

    fn manifest(name: &str, version: &str, bin: &str) -> String {
        format!(r#"{{"name": "{name}", "version": "{version}", "description": "About {name}", "bin": {bin}}}"#)
    }

    fn nvm_fixture() -> Fixture {
        let fixture = Fixture::new();
        let v22 = format!("{NVM}/v22.18.0/lib/node_modules");
        let v24 = format!("{NVM}/v24.4.1/lib/node_modules");
        fixture
            .file(
                &format!("{v22}/typescript/package.json"),
                manifest(
                    "typescript",
                    "5.9.2",
                    r#"{"tsc": "./bin/tsc", "tsserver": "./bin/tsserver"}"#,
                ),
            )
            .file(
                &format!("{v22}/@anthropic-ai/claude-code/package.json"),
                manifest(
                    "@anthropic-ai/claude-code",
                    "2.1.266",
                    r#"{"claude": "bin/claude.exe"}"#,
                ),
            )
            .file(
                &format!("{v22}/@ionic/cli/package.json"),
                manifest("@ionic/cli", "7.2.1", r#""./bin/ionic""#),
            )
            .file(
                &format!("{v22}/npm/package.json"),
                manifest("npm", "10.9.3", r#"{"npm": "bin/npm-cli.js"}"#),
            )
            .file(
                &format!("{v22}/corepack/package.json"),
                manifest("corepack", "0.33.0", r#"{"corepack": "dist/corepack.js"}"#),
            )
            .file(&format!("{v22}/.package-lock.json"), "{}")
            .dir(&format!("{v22}/.bin"))
            .dir(&format!("{v22}/@openai"))
            .file(
                &format!("{v24}/typescript/package.json"),
                manifest("typescript", "5.8.3", r#"{"tsc": "./bin/tsc"}"#),
            )
            .file(
                &format!("{v24}/lodash/package.json"),
                r#"{"name": "lodash", "version": "4.17.21"}"#,
            );
        fixture
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("no package {id}"))
    }

    #[test]
    fn lists_packages_per_node_version_with_unique_ids() {
        let packages = NpmSource.inventory(&nvm_fixture().host(FakeRunner::new())).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "nvm-v22.18.0/@anthropic-ai/claude-code",
                "nvm-v22.18.0/@ionic/cli",
                "nvm-v22.18.0/typescript",
                "nvm-v24.4.1/lodash",
                "nvm-v24.4.1/typescript",
            ]
        );
        let ts = find(&packages, "nvm-v22.18.0/typescript");
        assert_eq!(ts.name, "typescript");
        assert_eq!(ts.version.as_deref(), Some("5.9.2"));
        assert_eq!(ts.summary.as_deref(), Some("About typescript"));
        assert_eq!(ts.origin.as_deref(), Some("Node v22.18.0 (nvm)"));
        assert_eq!(ts.scope, Scope::User);
        assert_eq!(ts.reason, InstallReason::Explicit);
        assert_eq!(
            ts.location.as_deref(),
            Some(Path::new(
                "/home/user/.nvm/versions/node/v22.18.0/lib/node_modules/typescript"
            ))
        );
        assert_eq!(
            find(&packages, "nvm-v24.4.1/typescript").version.as_deref(),
            Some("5.8.3")
        );
    }

    #[test]
    fn bin_field_becomes_commands_and_decides_kind() {
        let packages = NpmSource.inventory(&nvm_fixture().host(FakeRunner::new())).unwrap();
        let ts = find(&packages, "nvm-v22.18.0/typescript");
        assert_eq!(ts.commands, ["tsc", "tsserver"]);
        assert_eq!(ts.kind, PackageKind::App);
        let ionic = find(&packages, "nvm-v22.18.0/@ionic/cli");
        assert_eq!(ionic.name, "@ionic/cli");
        assert_eq!(ionic.commands, ["cli"]);
        let lodash = find(&packages, "nvm-v24.4.1/lodash");
        assert!(lodash.commands.is_empty());
        assert_eq!(lodash.kind, PackageKind::Library);
    }

    #[test]
    fn system_prefix_fnm_and_custom_nvm_dir() {
        let fixture = Fixture::new();
        fixture
            .file(
                "/usr/lib/node_modules/yarn/package.json",
                manifest("yarn", "1.22.22", r#"{"yarn": "bin/yarn.js"}"#),
            )
            .file(
                "/opt/npm/lib/node_modules/pnpm/package.json",
                manifest("pnpm", "10.15.0", r#"{"pnpm": "bin/pnpm.cjs"}"#),
            )
            .file(
                "/home/user/.local/share/fnm/node-versions/v20.19.4/installation/lib/node_modules/eslint/package.json",
                manifest("eslint", "9.34.0", r#"{"eslint": "./bin/eslint.js"}"#),
            )
            .file(
                "/srv/nvm/versions/node/v18.20.8/lib/node_modules/serve/package.json",
                manifest("serve", "14.2.4", r#"{"serve": "build/main.js"}"#),
            );
        let host = fixture.host_with_vars(
            FakeRunner::new(),
            &[("NPM_CONFIG_PREFIX", "/opt/npm"), ("NVM_DIR", "/srv/nvm")],
        );
        let packages = NpmSource.inventory(&host).unwrap();
        let yarn = find(&packages, "system/yarn");
        assert_eq!(yarn.scope, Scope::System);
        assert_eq!(yarn.origin.as_deref(), Some("system Node"));
        let pnpm = find(&packages, "prefix/pnpm");
        assert_eq!(pnpm.scope, Scope::System);
        assert_eq!(pnpm.origin.as_deref(), Some("npm prefix /opt/npm"));
        let eslint = find(&packages, "fnm-v20.19.4/eslint");
        assert_eq!(eslint.origin.as_deref(), Some("Node v20.19.4 (fnm)"));
        assert_eq!(eslint.scope, Scope::User);
        assert_eq!(find(&packages, "nvm-v18.20.8/serve").version.as_deref(), Some("14.2.4"));
        assert_eq!(packages.len(), 4);
    }

    #[test]
    fn prefix_pointing_at_a_known_root_is_scanned_once() {
        let fixture = Fixture::new();
        fixture.file(
            "/home/user/.npm-global/lib/node_modules/nodemon/package.json",
            manifest("nodemon", "3.1.10", r#"{"nodemon": "bin/nodemon.js"}"#),
        );
        let prefix = format!("{HOME}/.npm-global");
        let host = fixture.host_with_vars(FakeRunner::new(), &[("NPM_CONFIG_PREFIX", &prefix)]);
        let packages = NpmSource.inventory(&host).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["prefix/nodemon"]);
    }

    #[test]
    fn broken_manifest_is_listed_by_directory_name() {
        let fixture = Fixture::new();
        fixture
            .file("/usr/local/lib/node_modules/@acme/tool/package.json", "{ not json")
            .dir("/usr/local/lib/node_modules/half-installed");
        let packages = NpmSource.inventory(&fixture.host(FakeRunner::new())).unwrap();
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["usr-local/@acme/tool", "usr-local/half-installed"]);
        assert_eq!(packages[0].name, "@acme/tool");
        assert_eq!(packages[0].version, None);
    }

    #[test]
    fn probe_needs_a_global_directory() {
        let empty = Fixture::new();
        assert!(matches!(
            NpmSource.probe(&empty.host(FakeRunner::new())),
            Availability::Unavailable { .. }
        ));
        assert_eq!(
            NpmSource.probe(&nvm_fixture().host(FakeRunner::new())),
            Availability::Available { version: None }
        );
    }
}
