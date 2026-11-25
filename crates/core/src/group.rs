//! One app installed more than once: Sublime Merge as a `.deb` and as a snap, Claude Code through npm and by hand.
//! The installs stay separate packages; a group only says they are the same app, and why.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::model::{Package, PackageKind};

/// Installs of one app, from one source or several.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppGroup {
    /// What the app is called: the first install's title, or else its name.
    pub name: String,
    /// The installs' keys (`source:id`), in list order.
    pub packages: Vec<String>,
    /// What the installs have in common, sorted.
    pub shared: Vec<Shared>,
}

/// One thing two or more installs of a group have in common.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum Shared {
    /// The same AppStream ID, or a launcher id that equals one (`org.gnome.Papers`); compared without `.desktop` and
    /// case.
    AppId(String),
    /// The same name or title, compared as lowercase letters and digits (`sublimemerge`).
    Name(String),
    /// The same command on the `PATH` (`claude`).
    Command(String),
}

/// Groups the apps in `packages` (in list order) that share an AppStream ID, a launcher id, a name or a command.
/// Libraries, runtimes and components are left out: two packages of the same name there are parts, not installs.
pub(crate) fn group(packages: &[Package]) -> Vec<AppGroup> {
    let mut holders: HashMap<Shared, Vec<usize>> = HashMap::new();
    for (i, package) in packages.iter().enumerate() {
        if package.kind == PackageKind::App {
            for key in keys(package) {
                holders.entry(key).or_default().push(i);
            }
        }
    }

    // Union-find; a set's root is its smallest index, so sets come out in list order. A key links two packages only
    // when they can be two installs (see `installs_of_one_app`); `linking` keeps the keys that did.
    let mut parent: Vec<usize> = (0..packages.len()).collect();
    let mut linking: Vec<(Shared, usize)> = Vec::new();
    for (key, members) in &holders {
        for (n, &a) in members.iter().enumerate() {
            for &b in &members[n + 1..] {
                if installs_of_one_app(&packages[a], &packages[b]) {
                    let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                    parent[ra.max(rb)] = ra.min(rb);
                    linking.push((key.clone(), a));
                }
            }
        }
    }
    let roots: Vec<usize> = (0..packages.len()).map(|i| root(&mut parent, i)).collect();
    let mut sets: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, package) in packages.iter().enumerate() {
        if package.kind == PackageKind::App {
            sets.entry(roots[i]).or_default().push(i);
        }
    }

    sets.into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|(top, members)| {
            let shared: BTreeSet<Shared> = linking
                .iter()
                .filter(|(_, member)| roots[*member] == top)
                .map(|(key, _)| key.clone())
                .collect();
            let name = members
                .iter()
                .find_map(|&i| packages[i].title.clone())
                .unwrap_or_else(|| packages[members[0]].name.clone());
            AppGroup {
                name,
                packages: members.iter().map(|&i| packages[i].key()).collect(),
                shared: shared.into_iter().collect(),
            }
        })
        .collect()
}

/// Packages from different sources can be two installs of an app. Within one source, only packages of the same name
/// are (`typescript` under two Node versions); others are parts of one install (`vim` and `vim-common`).
fn installs_of_one_app(a: &Package, b: &Package) -> bool {
    a.source != b.source || a.name == b.name
}

/// What identifies an app across sources.
fn keys(package: &Package) -> BTreeSet<Shared> {
    let mut keys = BTreeSet::new();
    let ids = package
        .app_ids
        .iter()
        .map(String::as_str)
        .chain(package.launchers.iter().map(|launcher| launcher.id.as_str()));
    for id in ids {
        let id = id.strip_suffix(".desktop").unwrap_or(id).to_lowercase();
        if !id.is_empty() {
            keys.insert(Shared::AppId(id));
        }
    }
    for name in std::iter::once(&package.name).chain(&package.title) {
        let name: String = name
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        // Names that are only digits (`7zip` stays, `2048` does not) say nothing about the app.
        if name.chars().any(|c| c.is_ascii_alphabetic()) {
            keys.insert(Shared::Name(name));
        }
    }
    keys.extend(package.commands.iter().cloned().map(Shared::Command));
    keys
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Selection;
    use crate::model::{Launcher, Metainfo, SourceId};
    use crate::stub::{Stub, housekeeper_with, package};
    use crate::testing::{Fixture, HOME};
    use std::path::PathBuf;

    fn launcher(id: &str, name: &str, path: &str) -> Launcher {
        Launcher {
            id: id.into(),
            name: name.into(),
            icon: None,
            path: PathBuf::from(path),
            shadows: Vec::new(),
            program: None,
            hidden: false,
        }
    }

    fn app(source: &'static str, id: &str, name: &str, title: Option<&str>, commands: &[&str]) -> Package {
        let mut package = package(source, name, None, commands);
        package.source = SourceId::new(source);
        package.id = id.into();
        package.kind = PackageKind::App;
        package.title = title.map(String::from);
        package
    }

    /// `(name, packages)` of each group.
    fn summary(groups: &[AppGroup]) -> Vec<(&str, Vec<&str>)> {
        groups
            .iter()
            .map(|g| (g.name.as_str(), g.packages.iter().map(String::as_str).collect()))
            .collect()
    }

    #[test]
    fn installs_of_one_app_are_grouped_and_parts_of_one_install_are_not() {
        let fixture = Fixture::new();
        let mut libc = package("dpkg", "libc6", None, &[]);
        libc.kind = PackageKind::Library;
        let dpkg = Stub {
            id: "dpkg",
            packages: vec![
                package("dpkg", "papers", None, &[]),
                package("dpkg", "sublime-merge", None, &["sublime_merge"]),
                package("dpkg", "vim", None, &["vim.basic"]),
                package("dpkg", "vim-common", None, &[]),
                libc,
            ],
            files: vec![
                ("/usr/share/applications/org.gnome.Papers.desktop", "papers"),
                ("/usr/share/metainfo/org.gnome.Papers.metainfo.xml", "papers"),
                ("/usr/share/applications/sublime_merge.desktop", "sublime-merge"),
                ("/usr/share/applications/vim.desktop", "vim-common"),
            ],
            ..Stub::default()
        };
        let mut papers = app(
            "flatpak",
            "system/app/org.gnome.Papers",
            "org.gnome.Papers",
            Some("Papers"),
            &[],
        );
        papers.app_ids = vec!["org.gnome.Papers".into()];
        let mut libc_flatpak = app("flatpak", "runtime/libc6", "libc6", None, &[]);
        libc_flatpak.kind = PackageKind::Runtime;
        let flatpak = Stub {
            id: "flatpak",
            packages: vec![papers, libc_flatpak],
            ..Stub::default()
        };
        let snap = Stub {
            id: "snap",
            packages: vec![app(
                "snap",
                "sublime-merge",
                "sublime-merge",
                Some("Sublime Merge"),
                &[],
            )],
            ..Stub::default()
        };
        let npm = Stub {
            id: "npm",
            packages: vec![
                app("npm", "v20/typescript", "typescript", None, &["tsc", "tsserver"]),
                app("npm", "v22/typescript", "typescript", None, &["tsc", "tsserver"]),
                app(
                    "npm",
                    "v22/@anthropic-ai/claude-code",
                    "@anthropic-ai/claude-code",
                    None,
                    &["claude"],
                ),
            ],
            ..Stub::default()
        };
        let manual = Stub {
            id: "manual",
            packages: vec![app("manual", "/opt/claude", "claude", None, &["claude"])],
            ..Stub::default()
        };
        let launchers = vec![
            launcher(
                "org.gnome.Papers.desktop",
                "Document Viewer",
                "/usr/share/applications/org.gnome.Papers.desktop",
            ),
            launcher(
                "sublime_merge.desktop",
                "Sublime Merge",
                "/usr/share/applications/sublime_merge.desktop",
            ),
            launcher("vim.desktop", "Vim", "/usr/share/applications/vim.desktop"),
        ];
        let metainfo = vec![Metainfo {
            id: "org.gnome.Papers".into(),
            path: PathBuf::from("/usr/share/metainfo/org.gnome.Papers.metainfo.xml"),
            launchables: Vec::new(),
        }];
        let hk = housekeeper_with(
            &fixture,
            "/usr/bin",
            vec![dpkg, flatpak, snap, npm, manual],
            launchers,
            metainfo,
        );
        let scan = hk.scan(&Selection::All);

        assert_eq!(
            summary(&scan.groups),
            [
                (
                    "@anthropic-ai/claude-code",
                    vec!["npm:v22/@anthropic-ai/claude-code", "manual:/opt/claude"]
                ),
                (
                    "Document Viewer",
                    vec!["dpkg:papers", "flatpak:system/app/org.gnome.Papers"]
                ),
                ("Sublime Merge", vec!["dpkg:sublime-merge", "snap:sublime-merge"]),
                ("typescript", vec!["npm:v20/typescript", "npm:v22/typescript"]),
            ]
        );
        let shared = |name: &str| &scan.groups.iter().find(|g| g.name == name).unwrap().shared;
        assert_eq!(
            shared("Document Viewer"),
            &[Shared::AppId("org.gnome.papers".into()), Shared::Name("papers".into())],
            "the catalog entry the .deb installs, and the Flatpak's title equal to the .deb's name"
        );
        assert_eq!(shared("@anthropic-ai/claude-code"), &[Shared::Command("claude".into())]);
        assert_eq!(
            shared("typescript"),
            &[
                Shared::Name("typescript".into()),
                Shared::Command("tsc".into()),
                Shared::Command("tsserver".into()),
            ]
        );
    }

    #[test]
    fn a_catalog_entry_reaches_its_package_through_the_launcher_it_names() {
        let fixture = Fixture::new();
        let dpkg = Stub {
            id: "dpkg",
            packages: vec![package("dpkg", "gimp", None, &[])],
            files: vec![("/usr/share/applications/gimp.desktop", "gimp")],
            ..Stub::default()
        };
        let mut gimp = app(
            "flatpak",
            "system/app/org.gimp.GIMP",
            "org.gimp.GIMP",
            Some("GNU Image Manipulation Program"),
            &[],
        );
        gimp.app_ids = vec!["org.gimp.GIMP".into()];
        let flatpak = Stub {
            id: "flatpak",
            packages: vec![gimp],
            ..Stub::default()
        };
        // Nobody installed this copy; it names the launcher the .deb did.
        let metainfo = vec![Metainfo {
            id: "org.gimp.GIMP".into(),
            path: PathBuf::from(format!("{HOME}/.local/share/metainfo/org.gimp.GIMP.metainfo.xml")),
            launchables: vec!["gimp.desktop".into()],
        }];
        let launchers = vec![launcher("gimp.desktop", "GIMP", "/usr/share/applications/gimp.desktop")];
        let hk = housekeeper_with(&fixture, "/usr/bin", vec![dpkg, flatpak], launchers, metainfo);
        let scan = hk.scan(&Selection::All);

        assert_eq!(
            summary(&scan.groups),
            [("GIMP", vec!["dpkg:gimp", "flatpak:system/app/org.gimp.GIMP"])]
        );
    }
}
