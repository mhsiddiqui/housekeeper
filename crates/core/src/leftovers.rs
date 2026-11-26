//! Leftovers: what an installed item leaves behind outside its own files: settings, caches and data in the folders the
//! platform keeps per-user state in, and lines in shell startup files that load it (decisions D7, D36). Found only;
//! nothing here changes anything.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::host::{FileType, Host};
use crate::model::Package;
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
    /// Other installed items (`source:id`) whose names match the folder too: it may still be theirs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_used_by: Vec<String>,
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

/// Folder names in the per-user state folders that several programs share; never one item's leftover.
const SHARED_FOLDERS: &[&str] = &[
    "applications",
    "autostart",
    "dbus-1",
    "desktop-directories",
    "fonts",
    "icons",
    "menus",
    "mime",
    "sounds",
    "systemd",
    "themes",
    "trash",
];

/// What `package` (one of `installed`) leaves behind on `platform`.
pub(crate) fn find(platform: &dyn Platform, host: &Host, package: &Package, installed: &[Package]) -> Leftovers {
    let own = names(package);
    let roots: Vec<PathBuf> = platform
        .sources()
        .iter()
        .flat_map(|source| source.install_roots(host))
        .collect();
    // Folders that hold installs (a package's location, a source's install root) are installs, not leftovers.
    let install_dirs: Vec<&Path> = installed
        .iter()
        .filter_map(|p| p.location.as_deref())
        .chain(roots.iter().map(PathBuf::as_path))
        .collect();
    let mut folders = Vec::new();
    for (root, kind) in platform.leftover_roots(host) {
        let Ok(entries) = host.read_dir(&root) else {
            continue;
        };
        for entry in entries {
            let name = entry.name.to_lowercase();
            if entry.file_type != Some(FileType::Dir)
                || entry.is_symlink
                || !own.contains(&name)
                || SHARED_FOLDERS.contains(&name.as_str())
                || install_dirs.iter().any(|dir| dir.starts_with(&entry.path))
                || package.location.as_ref().is_some_and(|own| entry.path.starts_with(own))
            {
                continue;
            }
            let also_used_by = installed
                .iter()
                .filter(|other| other.key() != package.key() && names(other).contains(&name))
                .map(Package::key)
                .collect();
            folders.push(Leftover {
                size_bytes: size(host, &entry.path),
                path: entry.path,
                kind,
                also_used_by,
            });
        }
    }
    folders.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then_with(|| a.path.cmp(&b.path)));
    Leftovers {
        folders,
        shell_lines: shell_lines(platform, host, package, installed, &roots),
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

/// `line` names `path` itself or something inside it, not a longer name that starts the same (`~/.nvmrc`).
fn mentions(line: &str, path: &str) -> bool {
    line.match_indices(path).any(|(at, _)| {
        line[at + path.len()..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_alphanumeric() || matches!(next, '-' | '_' | '.')))
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
             export PATH=\"$HOME/.cargo/bin:$PATH\"\n",
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
}
