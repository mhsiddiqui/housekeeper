//! Leftovers: what an installed item leaves behind outside its own files: settings, caches and data in the folders the
//! platform and the item's source keep per-user state in, and lines in shell startup files that load it (decisions
//! D7, D36). Found only; nothing here changes anything.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::host::{FileType, Host};
use crate::model::{Package, SourceId, SourceKind};
use crate::platform::Platform;

/// What an item leaves behind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leftovers {
    /// Folders named after the item in the per-user state folders, largest first.
    pub folders: Vec<Leftover>,
    /// Lines in shell startup files that mention the item's install folder.
    pub shell_lines: Vec<ShellLine>,
}

/// A folder named after the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leftover {
    pub path: PathBuf,
    pub kind: LeftoverKind,
    /// Size of the files in it (symlinks are not followed).
    pub size_bytes: u64,
    /// Other installed items (`source:id`) that use the same folder and whose names match it too: it may still be
    /// theirs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_used_by: Vec<String>,
    /// The item's source also installs copies Housekeeper does not read (a language package inside a project or a
    /// virtual environment), which may use the folder too.
    #[serde(default)]
    pub unseen_copies: bool,
}

/// What a leftover folder holds. Only caches can go without asking; the rest is the user's (D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LeftoverKind {
    /// Settings (`~/.config/<name>`).
    Config,
    /// Rebuilt when needed (`~/.cache/<name>`).
    Cache,
    /// What the item stored for the user (`~/.local/share/<name>`, a Flatpak's `~/.var/app/<id>`).
    Data,
    /// History and logs (`~/.local/state/<name>`).
    State,
}

impl LeftoverKind {
    /// Whether the folder holds the user's own things, which are only removed when the user asks.
    pub fn is_user_data(self) -> bool {
        self != Self::Cache
    }
}

/// A line in a shell startup file that mentions the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellLine {
    pub file: PathBuf,
    /// 1-based.
    pub line: usize,
    pub text: String,
}

/// A folder leftovers are looked for in.
struct Root {
    dir: PathBuf,
    kind: LeftoverKind,
    /// The source whose items alone keep their state here; `None` for the platform's folders, which every item outside
    /// a sandbox uses.
    source: Option<SourceId>,
}

impl Root {
    fn used_by(&self, package: &Package) -> bool {
        match &self.source {
            None => !package.sandboxed,
            Some(source) => package.source == *source,
        }
    }
}

/// What `package` (one of `installed`) leaves behind on `platform`.
pub(crate) fn find(platform: &dyn Platform, host: &Host, package: &Package, installed: &[Package]) -> Leftovers {
    let own = names(package);
    let shared_folders = platform.shared_state_folders();
    let install_roots: Vec<PathBuf> = platform
        .sources()
        .iter()
        .flat_map(|source| source.install_roots(host))
        .collect();
    // Folders that hold installs (a package's location, a source's install root) are installs, not leftovers.
    let install_dirs: Vec<&Path> = installed
        .iter()
        .filter_map(|p| p.location.as_deref())
        .chain(install_roots.iter().map(PathBuf::as_path))
        .collect();
    let unseen_copies = platform.sources().iter().any(|source| {
        let info = source.info();
        info.id == package.source && info.kind == SourceKind::Language
    });
    let roots = platform
        .leftover_roots(host)
        .into_iter()
        .map(|(dir, kind)| Root {
            dir,
            kind,
            source: None,
        })
        .chain(platform.sources().iter().flat_map(|source| {
            let id = source.info().id;
            source.leftover_roots(host).into_iter().map(move |(dir, kind)| Root {
                dir,
                kind,
                source: Some(id.clone()),
            })
        }));
    let mut folders = Vec::new();
    for root in roots.filter(|root| root.used_by(package)) {
        let Ok(entries) = host.read_dir(&root.dir) else {
            continue;
        };
        for entry in entries {
            let name = entry.name.to_lowercase();
            if entry.file_type != Some(FileType::Dir)
                || entry.is_symlink
                || !own.contains(&name)
                || shared_folders.contains(&name.as_str())
                || install_dirs.iter().any(|dir| dir.starts_with(&entry.path))
                || package.location.as_ref().is_some_and(|own| entry.path.starts_with(own))
            {
                continue;
            }
            let also_used_by = installed
                .iter()
                .filter(|other| other.key() != package.key() && root.used_by(other) && names(other).contains(&name))
                .map(Package::key)
                .collect();
            folders.push(Leftover {
                size_bytes: size(host, &entry.path),
                path: entry.path,
                kind: root.kind,
                also_used_by,
                unseen_copies,
            });
        }
    }
    folders.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then_with(|| a.path.cmp(&b.path)));
    Leftovers {
        folders,
        shell_lines: shell_lines(platform, host, package, installed, &install_roots),
    }
}

/// The names a folder of the item may carry, lowercase: its name, its AppStream IDs and its launchers' IDs.
fn names(package: &Package) -> HashSet<String> {
    std::iter::once(package.name.as_str())
        .chain(package.app_ids.iter().map(String::as_str))
        .chain(
            package
                .launchers
                .iter()
                .map(|launcher| launcher.id.strip_suffix(".desktop").unwrap_or(&launcher.id)),
        )
        .filter(|name| name.len() > 1)
        .map(str::to_lowercase)
        .collect()
}

/// Lines that mention the item's own install folder, for items installed in a folder of their own in the home folder
/// (a script-installed tool, a hand install). A folder inside a source's install root (an npm package under nvm, a
/// cargo binary in `~/.cargo/bin`) or shared with another item belongs to the source, so its lines are not the item's.
fn shell_lines(
    platform: &dyn Platform,
    host: &Host,
    package: &Package,
    installed: &[Package],
    roots: &[PathBuf],
) -> Vec<ShellLine> {
    let home = host.home();
    let Some(location) = package.location.as_deref() else {
        return Vec::new();
    };
    let Ok(relative) = location.strip_prefix(home) else {
        return Vec::new();
    };
    let shared = roots.iter().any(|root| root != location && location.starts_with(root))
        || installed
            .iter()
            .any(|other| other.key() != package.key() && other.location.as_deref() == Some(location));
    if relative.as_os_str().is_empty() || shared {
        return Vec::new();
    }
    let relative = relative.to_string_lossy();
    let forms = [
        location.to_string_lossy().into_owned(),
        format!("$HOME/{relative}"),
        format!("${{HOME}}/{relative}"),
        format!("~/{relative}"),
    ];
    let mut lines = Vec::new();
    for file in platform.shell_files(host) {
        let Ok(Some(text)) = host.read_optional(&file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if forms.iter().any(|form| mentions(line, form)) {
                lines.push(ShellLine {
                    file: file.clone(),
                    line: n + 1,
                    text: line.trim().to_string(),
                });
            }
        }
    }
    lines
}

/// `line` names `path` itself or something inside it, not a longer name that starts or ends the same (`~/.nvmrc`,
/// `/mnt/backup/home/u/.nvm`).
fn mentions(line: &str, path: &str) -> bool {
    let part_of_name = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.');
    line.match_indices(path).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + path.len()..].chars().next();
        !before.is_some_and(|c| part_of_name(c) || matches!(c, '/' | '~' | '$')) && !after.is_some_and(part_of_name)
    })
}

/// Size of the files under `dir`, not following symlinks; folders that cannot be read count as empty.
fn size(host: &Host, dir: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = host.read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            match entry.file_type {
                _ if entry.is_symlink => {}
                Some(FileType::Dir) => pending.push(entry.path),
                Some(FileType::File { .. }) => total += host.metadata(&entry.path).map_or(0, |meta| meta.len()),
                _ => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::show::Lookup;
    use crate::stub::{Stub, housekeeper, package};
    use crate::testing::{Fixture, HOME};

    fn leftovers(hk: &crate::inventory::Housekeeper, query: &str) -> Leftovers {
        match hk.show(query) {
            Lookup::Found(details) => details.leftovers,
            other => panic!("expected one item, got {other:?}"),
        }
    }

    fn home(path: &str) -> String {
        format!("{HOME}/{path}")
    }

    #[test]
    fn folders_named_after_the_item_are_found_by_kind_largest_first() {
        let fixture = Fixture::new();
        fixture
            .file(&home(".config/Tool/settings.ini"), "0123456789")
            .file(&home(".cache/tool/blob"), "x".repeat(100))
            .file(&home(".local/share/org.example.Tool/db"), "12345")
            .file(&home(".local/share/fonts/tool.ttf"), "shared")
            .file(&home(".config/toolbox/other"), "not this one");
        let mut tool = package("dpkg", "tool", None, &[]);
        tool.app_ids = vec!["org.example.Tool".into()];
        let snap_tool = package("snap", "tool", None, &[]);
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![tool],
                    ..Stub::default()
                },
                Stub {
                    id: "snap",
                    packages: vec![snap_tool],
                    ..Stub::default()
                },
            ],
        );

        let found = leftovers(&hk, "dpkg:tool");
        let summary: Vec<(String, LeftoverKind, u64)> = found
            .folders
            .iter()
            .map(|f| (f.path.display().to_string(), f.kind, f.size_bytes))
            .collect();
        assert_eq!(
            summary,
            [
                (home(".cache/tool"), LeftoverKind::Cache, 100),
                (home(".config/Tool"), LeftoverKind::Config, 10),
                (home(".local/share/org.example.Tool"), LeftoverKind::Data, 5),
            ]
        );
        assert!(!LeftoverKind::Cache.is_user_data() && LeftoverKind::Config.is_user_data());
        assert!(
            !found.folders[0].unseen_copies,
            "a system package has no copies Housekeeper does not read"
        );
        assert_eq!(
            found.folders[0].also_used_by,
            ["snap:tool"],
            "the snap install of the same app may still use it"
        );
    }

    #[test]
    fn folders_holding_installs_are_not_leftovers() {
        let fixture = Fixture::new();
        let app = home(".local/share/flatpak/app/org.example.App");
        fixture.file(&format!("{app}/current/files/bin/app"), "");
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![package("dpkg", "flatpak", None, &[])],
                    ..Stub::default()
                },
                Stub {
                    id: "flatpak",
                    packages: vec![package("flatpak", "org.example.App", Some(&app), &[])],
                    ..Stub::default()
                },
            ],
        );
        assert!(leftovers(&hk, "dpkg:flatpak").folders.is_empty());
    }

    #[test]
    fn shell_lines_mention_the_items_own_folder_and_not_a_shared_one() {
        let fixture = Fixture::new();
        fixture.file(
            &home(".bashrc"),
            "alias ll='ls -l'\n\
             export NVM_DIR=\"$HOME/.nvm\"\n\
             [ -s \"$NVM_DIR/nvm.sh\" ] && . \"$NVM_DIR/nvm.sh\"\n\
             source ~/.nvmrc-helper\n\
             export PATH=/home/user/.nvm/bin:$PATH\n\
             export PATH=\"$HOME/.cargo/bin:$PATH\"\n\
             cp -r /mnt/backup/home/user/.nvm /tmp\n",
        );
        let nvm = home(".nvm");
        let cargo_bin = home(".cargo/bin");
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "manual",
                    packages: vec![package("manual", "nvm", Some(&nvm), &[])],
                    ..Stub::default()
                },
                Stub {
                    id: "cargo",
                    packages: vec![package("cargo", "ripgrep", Some(&cargo_bin), &[])],
                    roots: vec!["/home/user/.cargo"],
                    ..Stub::default()
                },
            ],
        );

        let lines: Vec<(usize, String)> = leftovers(&hk, "manual:nvm")
            .shell_lines
            .into_iter()
            .map(|l| (l.line, l.text))
            .collect();
        assert_eq!(
            lines,
            [
                (2, "export NVM_DIR=\"$HOME/.nvm\"".to_string()),
                (5, "export PATH=/home/user/.nvm/bin:$PATH".to_string())
            ]
        );
        assert!(
            leftovers(&hk, "cargo:ripgrep").shell_lines.is_empty(),
            "~/.cargo/bin is cargo's folder, shared by every crate"
        );
    }

    #[test]
    fn folders_several_programs_share_are_never_one_items() {
        // Fedora and Arch have a package named `dconf`; `~/.config/dconf` holds every GNOME app's settings.
        let fixture = Fixture::new();
        fixture.file(&home(".config/fonts/fonts.conf"), "everyone's");
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![Stub {
                id: "rpm",
                packages: vec![package("rpm", "Fonts", None, &[])],
                ..Stub::default()
            }],
        );
        assert!(leftovers(&hk, "rpm:Fonts").folders.is_empty());
    }

    #[test]
    fn sandboxed_items_have_leftovers_only_in_their_sources_folders() {
        let fixture = Fixture::new();
        fixture
            .file(&home(".config/org.example.App/native.ini"), "")
            .file(&home(".var/app/org.example.App/config/app.ini"), "");
        let mut native = package("dpkg", "app", None, &[]);
        native.app_ids = vec!["org.example.App".into()];
        let sandboxed = |id: &str| {
            let mut app = package("flatpak", "org.example.App", None, &[]);
            app.id = id.into();
            app.app_ids = vec!["org.example.App".into()];
            app.sandboxed = true;
            app
        };
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![native],
                    ..Stub::default()
                },
                Stub {
                    id: "flatpak",
                    packages: vec![sandboxed("system/app"), sandboxed("user/app")],
                    state_roots: vec!["/home/user/.var/app"],
                    ..Stub::default()
                },
            ],
        );
        let folders = |query: &str| -> Vec<(String, Vec<String>)> {
            leftovers(&hk, query)
                .folders
                .into_iter()
                .map(|f| (f.path.display().to_string(), f.also_used_by))
                .collect()
        };

        assert_eq!(
            folders("dpkg:app"),
            [(home(".config/org.example.App"), Vec::new())],
            "the Flatpak's own folder is not the native install's, and the Flatpak cannot use ~/.config"
        );
        assert_eq!(
            folders("flatpak:system/app"),
            [(home(".var/app/org.example.App"), vec!["flatpak:user/app".to_string()])],
            "a sandboxed app's state is in its source's folder, shared by its other installation"
        );
    }

    #[test]
    fn language_packages_may_have_copies_housekeeper_does_not_read() {
        let fixture = Fixture::new();
        fixture.file(&home(".config/matplotlib/matplotlibrc"), "");
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![Stub {
                id: "pip",
                packages: vec![package("pip", "matplotlib", None, &[])],
                language: true,
                ..Stub::default()
            }],
        );
        let found = leftovers(&hk, "pip:matplotlib");
        assert_eq!(found.folders.len(), 1);
        assert!(
            found.folders[0].unseen_copies,
            "a virtual environment's copy may use it too"
        );
    }
}
