//! What each package is to a person: the launchers and commands it provides, and from them its kind, title and icon.
//!
//! Launchers and commands are attached with the same evidence `who` uses ([`Ownership`]), so no source is named here
//! and a new source is covered as soon as it reports install locations or install folders.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::host::{FileType, Host};
use crate::inventory::{SourceReport, SourceStatus};
use crate::model::{Launcher, Package, PackageKind, SourceId, SourceInfo};
use crate::ownership::{Ownership, all_paths, chain};
use crate::platform::Platform;
use crate::source::{ScanContext, Source, SourceError};
use crate::who::{Evidence, Owner};

/// Launchers and commands, with what the package databases said about their files. Gathering needs no packages, so
/// it runs while the sources are being read.
pub(crate) struct Gathered<'a> {
    /// Each launcher with the chains that may tell its owner, in order: its file, the files it hides, its program.
    launchers: Vec<(Launcher, Vec<Vec<PathBuf>>)>,
    /// Each command's name and chain.
    commands: Vec<(String, Vec<PathBuf>)>,
    ownership: Ownership<'a>,
    failures: Vec<(SourceInfo, SourceError)>,
    warnings: Vec<String>,
}

/// Reads the launchers and the commands, and asks the package databases of the `selected` sources who owns them.
pub(crate) fn gather<'a>(host: &Host, platform: &'a dyn Platform, selected: &[&'a dyn Source]) -> Gathered<'a> {
    let context = ScanContext::new(platform.sources());
    let launchers: Vec<(Launcher, Vec<Vec<PathBuf>>)> = platform
        .launchers(host, &context)
        .into_iter()
        .map(|launcher| {
            let chains = std::iter::once(&launcher.path)
                .chain(&launcher.shadows)
                .chain(&launcher.program)
                .map(|path| chain(host, path))
                .collect();
            (launcher, chains)
        })
        .collect();
    let databases: Vec<&dyn Source> = selected
        .iter()
        .copied()
        .filter(|source| source.owner_lookup().is_some())
        .collect();
    // Only sources whose package databases list files learn their commands here; the others report their own.
    let commands = if databases.is_empty() {
        Vec::new()
    } else {
        commands(host, platform, &context)
    };
    let paths = all_paths(
        launchers
            .iter()
            .flat_map(|(_, chains)| chains)
            .chain(commands.iter().map(|(_, hops)| hops)),
    );
    let mut failures = Vec::new();
    let ownership = Ownership::ask(host, platform.sources(), &databases, &paths, &mut failures);
    Gathered {
        launchers,
        commands,
        ownership,
        failures,
        warnings: context.into_warnings(),
    }
}

/// Every program in the platform's command directories, each folder read once (`/bin` is `/usr/bin` when it links
/// there), with its chain of symlinks. Programs that lead to one file stay apart: every `/snap/bin` command is a link
/// to `/usr/bin/snap`, and each is its own snap's command.
fn commands(host: &Host, platform: &dyn Platform, context: &ScanContext<'_>) -> Vec<(String, Vec<PathBuf>)> {
    let mut seen = HashSet::new();
    let mut commands = Vec::new();
    for dir in platform.command_dirs(host) {
        // A `PATH` can name files and missing folders; only folders that cannot be read are worth a warning.
        if host.file_type(&dir) != Some(FileType::Dir) || !seen.insert(host.canonical(&dir)) {
            continue;
        }
        let entries = match host.read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                context.warn(error.to_string());
                continue;
            }
        };
        for entry in entries {
            if entry.file_type == Some(FileType::File { executable: true }) {
                commands.push((entry.name, chain(host, &entry.path)));
            }
        }
    }
    commands
}

impl Gathered<'_> {
    /// Attaches launchers and commands to `packages` and derives kinds, titles and icons. A package database that
    /// could not answer becomes a warning of its source; problems reading launchers go to `warnings`.
    pub fn apply(self, packages: &mut [Package], reports: &mut [SourceReport], warnings: &mut Vec<String>) {
        warnings.extend(self.warnings);
        for (info, error) in self.failures {
            let status = reports
                .iter_mut()
                .find(|report| report.info.id == info.id)
                .map(|report| &mut report.status);
            if let Some(SourceStatus::Ok { warnings, .. }) = status {
                warnings.push(format!(
                    "could not tell which packages provide launchers and commands: {error}"
                ));
            }
        }

        let index: HashMap<(&SourceId, &str), usize> = packages
            .iter()
            .enumerate()
            .map(|(i, package)| ((&package.source, package.id.as_str()), i))
            .collect();
        let mut launchers: HashMap<usize, Vec<Launcher>> = HashMap::new();
        for (launcher, chains) in self.launchers {
            // The first chain a source accounts for decides, even when it cannot name the package: a launcher in a
            // source's folder is that source's, whatever program its command line starts.
            let owner = chains
                .iter()
                .find_map(|hops| self.ownership.owner(hops, packages))
                .and_then(|owner| owner.package)
                .and_then(|package| index.get(&(&package.source, package.id.as_str())).copied());
            if let Some(i) = owner {
                launchers.entry(i).or_default().push(launcher);
            }
        }
        // A command counts only when a package database lists its file: other sources report their own commands,
        // and a link in another source's folder (`/snap/bin/firefox` → `/usr/bin/snap`) is not the database's.
        let mut commands: HashMap<usize, Vec<String>> = HashMap::new();
        for (name, hops) in self.commands {
            let Some(Owner {
                package: Some(package),
                evidence: Evidence::ListedFile { .. },
                ..
            }) = self.ownership.owner(&hops, packages)
            else {
                continue;
            };
            if let Some(&i) = index.get(&(&package.source, package.id.as_str())) {
                commands.entry(i).or_default().push(name);
            }
        }

        let files_known: Vec<bool> = packages
            .iter()
            .map(|package| self.ownership.answered(&package.source))
            .collect();
        for (i, package) in packages.iter_mut().enumerate() {
            if let Some(mut found) = launchers.remove(&i) {
                found.sort_by(|a, b| a.id.cmp(&b.id));
                package.launchers = found;
            }
            if let Some(found) = commands.remove(&i) {
                package.commands.extend(found);
                package.commands.sort();
                package.commands.dedup();
            }
            describe(package, files_known[i]);
        }
    }
}

/// Kind, title and icon from what the package provides. `files_known`: its package database answered, so a package
/// with no command and no visible launcher is known to have none.
fn describe(package: &mut Package, files_known: bool) {
    let visible: Vec<&Launcher> = package.launchers.iter().filter(|launcher| !launcher.hidden).collect();
    if package.kind == PackageKind::Unknown {
        if !visible.is_empty() || !package.commands.is_empty() {
            package.kind = PackageKind::App;
        } else if files_known {
            package.kind = PackageKind::Component;
        }
    }
    // The main launcher: the only one shown in menus, or the one named after the package (`brave-browser.desktop`
    // among the web apps that run in it).
    let main = match visible.as_slice() {
        [one] => Some(*one),
        _ => visible.iter().copied().find(|launcher| {
            Path::new(&launcher.id)
                .file_stem()
                .is_some_and(|stem| stem == package.name.as_str() || stem == package.id.as_str())
        }),
    };
    if let Some(main) = main {
        let (title, icon) = (main.name.clone(), main.icon.clone());
        package.title.get_or_insert(title);
        if package.icon.is_none() {
            package.icon = icon;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Scan, Selection};
    use crate::stub::{Stub, housekeeper_with_launchers, package};
    use crate::testing::{Fixture, HOME};

    fn launcher(id: &str, name: &str, path: &str, program: Option<&str>) -> Launcher {
        Launcher {
            id: id.into(),
            name: name.into(),
            icon: Some(id.trim_end_matches(".desktop").into()),
            path: PathBuf::from(path),
            shadows: Vec::new(),
            program: program.map(PathBuf::from),
            hidden: false,
        }
    }

    fn find<'a>(scan: &'a Scan, source: &str, name: &str) -> &'a Package {
        scan.packages
            .iter()
            .find(|p| p.source.as_str() == source && p.name == name)
            .unwrap_or_else(|| panic!("{source}:{name} missing"))
    }

    fn launcher_ids(package: &Package) -> Vec<&str> {
        package.launchers.iter().map(|l| l.id.as_str()).collect()
    }

    #[test]
    fn native_packages_are_apps_when_they_have_a_launcher_or_a_command_and_components_otherwise() {
        let fixture = Fixture::new();
        fixture.file("/usr/share/applications/gimp.desktop", "");
        fixture.file("/usr/share/applications/brave-browser.desktop", "");
        let web_app = format!("{HOME}/.local/share/applications/brave-whatsapp.desktop");
        fixture.file(&web_app, "");
        fixture.executable("/usr/bin/gimp", "");
        fixture.executable("/usr/bin/git", "");
        fixture.executable("/opt/brave.com/brave/brave-browser", "");
        // Merged /usr: /bin/git is /usr/bin/git and must count once.
        fixture.symlink("/bin", "usr/bin");
        let mut libc = package("dpkg", "libc6", None, &[]);
        libc.kind = PackageKind::Library;
        let dpkg = Stub {
            id: "dpkg",
            packages: vec![
                package("dpkg", "gimp", None, &[]),
                package("dpkg", "git", None, &[]),
                package("dpkg", "brave-browser", None, &[]),
                package("dpkg", "fonts-noto-core", None, &[]),
                libc,
            ],
            files: vec![
                ("/usr/share/applications/gimp.desktop", "gimp"),
                ("/usr/bin/gimp", "gimp"),
                ("/usr/bin/git", "git"),
                ("/usr/share/applications/brave-browser.desktop", "brave-browser"),
                ("/opt/brave.com/brave/brave-browser", "brave-browser"),
                ("/usr/share/fonts/noto/NotoSans.ttf", "fonts-noto-core"),
            ],
            ..Stub::default()
        };
        let broken = Stub {
            id: "rpm",
            packages: vec![package("rpm", "mystery", None, &[])],
            lookup_fails: true,
            ..Stub::default()
        };
        let launchers = vec![
            launcher(
                "gimp.desktop",
                "GNU Image Manipulation Program",
                "/usr/share/applications/gimp.desktop",
                Some("/usr/bin/gimp"),
            ),
            launcher(
                "brave-browser.desktop",
                "Brave Web Browser",
                "/usr/share/applications/brave-browser.desktop",
                Some("/opt/brave.com/brave/brave-browser"),
            ),
            // A web app the browser made: the user's own file, attached through the program it starts.
            launcher(
                "brave-whatsapp.desktop",
                "WhatsApp Web",
                &web_app,
                Some("/opt/brave.com/brave/brave-browser"),
            ),
        ];
        let hk = housekeeper_with_launchers(&fixture, "/usr/bin:/bin", vec![dpkg, broken], launchers);
        let scan = hk.scan(&Selection::All);

        let gimp = find(&scan, "dpkg", "gimp");
        assert_eq!(gimp.kind, PackageKind::App);
        assert_eq!(gimp.display_name(), "GNU Image Manipulation Program");
        assert_eq!(gimp.icon.as_deref(), Some("gimp"));
        assert_eq!(gimp.commands, ["gimp"]);
        assert_eq!(launcher_ids(gimp), ["gimp.desktop"]);

        let git = find(&scan, "dpkg", "git");
        assert_eq!(
            (git.kind, git.commands.as_slice()),
            (PackageKind::App, ["git".to_string()].as_slice())
        );
        assert_eq!(git.title, None);

        let brave = find(&scan, "dpkg", "brave-browser");
        assert_eq!(launcher_ids(brave), ["brave-browser.desktop", "brave-whatsapp.desktop"]);
        assert_eq!(
            brave.display_name(),
            "Brave Web Browser",
            "the launcher named after the package"
        );
        assert_eq!(brave.icon.as_deref(), Some("brave-browser"));

        assert_eq!(find(&scan, "dpkg", "fonts-noto-core").kind, PackageKind::Component);
        assert_eq!(find(&scan, "dpkg", "libc6").kind, PackageKind::Library);

        // A database that cannot answer says nothing about its packages, and says why.
        assert_eq!(find(&scan, "rpm", "mystery").kind, PackageKind::Unknown);
        let rpm = scan.sources.iter().find(|r| r.info.id.as_str() == "rpm").unwrap();
        assert!(
            matches!(&rpm.status, SourceStatus::Ok { warnings, .. } if warnings.len() == 1),
            "{:?}",
            rpm.status
        );
    }

    #[test]
    fn launchers_go_to_the_install_they_come_from_not_the_program_that_runs_them() {
        let fixture = Fixture::new();
        // A user's copy of Godot's entry hides the one Flatpak exports; both start /usr/bin/flatpak (a dpkg file).
        let copy = format!("{HOME}/.local/share/applications/org.godotengine.Godot.desktop");
        fixture.file(&copy, "");
        let exported = "/var/lib/flatpak/exports/share/applications/org.godotengine.Godot.desktop";
        let app = "/var/lib/flatpak/app/org.godotengine.Godot";
        fixture.file(
            &format!("{app}/current/active/export/share/applications/org.godotengine.Godot.desktop"),
            "",
        );
        fixture.symlink(
            exported,
            "../../../app/org.godotengine.Godot/current/active/export/share/applications/org.godotengine.Godot.desktop",
        );
        fixture.executable("/usr/bin/flatpak", "");
        // Snap entries start /snap/bin/pinta, a link to snapd's binary (`snapd` is also the snapd snap's name).
        let snap_entry = "/var/lib/snapd/desktop/applications/pinta_pinta.desktop";
        fixture.file(snap_entry, "");
        fixture.executable("/usr/lib/snapd/snapd", "");
        fixture.symlink("/usr/bin/snap", "../lib/snapd/snapd");
        fixture.symlink("/snap/bin/pinta", "/usr/bin/snap");

        let dpkg = Stub {
            id: "dpkg",
            packages: vec![
                package("dpkg", "flatpak", None, &[]),
                package("dpkg", "snapd", None, &[]),
            ],
            files: vec![
                ("/usr/bin/flatpak", "flatpak"),
                ("/usr/bin/snap", "snapd"),
                ("/usr/lib/snapd/snapd", "snapd"),
            ],
            ..Stub::default()
        };
        let mut godot = package("flatpak", "org.godotengine.Godot", Some(app), &[]);
        godot.kind = PackageKind::App;
        godot.title = Some("Godot Engine".into());
        let flatpak = Stub {
            id: "flatpak",
            packages: vec![godot],
            roots: vec!["/var/lib/flatpak/exports"],
            ..Stub::default()
        };
        let snap = Stub {
            id: "snap",
            packages: vec![
                package("snap", "snapd", Some("/snap/snapd/current"), &[]),
                package("snap", "pinta", Some("/snap/pinta/current"), &["pinta"]),
            ],
            roots: vec!["/snap"],
            ..Stub::default()
        };
        let mut godot_launcher = launcher(
            "org.godotengine.Godot.desktop",
            "Godot (my copy)",
            &copy,
            Some("/usr/bin/flatpak"),
        );
        godot_launcher.shadows = vec![PathBuf::from(exported)];
        let launchers = vec![
            godot_launcher,
            launcher("pinta_pinta.desktop", "Pinta", snap_entry, Some("/snap/bin/pinta")),
        ];
        let hk = housekeeper_with_launchers(&fixture, "/snap/bin:/usr/bin", vec![dpkg, flatpak, snap], launchers);
        let scan = hk.scan(&Selection::All);

        let godot = find(&scan, "flatpak", "org.godotengine.Godot");
        assert_eq!(launcher_ids(godot), ["org.godotengine.Godot.desktop"]);
        assert_eq!(godot.display_name(), "Godot Engine", "the source's own title stays");
        assert_eq!(launcher_ids(find(&scan, "snap", "pinta")), ["pinta_pinta.desktop"]);
        assert!(find(&scan, "snap", "snapd").launchers.is_empty());

        let flatpak_tool = find(&scan, "dpkg", "flatpak");
        assert!(flatpak_tool.launchers.is_empty());
        assert_eq!(flatpak_tool.commands, ["flatpak"]);
        let snapd = find(&scan, "dpkg", "snapd");
        assert!(snapd.launchers.is_empty());
        assert_eq!(
            snapd.commands,
            ["snap"],
            "/snap/bin/pinta is the snap's command, not snapd's"
        );
    }
}
