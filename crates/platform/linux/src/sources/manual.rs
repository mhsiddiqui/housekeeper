//! Software installed by hand or by a script: directories in `/opt`, loose executables on the PATH and the
//! script-installed tools the recipes describe (rustup, nvm, pyenv…; this crate's `recipes/`, decision D26).

use std::collections::{BTreeMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, Entry, FilePackage, FileType, Host, InstallReason, Package, PackageKind, RemovalPlan, Remover,
    ScanContext, Source, SourceError, SourceId, SourceInfo, SourceKind,
};

use crate::files::{is_appimage, is_appimage_head, looks_like_version, resolve_symlinks, scope_of};
use crate::formats::{elf_machine, is_compressed, is_tar, nothing_inside};
use crate::provider::{Context, FileFormat, LinuxSource, register};
use crate::recipe::{Recipe, recipes};

pub const ID: SourceId = SourceId::new("manual");

const OPT: &str = "/opt";
const USR_LOCAL_BIN: &str = "/usr/local/bin";

/// Directories of loose executables in the home directory.
const HOME_BIN_DIRS: &[&str] = &[".local/bin", "bin"];

/// Files here belong to the operating system; a symlink to them is not an install of its own.
const SYSTEM_PREFIXES: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/etc"];

/// Directory names that say nothing about what is installed, skipped when naming the install a symlink points into.
const GENERIC_DIRS: &[&str] = &["bin", "sbin", "libexec", "versions", "current", "latest"];

/// How much of an executable to read when checking for a pip launcher.
const SCRIPT_HEAD_LEN: usize = 1024;

/// What no other source accounts for. Package managers' owner lookups and every source's install roots, both reached
/// through the [`ScanContext`], decide what is left out.
pub struct ManualSource;

impl LinuxSource for ManualSource {
    /// What someone unpacks or runs by hand; what it installs is listed here, if anywhere.
    const FILES: &'static [FileFormat] = &[
        FileFormat {
            id: "archive",
            title: "Archive",
            suffixes: &[
                ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.zst", ".tzst", ".tar.bz2", ".tbz2", ".tar", ".zip",
            ],
            matches: is_archive,
            evidence: "a tar archive (compressed or not) or a zip, by its name and first bytes",
            read: nothing_inside,
        },
        FileFormat {
            id: "script",
            title: "Script",
            suffixes: &[],
            matches: is_script,
            evidence: "a script (`#!` on its first line), whatever its name",
            read: nothing_inside,
        },
        FileFormat {
            id: "program",
            title: "Installer program",
            suffixes: &[".run", ".bin"],
            matches: is_program,
            evidence: "an ELF program named `.run` or `.bin`",
            read: read_program,
        },
    ];

    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(ManualSource);

fn is_archive(head: &[u8]) -> bool {
    is_compressed(head) || is_tar(head) || head.starts_with(b"PK\x03\x04")
}

fn is_script(head: &[u8]) -> bool {
    head.starts_with(b"#!")
}

/// An ELF program that is not an AppImage (those are the AppImage source's).
fn is_program(head: &[u8]) -> bool {
    head.starts_with(b"\x7fELF") && !is_appimage_head(head)
}

/// The processor its ELF header names.
fn read_program(_: &Host, _: &Path, head: &[u8]) -> Result<Option<FilePackage>, SourceError> {
    Ok(Some(FilePackage {
        arch: elf_machine(head),
        ..FilePackage::default()
    }))
}

impl Source for ManualSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("manual", "Installed by hand or script", SourceKind::Manual).evidence(
            "its files on disk (in `/opt`, `/usr/local/bin`, `~/.local/bin` or a known script installer's folder), \
             which no package manager or installer claims",
        )
    }

    fn probe(&self, _host: &Host) -> Availability {
        Availability::Available { version: None }
    }

    fn inventory(&self, host: &Host, scan: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let home = host.home();
        let bin_dirs: Vec<PathBuf> = std::iter::once(PathBuf::from(USR_LOCAL_BIN))
            .chain(HOME_BIN_DIRS.iter().map(|dir| home.join(dir)))
            .collect();
        let opt_dirs = opt_dirs(host)?;
        let mut executables: Vec<Entry> = Vec::new();
        for dir in &bin_dirs {
            executables.extend(
                host.read_dir(dir)?
                    .into_iter()
                    .filter(|entry| entry.file_type == Some(FileType::File { executable: true })),
            );
        }

        let asked: Vec<PathBuf> = opt_dirs
            .iter()
            .map(|(dir, _)| dir.clone())
            .chain(
                executables
                    .iter()
                    .filter(|entry| entry.path.starts_with(USR_LOCAL_BIN))
                    .map(|entry| entry.path.clone()),
            )
            .collect();
        let owned = owned_paths(host, scan, &asked);

        let mut items: BTreeMap<String, Package> = BTreeMap::new();
        let installers = detect_installers(host, scan);
        for (_, package) in &installers {
            items.insert(package.id.clone(), package.clone());
        }
        for (dir, has_program) in opt_dirs.into_iter().filter(|(dir, _)| !owned.contains(dir)) {
            let mut package = dir_package(host, &dir);
            if has_program {
                package.kind = PackageKind::App;
            }
            items.insert(package.id.clone(), package);
        }
        let executables: Vec<Entry> = executables
            .into_iter()
            .filter(|entry| !owned.contains(&entry.path) && !is_appimage(host, &entry.path))
            .collect();
        for entry in executables.iter().filter(|entry| !entry.is_symlink) {
            if !is_pip_script(host, &entry.path) {
                let package = file_package(host, &entry.path, &entry.name);
                items.insert(package.id.clone(), package);
            }
        }

        let managed: Vec<PathBuf> = scan.sources().flat_map(|source| source.install_roots(host)).collect();
        for link in executables.iter().filter(|entry| entry.is_symlink) {
            let target = resolve_symlinks(host, &link.path);
            let command = link.name.clone();
            if let Some((_, installer)) = installers.iter().find(|(root, _)| target.starts_with(root)) {
                add_command(&mut items, &installer.id, command);
                continue;
            }
            if managed.iter().any(|root| target.starts_with(root)) {
                continue;
            }
            let target_id = target.to_string_lossy().into_owned();
            if items.contains_key(&target_id) {
                // An alias of a listed executable (`beads -> bd`).
                add_command(&mut items, &target_id, command);
                continue;
            }
            if target.parent().is_some_and(|dir| bin_dirs.iter().any(|bin| bin == dir)) {
                // Points at an executable excluded above (native-owned, pip launcher).
                continue;
            }
            if let Some(opt_dir) = opt_dir_of(&target) {
                // Merges into the /opt item; native-owned or unreadable /opt directories stay out.
                add_command(&mut items, &opt_dir.to_string_lossy(), command);
                continue;
            }
            if is_system_path(&target) {
                continue;
            }
            match install_root(home, &target) {
                Some(root) => {
                    let id = root.to_string_lossy().into_owned();
                    items.entry(id.clone()).or_insert_with(|| dir_package(host, &root));
                    add_command(&mut items, &id, command);
                }
                None => {
                    let mut package =
                        file_package(host, &target, &target.file_name().unwrap_or_default().to_string_lossy());
                    package.commands = vec![command];
                    items.entry(package.id.clone()).or_insert(package);
                }
            }
        }

        let mut packages: Vec<Package> = items.into_values().collect();
        for package in &mut packages {
            package.commands.sort();
            package.commands.dedup();
            if !package.commands.is_empty() {
                package.kind = PackageKind::App;
            }
        }
        Ok(packages)
    }

    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }
}

/// Script-installed tools are removed as their recipe says; hand installs have nothing recording how.
impl Remover for ManualSource {
    fn plan(&self, host: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        let recipe = package
            .id
            .strip_prefix(INSTALLER_PREFIX)
            .and_then(|key| recipes().iter().flatten().find(|recipe| recipe.key == key));
        Ok(match (recipe, &package.location) {
            (Some(recipe), Some(root)) => recipe.plan(host, root),
            _ => RemovalPlan::unknown(),
        })
    }
}

/// Directories in `/opt` the user can list, with whether they hold a program at the top or in `bin/`. Unlistable
/// ones are service state (containerd keeps `/opt/containerd` mode 711), not something the user runs.
fn opt_dirs(host: &Host) -> Result<Vec<(PathBuf, bool)>, SourceError> {
    let mut dirs = Vec::new();
    for entry in host.read_dir(OPT)? {
        if entry.file_type != Some(FileType::Dir) {
            continue;
        }
        let Ok(children) = host.read_dir(&entry.path) else {
            continue;
        };
        let has_program = children.iter().any(|child| match child.file_type {
            Some(FileType::File { executable }) => executable,
            Some(FileType::Dir) => child.name == "bin",
            _ => false,
        });
        dirs.push((entry.path, has_program));
    }
    Ok(dirs)
}

/// Prefix of a script-installed tool's id: `installer/rustup`.
const INSTALLER_PREFIX: &str = "installer/";

/// The recipes' tools that are installed, with their install folder. A recipe that cannot be read is a warning.
fn detect_installers(host: &Host, scan: &ScanContext<'_>) -> Vec<(PathBuf, Package)> {
    let mut found = Vec::new();
    for recipe in recipes() {
        let recipe: &Recipe = match recipe {
            Ok(recipe) => recipe,
            Err(error) => {
                scan.warn(format!("a script-installer recipe could not be read: {error}"));
                continue;
            }
        };
        if let Some(root) = recipe.detect(host) {
            let mut package = Package::new(
                ID,
                format!("{INSTALLER_PREFIX}{}", recipe.key),
                recipe.key,
                scope_of(host, &root),
            );
            package.title = Some(recipe.title.clone());
            package.kind = PackageKind::App;
            package.reason = InstallReason::Explicit;
            package.location = Some(root.clone());
            found.push((root, package));
        }
    }
    found
}

/// The subset of `asked` that an installed package owns, from every source that can tell. A source that fails to
/// answer gets a warning, and all of `asked` counts as owned: hiding a few hand installs beats listing system files.
fn owned_paths(host: &Host, scan: &ScanContext<'_>, asked: &[PathBuf]) -> HashSet<PathBuf> {
    let mut owned = HashSet::new();
    for source in scan.sources() {
        let Some(lookup) = source.owner_lookup() else {
            continue;
        };
        match lookup.owners(host, asked) {
            Ok(owners) => owned.extend(owners.into_keys()),
            Err(error) => {
                scan.warn(format!(
                    "could not ask {} which entries of {OPT} and {USR_LOCAL_BIN} it installed, so they are left \
                     out: {error}",
                    source.info().label
                ));
                owned.extend(asked.iter().cloned());
            }
        }
    }
    owned
}

/// A console-script launcher pip or uv wrote for an installed Python package.
///
/// Recognised by its generated text: a Python shebang (on the first line, or on the second for the `#!/bin/sh` +
/// `'''exec'` form used for long or spaced interpreter paths) and the `-script.pyw` argv fix-up every pip, distlib and
/// uv launcher template contains. Unlike `RECORD` files this works wherever the package went (user site, a pyenv
/// interpreter, `/usr/local`) without walking site-packages directories.
fn is_pip_script(host: &Host, path: &Path) -> bool {
    let Ok(head) = host.read_head(path, SCRIPT_HEAD_LEN) else {
        return false;
    };
    let head = String::from_utf8_lossy(&head);
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let python =
        first.starts_with("#!") && (first.contains("python") || lines.next().is_some_and(|l| l.contains("python")));
    python && head.contains("sys.exit(") && (head.contains("-script.pyw") || head.contains("-script\\.pyw"))
}

/// `/opt/<dir>` for anything inside it.
fn opt_dir_of(path: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(OPT).ok()?;
    let dir = rest.components().next()?;
    (rest.components().count() > 1).then(|| Path::new(OPT).join(dir))
}

fn is_system_path(path: &Path) -> bool {
    !path.starts_with("/usr/local") && SYSTEM_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// The directory an install lives in, from one of its files: the file's directory, skipping generic and
/// version-named levels (`~/pg/bin/psql` → `~/pg`, `~/.local/share/claude/versions/2.1` → `~/.local/share/claude`).
/// `None` when that would be the home directory or a top-level directory.
fn install_root(home: &Path, target: &Path) -> Option<PathBuf> {
    let mut root = target.parent()?;
    while root != home && is_generic_dir(root) {
        root = root.parent()?;
    }
    (root != home && !home.starts_with(root) && root.components().count() > 2).then(|| root.to_path_buf())
}

fn is_generic_dir(dir: &Path) -> bool {
    dir.file_name()
        .map(|name| name.to_string_lossy())
        .is_some_and(|name| GENERIC_DIRS.contains(&name.as_ref()) || looks_like_version(&name))
}

fn add_command(items: &mut BTreeMap<String, Package>, id: &str, command: String) {
    if let Some(package) = items.get_mut(id) {
        package.commands.push(command);
    }
}

fn new_package(host: &Host, path: &Path, name: &str) -> Package {
    let mut package = Package::new(ID, path.to_string_lossy(), name, scope_of(host, path));
    package.reason = InstallReason::Explicit;
    package.location = Some(path.to_path_buf());
    package
}

/// A directory install; `App` once something on the PATH points into it.
fn dir_package(host: &Host, dir: &Path) -> Package {
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    new_package(host, dir, &name)
}

fn file_package(host: &Host, path: &Path, name: &str) -> Package {
    let mut package = new_package(host, path, name);
    package.kind = PackageKind::App;
    package.commands.push(name.to_string());
    if let Some(meta) = host.metadata(path) {
        package.size_bytes = Some(meta.len());
        package.installed_at = Some(meta.mtime());
    }
    package
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::cargo::CargoSource;
    use crate::sources::dpkg::DpkgSource;
    use crate::sources::npm::NpmSource;
    use crate::sources::pipx::PipxSource;
    use crate::sources::snap::SnapSource;
    use crate::sources::uv::UvToolSource;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};
    use housekeeper_core::{Actor, Output, Scope, Step, Tier};
    use std::sync::Arc;

    const ELF: &[u8] = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    const APPIMAGE: &[u8] = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00";

    /// What pip (distlib) writes for `[console_scripts]`.
    const PIP_LAUNCHER: &str = "\
#!/home/user/.pyenv/versions/3.11.5/bin/python3.11
# -*- coding: utf-8 -*-
import re
import sys
from coverage.cmdline import main
if __name__ == '__main__':
    sys.argv[0] = re.sub(r'(-script\\.pyw|\\.exe)?$', '', sys.argv[0])
    sys.exit(main())
";

    /// What uv and newer pip write, with the `/bin/sh` trampoline used for interpreter paths containing spaces.
    const UV_LAUNCHER: &str = "\
#!/bin/sh
'''exec' '/home/user/My Envs/py/bin/python' \"$0\" \"$@\"
' '''
# -*- coding: utf-8 -*-
import sys
from fontTools.__main__ import main
if __name__ == \"__main__\":
    if sys.argv[0].endswith(\"-script.pyw\"):
        sys.argv[0] = sys.argv[0][:-11]
    elif sys.argv[0].endswith(\".exe\"):
        sys.argv[0] = sys.argv[0][:-4]
    sys.exit(main())
";

    fn home(path: &str) -> String {
        format!("{HOME}/{path}")
    }

    /// The manual source's packages and warnings, with `others` as the platform's other sources.
    fn scan_with(host: &Host, others: Vec<Arc<dyn Source>>) -> (Vec<Package>, Vec<String>) {
        let context = ScanContext::new(&others);
        let packages = ManualSource.inventory(host, &context).unwrap();
        (packages, context.into_warnings())
    }

    fn scan(fixture: &Fixture, runner: FakeRunner, others: Vec<Arc<dyn Source>>) -> Vec<Package> {
        scan_with(&fixture.host(runner), others).0
    }

    fn ids(packages: &[Package]) -> Vec<&str> {
        packages.iter().map(|p| p.id.as_str()).collect()
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("{id} missing from {:?}", ids(packages)))
    }

    #[test]
    fn opt_dirs_owned_by_packages_are_excluded() {
        let fixture = Fixture::new();
        fixture.executable("/opt/brave.com/brave/brave", ELF);
        fixture.executable("/opt/linear-linux-0.2.1/linear.AppImage", APPIMAGE);
        fixture.file("/opt/sdk/README", "docs");
        fixture.executable("/usr/local/bin/ngrok", ELF);
        fixture.executable("/usr/local/bin/mailhog", ELF);
        fixture.symlink("/usr/local/bin/brave", "/opt/brave.com/brave/brave");
        fixture.executable("/opt/tool/bin/tool", ELF);
        fixture.symlink("/usr/local/bin/tool", "/opt/tool/bin/tool");
        let runner = FakeRunner::new().respond_with(
            "dpkg-query --search /opt/brave.com /opt/linear-linux-0.2.1 /opt/sdk /opt/tool \
             /usr/local/bin/brave /usr/local/bin/mailhog /usr/local/bin/ngrok /usr/local/bin/tool",
            Output {
                code: Some(1),
                stdout: "brave-browser: /opt/brave.com\nbrave-browser: /usr/local/bin/brave\n\
                         ngrok: /usr/local/bin/ngrok\n"
                    .into(),
                stderr: "dpkg-query: no path found matching pattern /opt/sdk\n".into(),
            },
        );
        let packages = scan(&fixture, runner, vec![Arc::new(DpkgSource::default())]);
        assert_eq!(
            ids(&packages),
            [
                "/opt/linear-linux-0.2.1",
                "/opt/sdk",
                "/opt/tool",
                "/usr/local/bin/mailhog"
            ]
        );
        let linear = find(&packages, "/opt/linear-linux-0.2.1");
        assert_eq!(linear.name, "linear-linux-0.2.1");
        assert_eq!(linear.scope, Scope::System);
        assert_eq!(linear.kind, PackageKind::App);
        assert_eq!(linear.location.as_deref(), Some(Path::new("/opt/linear-linux-0.2.1")));
        assert_eq!(find(&packages, "/opt/sdk").kind, PackageKind::Unknown);
        assert_eq!(find(&packages, "/opt/tool").commands, ["tool"]);
        let mailhog = find(&packages, "/usr/local/bin/mailhog");
        assert_eq!(mailhog.size_bytes, Some(ELF.len() as u64));
        assert_eq!(mailhog.commands, ["mailhog"]);
        assert_eq!(mailhog.reason, InstallReason::Explicit);
    }

    #[test]
    fn without_a_native_manager_nothing_is_asked() {
        let fixture = Fixture::new();
        fixture.dir("/opt/brave.com");
        fixture.executable("/usr/local/bin/ngrok", ELF);
        let packages = scan(&fixture, FakeRunner::new(), Vec::new());
        assert_eq!(ids(&packages), ["/opt/brave.com", "/usr/local/bin/ngrok"]);
    }

    #[test]
    fn a_failed_owner_lookup_hides_what_it_could_not_check_and_warns() {
        let fixture = Fixture::new();
        fixture.dir("/opt/brave.com");
        fixture.executable("/usr/local/bin/ngrok", ELF);
        fixture.executable(&home(".local/bin/mine"), ELF);
        let runner = FakeRunner::new().fail(
            "dpkg-query --search /opt/brave.com /usr/local/bin/ngrok",
            "did not finish within 60 s",
        );
        let (packages, warnings) = scan_with(&fixture.host(runner), vec![Arc::new(DpkgSource::default())]);
        assert_eq!(ids(&packages), ["/home/user/.local/bin/mine"]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("did not finish"), "{warnings:?}");
    }

    #[test]
    fn symlink_farms_become_one_item_per_install() {
        let fixture = Fixture::new();
        for tool in ["psql", "pg_dump", "createdb"] {
            fixture.executable(&home(&format!("Softwares/pgsql18-home/bin/{tool}")), ELF);
            fixture.symlink(
                &home(&format!(".local/bin/{tool}")),
                &format!("{HOME}/Softwares/pgsql18-home/bin/{tool}"),
            );
        }
        fixture.executable(&home(".local/zed.app/bin/zed"), ELF);
        fixture.symlink(&home(".local/bin/zed"), "../zed.app/bin/zed");
        fixture.executable(&home(".local/share/claude/versions/2.1.329"), ELF);
        fixture.symlink(
            &home(".local/bin/claude"),
            &home(".local/share/claude/versions/2.1.329"),
        );
        fixture.executable("/usr/bin/buzz", ELF);
        fixture.symlink(&home(".local/bin/buzz"), "/usr/bin/buzz");
        fixture.executable(&home(".local/bin/bd"), ELF);
        fixture.symlink(&home(".local/bin/beads"), "bd");
        fixture.executable(&home("loose.sh"), "#!/bin/sh\n");
        fixture.symlink(&home("bin/loose"), &home("loose.sh"));
        fixture.symlink(&home(".local/bin/dangling"), &home("gone/bin/x"));
        let packages = scan(&fixture, FakeRunner::new(), Vec::new());
        assert_eq!(
            ids(&packages),
            [
                "/home/user/.local/bin/bd",
                "/home/user/.local/share/claude",
                "/home/user/.local/zed.app",
                "/home/user/Softwares/pgsql18-home",
                "/home/user/loose.sh",
            ]
        );
        let pg = find(&packages, "/home/user/Softwares/pgsql18-home");
        assert_eq!(pg.name, "pgsql18-home");
        assert_eq!(pg.commands, ["createdb", "pg_dump", "psql"]);
        assert_eq!(pg.scope, Scope::User);
        assert_eq!(pg.kind, PackageKind::App);
        assert_eq!(pg.size_bytes, None);
        assert_eq!(find(&packages, "/home/user/.local/zed.app").name, "zed.app");
        assert_eq!(find(&packages, "/home/user/.local/share/claude").commands, ["claude"]);
        assert_eq!(find(&packages, "/home/user/.local/bin/bd").commands, ["bd", "beads"]);
        assert_eq!(find(&packages, "/home/user/loose.sh").commands, ["loose"]);
    }

    #[test]
    fn executables_other_sources_list_are_skipped() {
        let fixture = Fixture::new();
        let pipx = home(".local/share/pipx/venvs/black/bin/black");
        let uv = home(".local/share/uv/tools/ruff/bin/ruff");
        let cargo = home(".cargo/bin/rg");
        let npm = "/usr/local/lib/node_modules/typescript/bin/tsc";
        for target in [&pipx, &uv, &cargo, npm] {
            fixture.executable(target, ELF);
        }
        fixture.symlink(&home(".local/bin/black"), &pipx);
        fixture.symlink(&home(".local/bin/ruff"), &uv);
        fixture.symlink(&home("bin/rg"), &cargo);
        fixture.symlink("/usr/local/bin/tsc", npm);
        fixture.executable("/snap/bin/firefox", ELF);
        fixture.symlink(&home("bin/firefox"), "/snap/bin/firefox");
        fixture.executable(&home(".local/bin/coverage"), PIP_LAUNCHER);
        fixture.executable(&home(".local/bin/fonttools"), UV_LAUNCHER);
        fixture.symlink(&home("bin/cov"), &home(".local/bin/coverage"));
        fixture.executable(&home(".local/bin/Obsidian.AppImage"), APPIMAGE);
        fixture.executable(
            &home(".local/bin/hand-written.py"),
            "#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n",
        );
        fixture.file(&home(".local/bin/notes.txt"), "not executable");
        let others: Vec<Arc<dyn Source>> = vec![
            Arc::new(PipxSource),
            Arc::new(UvToolSource),
            Arc::new(CargoSource),
            Arc::new(NpmSource),
            Arc::new(SnapSource),
        ];
        let packages = scan(&fixture, FakeRunner::new(), others);
        assert_eq!(ids(&packages), ["/home/user/.local/bin/hand-written.py"]);

        // The sources' install roots are what keeps them out.
        let unclaimed = scan(&fixture, FakeRunner::new(), Vec::new());
        for id in [
            "/home/user/.local/share/pipx/venvs/black",
            "/home/user/.local/share/uv/tools/ruff",
            "/home/user/.cargo",
            "/usr/local/lib/node_modules/typescript",
            "/snap/bin/firefox",
        ] {
            assert!(ids(&unclaimed).contains(&id), "{id} missing from {:?}", ids(&unclaimed));
        }
    }

    #[test]
    fn script_installers_are_detected_and_absorb_their_links() {
        let fixture = Fixture::new();
        fixture.executable("/srv/node/nvm.sh", "# nvm");
        fixture.executable("/srv/node/versions/node/v22.18.0/bin/node", ELF);
        fixture.symlink("/usr/local/bin/node", "/srv/node/versions/node/v22.18.0/bin/node");
        fixture.dir(&home(".rustup/toolchains"));
        fixture.executable(&home(".cargo/bin/rustup"), ELF);
        fixture.executable(&home(".pyenv/bin/pyenv"), "#!/bin/bash\n");
        fixture.executable(&home(".pyenv/shims/python"), "#!/bin/bash\n");
        fixture.symlink(&home(".local/bin/python"), &home(".pyenv/shims/python"));
        fixture.executable("/home/linuxbrew/.linuxbrew/bin/brew", "#!/bin/bash\n");
        // Default locations without their marker are not installs.
        fixture.dir(&home(".nvm"));
        fixture.dir(&home(".bun"));
        let host = fixture.host_with_vars(FakeRunner::new(), &[("NVM_DIR", "/srv/node")]);
        // npm claims the nvm directory too; the nvm installer still gets the commands that are nvm's own.
        let (packages, _) = scan_with(&host, vec![Arc::new(NpmSource)]);
        assert_eq!(
            ids(&packages),
            [
                "installer/homebrew",
                "installer/nvm",
                "installer/pyenv",
                "installer/rustup"
            ]
        );
        let nvm = find(&packages, "installer/nvm");
        assert_eq!(nvm.title.as_deref(), Some("Node Version Manager (nvm)"));
        assert_eq!(nvm.location.as_deref(), Some(Path::new("/srv/node")));
        assert_eq!(nvm.commands, ["node"]);
        assert_eq!(find(&packages, "installer/pyenv").commands, ["python"]);
        assert_eq!(find(&packages, "installer/pyenv").scope, Scope::User);
        assert_eq!(find(&packages, "installer/homebrew").scope, Scope::System);
        assert_eq!(
            find(&packages, "installer/rustup").location.as_deref(),
            Some(Path::new("/home/user/.rustup"))
        );
    }

    #[test]
    fn install_roots_never_climb_to_home() {
        let home = Path::new(HOME);
        assert_eq!(install_root(home, Path::new("/home/user/bin/x")), None);
        assert_eq!(install_root(home, Path::new("/home/user/1.2/x")), None);
        assert_eq!(install_root(home, Path::new("/srv/x")), None);
        assert_eq!(
            install_root(home, Path::new("/srv/tools/go/1.23/bin/go")),
            Some(PathBuf::from("/srv/tools/go"))
        );
    }

    #[test]
    fn nothing_installed_is_empty() {
        assert!(scan(&Fixture::new(), FakeRunner::new(), Vec::new()).is_empty());
    }

    #[test]
    fn rustup_plans_its_own_uninstaller_from_cargo_home() {
        let fixture = Fixture::new();
        fixture.dir(&home(".rustup/toolchains"));
        fixture.executable("/srv/cargo/bin/rustup", ELF);
        let host = fixture.host_with_vars(FakeRunner::new(), &[("CARGO_HOME", "/srv/cargo")]);
        let (packages, _) = scan_with(&host, Vec::new());
        let plan = ManualSource.plan(&host, find(&packages, "installer/rustup")).unwrap();
        assert_eq!(plan.tier, Tier::OwnUninstaller);
        assert_eq!(
            plan.steps,
            [Step::run(
                Actor::User,
                ["/srv/cargo/bin/rustup", "self", "uninstall", "-y"]
            )]
        );
    }

    #[test]
    fn hand_installs_have_no_known_removal() {
        let fixture = Fixture::new();
        fixture.executable("/opt/tool/tool", ELF);
        let host = fixture.host(FakeRunner::new());
        let (packages, _) = scan_with(&host, Vec::new());
        let plan = ManualSource.plan(&host, find(&packages, "/opt/tool")).unwrap();
        assert_eq!(plan, RemovalPlan::unknown());
    }
}
