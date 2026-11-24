//! "Who installed this?": which source, and which package in it, a command or a file belongs to.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::host::{FileType, MAX_LINKS, normalize};
use crate::inventory::{Housekeeper, Scan, Selection, SourceStatus};
use crate::model::{Package, SourceId, SourceInfo};
use crate::source::Source;

/// One package database's answer: the packages that own each path it was asked about.
type Listed<'a> = (&'a dyn Source, HashMap<PathBuf, Vec<String>>);

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
        let scan = self.scan(&Selection::All);
        for report in &scan.sources {
            if let SourceStatus::Failed { error } = &report.status {
                who.warnings
                    .push(format!("could not read {}: {error}", report.info.label));
            }
        }
        let roots: Vec<(&dyn Source, PathBuf)> = self
            .platform
            .sources()
            .iter()
            .flat_map(|source| {
                let source = source.as_ref();
                source
                    .install_roots(&self.host)
                    .into_iter()
                    .map(move |root| (source, root))
            })
            .collect();
        for path in paths {
            let links = self.links(&path);
            let hops: Vec<PathBuf> = std::iter::once(path.clone()).chain(links.iter().cloned()).collect();
            let owner = self.owner(&hops, &scan, &roots, &mut who.warnings);
            who.found.push(Found { path, links, owner });
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

    /// The symlinks from `path` to its real file, then the real file's canonical path if a linked directory on the
    /// way makes it differ.
    fn links(&self, path: &Path) -> Vec<PathBuf> {
        let mut links = Vec::new();
        let mut current = path.to_path_buf();
        while links.len() < MAX_LINKS {
            let Some(target) = self.host.link_target(&current) else {
                break;
            };
            links.push(target.clone());
            current = target;
        }
        let real = self.host.canonical(path);
        if real != current {
            links.push(real);
        }
        links
    }

    fn owner(
        &self,
        hops: &[PathBuf],
        scan: &Scan,
        roots: &[(&dyn Source, PathBuf)],
        warnings: &mut Vec<String>,
    ) -> Option<Owner> {
        let mut listed: Vec<Listed<'_>> = Vec::new();
        for source in self.platform.sources() {
            let Some(lookup) = source.owner_lookup() else {
                continue;
            };
            match lookup.owners(&self.host, hops) {
                Ok(owners) => listed.push((source.as_ref(), owners)),
                Err(error) => warnings.push(format!(
                    "could not ask {} who owns {}: {error}",
                    source.info().label,
                    hops[0].display()
                )),
            }
        }
        // The command as typed and every file name on the way: `rg`, or `tsc` for a link to `…/bin/tsc`.
        let names: Vec<&str> = hops
            .iter()
            .filter_map(|hop| hop.file_name().and_then(|name| name.to_str()))
            .collect();
        for (index, hop) in hops.iter().enumerate() {
            if let Some(owner) = listed_owner(hop, &listed, scan) {
                return Some(owner);
            }
            // An install location further down the chain can be more specific: a link inside `~/.nvm` (the nvm
            // install) that leads into `node_modules/<package>` belongs to that npm package.
            let install = hops[index..]
                .iter()
                .filter_map(|later| self.install_owner(later, scan, &names))
                .reduce(|best, other| {
                    if location_depth(&other) > location_depth(&best) {
                        other
                    } else {
                        best
                    }
                });
            if let Some(owner) = install.or_else(|| folder_owner(hop, roots, scan, &names)) {
                return Some(owner);
            }
        }
        None
    }

    /// The package whose install location contains `hop` most closely. Packages sharing that location (everything
    /// `cargo install` puts in `~/.cargo/bin`) are told apart by their commands.
    fn install_owner(&self, hop: &Path, scan: &Scan, names: &[&str]) -> Option<Owner> {
        let inside: Vec<&Package> = scan
            .packages
            .iter()
            .filter(|package| {
                package
                    .location
                    .as_ref()
                    .is_some_and(|location| hop.starts_with(location))
            })
            .collect();
        let deepest = inside.iter().filter_map(|package| depth(package)).max()?;
        let closest: Vec<&Package> = inside
            .into_iter()
            .filter(|package| depth(package) == Some(deepest))
            .collect();
        let by_command: Vec<&Package> = closest
            .iter()
            .copied()
            .filter(|package| has_command(package, names))
            .collect();
        let package = match (by_command.as_slice(), closest.as_slice()) {
            ([one], _) | ([], [one]) => Some(*one),
            _ => None,
        };
        let first = closest.first()?;
        if package.is_none() && closest.iter().any(|other| other.source != first.source) {
            return None;
        }
        Some(Owner {
            source: self.source_info(&first.source)?,
            package: package.cloned(),
            evidence: Evidence::InsideInstall {
                path: hop.to_path_buf(),
                location: first.location.clone()?,
            },
        })
    }

    fn source_info(&self, id: &SourceId) -> Option<SourceInfo> {
        self.platform
            .sources()
            .iter()
            .map(|source| source.info())
            .find(|info| &info.id == id)
    }
}

/// A package database that lists `hop` as a file. The package is named only when it is the one owner; a directory
/// several packages share (`/usr/local/bin`) names the source alone.
fn listed_owner(hop: &Path, listed: &[Listed<'_>], scan: &Scan) -> Option<Owner> {
    listed.iter().find_map(|(source, owners)| {
        let names = owners.get(hop)?;
        let info = source.info();
        let package = match names.as_slice() {
            [name] => scan
                .packages
                .iter()
                .find(|package| package.source == info.id && (*name == package.id || *name == package.name))
                .cloned(),
            _ => None,
        };
        Some(Owner {
            source: info,
            package,
            evidence: Evidence::ListedFile {
                path: hop.to_path_buf(),
                packages: names.clone(),
            },
        })
    })
}

/// The source whose install folder contains `hop` (the deepest folder wins), with its package of that command name.
fn folder_owner(hop: &Path, roots: &[(&dyn Source, PathBuf)], scan: &Scan, names: &[&str]) -> Option<Owner> {
    let (source, folder) = roots
        .iter()
        .filter(|(_, root)| hop.starts_with(root))
        .max_by_key(|(_, root)| root.components().count())?;
    let info = source.info();
    let package = scan
        .packages
        .iter()
        .find(|package| {
            package.source == info.id
                && (has_command(package, names)
                    || names.iter().any(|name| *name == package.name || *name == package.id))
        })
        .cloned();
    Some(Owner {
        source: info,
        package,
        evidence: Evidence::InsideFolder {
            path: hop.to_path_buf(),
            folder: folder.clone(),
        },
    })
}

fn depth(package: &Package) -> Option<usize> {
    package.location.as_ref().map(|location| location.components().count())
}

fn has_command(package: &Package, names: &[&str]) -> bool {
    package.commands.iter().any(|command| names.contains(&command.as_str()))
}

fn location_depth(owner: &Owner) -> usize {
    match &owner.evidence {
        Evidence::InsideInstall { location, .. } => location.components().count(),
        Evidence::ListedFile { .. } | Evidence::InsideFolder { .. } => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use crate::model::{Availability, Scope, SourceKind, SystemInfo};
    use crate::platform::Platform;
    use crate::source::{OwnerLookup, ScanContext, SourceError};
    use crate::testing::{FakeRunner, Fixture, HOME};
    use std::sync::Arc;

    #[derive(Default)]
    struct Stub {
        id: &'static str,
        packages: Vec<Package>,
        roots: Vec<&'static str>,
        /// `(path, package)` pairs its package database lists.
        files: Vec<(&'static str, &'static str)>,
        broken: bool,
    }

    impl Source for Stub {
        fn info(&self) -> SourceInfo {
            SourceInfo::new(self.id, self.id, SourceKind::System)
        }
        fn probe(&self, _: &Host) -> Availability {
            Availability::Available { version: None }
        }
        fn inventory(&self, _: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
            if self.broken {
                return Err(SourceError::parse("database", "garbage"));
            }
            Ok(self.packages.clone())
        }
        fn owner_lookup(&self) -> Option<&dyn OwnerLookup> {
            (!self.files.is_empty()).then_some(self as &dyn OwnerLookup)
        }
        fn install_roots(&self, _: &Host) -> Vec<PathBuf> {
            self.roots.iter().map(PathBuf::from).collect()
        }
    }

    impl OwnerLookup for Stub {
        fn owners(&self, _: &Host, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<String>>, SourceError> {
            let mut owners: HashMap<PathBuf, Vec<String>> = HashMap::new();
            for (path, package) in &self.files {
                if paths.iter().any(|asked| asked == Path::new(path)) {
                    owners.entry(PathBuf::from(path)).or_default().push(package.to_string());
                }
            }
            Ok(owners)
        }
    }

    struct TestPlatform {
        system: SystemInfo,
        sources: Vec<Arc<dyn Source>>,
    }

    impl Platform for TestPlatform {
        fn system(&self) -> &SystemInfo {
            &self.system
        }
        fn sources(&self) -> &[Arc<dyn Source>] {
            &self.sources
        }
    }

    fn housekeeper(fixture: &Fixture, path: &str, sources: Vec<Stub>) -> Housekeeper {
        let system = SystemInfo {
            os: "linux".into(),
            name: "Test".into(),
            id: "test".into(),
            version: None,
            family: "test".into(),
            image_based: false,
            arch: "x86_64".into(),
        };
        let sources = sources.into_iter().map(|s| Arc::new(s) as Arc<dyn Source>).collect();
        let host = fixture.host_with_vars(FakeRunner::new(), &[("PATH", path)]);
        Housekeeper::new(Box::new(TestPlatform { system, sources }), host)
    }

    fn package(source: &'static str, name: &str, location: Option<&str>, commands: &[&str]) -> Package {
        let mut package = Package::new(SourceId::new(source), name, name, Scope::User);
        package.location = location.map(PathBuf::from);
        package.commands = commands.iter().map(|c| c.to_string()).collect();
        package
    }

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
