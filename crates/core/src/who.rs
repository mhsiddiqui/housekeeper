//! "Who installed this?": which source, and which package in it, a command or a file belongs to.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::host::{FileType, normalize};
use crate::inventory::{Housekeeper, Selection, SourceStatus};
use crate::model::{Package, SourceInfo};
use crate::ownership::{Ownership, all_paths, chain};
use crate::source::Source;

/// The answer to "who installed this?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Who {
    /// A command name or an absolute path, as asked.
    pub query: String,
    /// Every file the query names. For a command, the first is the one that runs; the others are on the `PATH` too
    /// but hidden behind it. Empty when nothing was found.
    pub found: Vec<Found>,
    /// Sources that could not be read or asked, so an owner may be missing.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub path: PathBuf,
    /// Where `path` leads, one symlink at a time, ending at the real file. Empty when `path` is the real file.
    pub links: Vec<PathBuf>,
    /// `None` when no source accounts for the file.
    pub owner: Option<Owner>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub source: SourceInfo,
    /// The package in that source, when it can be told.
    pub package: Option<Package>,
    pub evidence: Evidence,
}

/// Why a source is the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Evidence {
    /// The source's package database lists `path` as a file of `packages` (several for a shared directory).
    ListedFile { path: PathBuf, packages: Vec<String> },
    /// `path` is inside the package's install location.
    InsideInstall { path: PathBuf, location: PathBuf },
    /// `path` is inside a folder where the source installs programs.
    InsideFolder { path: PathBuf, folder: PathBuf },
}

impl Housekeeper {
    /// Which source and package a command on the `PATH` (`git`) or a file (an absolute path) belongs to.
    ///
    /// Every file in the chain of symlinks is checked, starting with the one asked about; the first that a source
    /// accounts for decides. For each file, in this order: a package database that lists it, a package whose install
    /// location contains it, a folder a source installs programs into. Reads every source, like a scan.
    pub fn who(&self, query: &str) -> Who {
        let mut who = Who {
            query: query.to_string(),
            found: Vec::new(),
            warnings: Vec::new(),
        };
        let paths = self.locate(query);
        if paths.is_empty() {
            return who;
        }
        let scan = self.read(&Selection::All, false, |_, _| {});
        for report in &scan.sources {
            if let SourceStatus::Failed { error } = &report.status {
                who.warnings
                    .push(format!("could not read {}: {error}", report.info.label));
            }
        }
        let chains: Vec<Vec<PathBuf>> = paths.iter().map(|path| chain(&self.host, path)).collect();
        let sources = self.platform.sources();
        let asked: Vec<&dyn Source> = sources.iter().map(|source| source.as_ref()).collect();
        let mut failures = Vec::new();
        let ownership = Ownership::ask(&self.host, sources, &asked, &all_paths(&chains), &mut failures);
        for (info, error) in failures {
            who.warnings
                .push(format!("could not ask {} who owns {query}: {error}", info.label));
        }
        for hops in chains {
            let owner = ownership.owner(&hops, &scan.packages);
            let mut hops = hops.into_iter();
            let Some(path) = hops.next() else {
                continue;
            };
            who.found.push(Found {
                path,
                links: hops.collect(),
                owner,
            });
        }
        who
    }

    /// The files a query names: an absolute path that exists, or every executable of that name on the `PATH` (one
    /// per real file, so `/bin/git` and `/usr/bin/git` count once when `/bin` links to `/usr/bin`).
    fn locate(&self, query: &str) -> Vec<PathBuf> {
        if query.contains('/') {
            let path = normalize(Path::new(query));
            let exists = self.host.file_type(&path).is_some() || self.host.link_target(&path).is_some();
            return if query.starts_with('/') && exists {
                vec![path]
            } else {
                Vec::new()
            };
        }
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        let search = self.host.var("PATH").unwrap_or_default();
        for dir in search.split(':').filter(|dir| dir.starts_with('/')) {
            let candidate = normalize(&Path::new(dir).join(query));
            if self.host.file_type(&candidate) == Some(FileType::File { executable: true })
                && seen.insert(self.host.canonical(&candidate))
            {
                found.push(candidate);
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::{Stub, housekeeper, package};
    use crate::testing::{Fixture, HOME};

    fn home(path: &str) -> String {
        format!("{HOME}/{path}")
    }

    /// `(source id, package name)` of the owner, `None` when nobody owns it.
    fn owner_of(found: &Found) -> Option<(&str, Option<&str>)> {
        let owner = found.owner.as_ref()?;
        Some((
            owner.source.id.as_str(),
            owner.package.as_ref().map(|p| p.name.as_str()),
        ))
    }

    #[test]
    fn a_command_is_followed_through_its_symlink_to_the_install_it_belongs_to() {
        let fixture = Fixture::new();
        let venv = home(".local/share/pipx/venvs/black");
        fixture.executable(&format!("{venv}/bin/black"), "");
        fixture.symlink(&home(".local/bin/black"), &format!("{venv}/bin/black"));
        let hk = housekeeper(
            &fixture,
            &home(".local/bin"),
            vec![Stub {
                id: "pipx",
                packages: vec![package("pipx", "black", Some(&venv), &["black"])],
                ..Stub::default()
            }],
        );

        let who = hk.who("black");
        let found = &who.found[0];
        assert_eq!(found.path, PathBuf::from(home(".local/bin/black")));
        assert_eq!(found.links, [PathBuf::from(format!("{venv}/bin/black"))]);
        assert_eq!(owner_of(found), Some(("pipx", Some("black"))));
        assert_eq!(
            found.owner.as_ref().unwrap().evidence,
            Evidence::InsideInstall {
                path: PathBuf::from(format!("{venv}/bin/black")),
                location: PathBuf::from(&venv),
            }
        );
    }

    #[test]
    fn the_first_match_on_the_path_runs_and_the_others_are_hidden_behind_it() {
        let fixture = Fixture::new();
        let nvm_node = home(".nvm/versions/node/v22.18.0/bin/node");
        fixture.executable(&nvm_node, "");
        fixture.executable("/usr/bin/node", "");
        // Merged /usr: /bin/node is the same file as /usr/bin/node and must not be listed twice.
        fixture.symlink("/bin", "usr/bin");
        let path = format!("{}:/usr/bin:/bin", home(".nvm/versions/node/v22.18.0/bin"));
        let hk = housekeeper(
            &fixture,
            &path,
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![package("dpkg", "nodejs", None, &[])],
                    files: vec![("/usr/bin/node", "nodejs")],
                    ..Stub::default()
                },
                Stub {
                    id: "manual",
                    packages: vec![package("manual", "nvm", Some(&home(".nvm")), &[])],
                    ..Stub::default()
                },
            ],
        );

        let who = hk.who("node");
        let paths: Vec<_> = who.found.iter().map(|found| found.path.clone()).collect();
        assert_eq!(paths, [PathBuf::from(&nvm_node), PathBuf::from("/usr/bin/node")]);
        assert_eq!(owner_of(&who.found[0]), Some(("manual", Some("nvm"))));
        assert_eq!(owner_of(&who.found[1]), Some(("dpkg", Some("nodejs"))));
    }

    #[test]
    fn a_package_database_names_the_owner_at_the_end_of_an_alternatives_chain() {
        let fixture = Fixture::new();
        fixture.executable("/usr/bin/vim.basic", "");
        fixture.symlink("/etc/alternatives/editor", "/usr/bin/vim.basic");
        fixture.symlink("/usr/bin/editor", "/etc/alternatives/editor");
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![Stub {
                id: "dpkg",
                packages: vec![package("dpkg", "vim", None, &[])],
                files: vec![("/usr/bin/vim.basic", "vim")],
                ..Stub::default()
            }],
        );

        let found = &hk.who("/usr/bin/editor").found[0];
        assert_eq!(found.links.len(), 2);
        assert_eq!(owner_of(found), Some(("dpkg", Some("vim"))));
        assert_eq!(
            found.owner.as_ref().unwrap().evidence,
            Evidence::ListedFile {
                path: PathBuf::from("/usr/bin/vim.basic"),
                packages: vec!["vim".into()],
            }
        );
    }

    #[test]
    fn a_deeper_install_along_the_chain_wins_over_an_enclosing_one() {
        // npm's link in nvm's bin folder leads into the package; nvm's own install contains both.
        let fixture = Fixture::new();
        let version = home(".nvm/versions/node/v22.18.0");
        let package_dir = format!("{version}/lib/node_modules/typescript");
        fixture.executable(&format!("{package_dir}/bin/tsc"), "");
        fixture.symlink(&format!("{version}/bin/tsc"), "../lib/node_modules/typescript/bin/tsc");
        let hk = housekeeper(
            &fixture,
            &format!("{version}/bin"),
            vec![
                Stub {
                    id: "manual",
                    packages: vec![package("manual", "nvm", Some(&home(".nvm")), &[])],
                    ..Stub::default()
                },
                Stub {
                    id: "npm",
                    packages: vec![package("npm", "typescript", Some(&package_dir), &["tsc", "tsserver"])],
                    ..Stub::default()
                },
            ],
        );

        assert_eq!(owner_of(&hk.who("tsc").found[0]), Some(("npm", Some("typescript"))));
    }

    #[test]
    fn packages_sharing_one_location_are_told_apart_by_command() {
        let fixture = Fixture::new();
        fixture.executable(&home(".cargo/bin/rg"), "");
        let bin = home(".cargo/bin");
        let hk = housekeeper(
            &fixture,
            &bin,
            vec![Stub {
                id: "cargo",
                packages: vec![
                    package("cargo", "fd-find", Some(&bin), &["fd"]),
                    package("cargo", "ripgrep", Some(&bin), &["rg"]),
                ],
                ..Stub::default()
            }],
        );

        assert_eq!(owner_of(&hk.who("rg").found[0]), Some(("cargo", Some("ripgrep"))));
    }

    #[test]
    fn a_source_folder_decides_before_the_link_reaches_another_package() {
        // /snap/bin/firefox is a link to /usr/bin/snap, which the snapd package owns; the snap is the answer.
        let fixture = Fixture::new();
        fixture.executable("/usr/bin/snap", "");
        fixture.symlink("/snap/bin/firefox", "/usr/bin/snap");
        let hk = housekeeper(
            &fixture,
            "/snap/bin:/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![package("dpkg", "snapd", None, &[])],
                    files: vec![("/usr/bin/snap", "snapd")],
                    ..Stub::default()
                },
                Stub {
                    id: "snap",
                    packages: vec![package("snap", "firefox", Some("/snap/firefox/current"), &["firefox"])],
                    roots: vec!["/snap"],
                    ..Stub::default()
                },
            ],
        );

        let found = &hk.who("firefox").found[0];
        assert_eq!(owner_of(found), Some(("snap", Some("firefox"))));
        assert!(matches!(
            found.owner.as_ref().unwrap().evidence,
            Evidence::InsideFolder { .. }
        ));
    }

    #[test]
    fn unowned_files_unknown_names_and_unreadable_sources() {
        let fixture = Fixture::new();
        fixture.executable("/usr/local/bin/mystery", "");
        let hk = housekeeper(
            &fixture,
            "/usr/local/bin",
            vec![Stub {
                id: "broken",
                broken: true,
                ..Stub::default()
            }],
        );

        let who = hk.who("mystery");
        assert_eq!(who.found.len(), 1);
        assert_eq!(owner_of(&who.found[0]), None);
        assert_eq!(who.warnings.len(), 1, "{:?}", who.warnings);

        assert!(hk.who("no-such-command").found.is_empty());
        assert!(hk.who("/usr/local/bin/no-such-file").found.is_empty());
    }
}
