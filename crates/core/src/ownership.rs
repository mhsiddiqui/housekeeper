//! Which source, and which package in it, accounts for a file. Shared by `who` and by the scan, which uses it to
//! attach launchers and commands to packages.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::host::{Host, MAX_LINKS};
use crate::model::{Package, SourceId, SourceInfo};
use crate::source::{Source, SourceError};
use crate::who::{Evidence, Owner};

/// One package database's answer: the packages that own each path it was asked about.
type Listed<'a> = (&'a dyn Source, HashMap<PathBuf, Vec<String>>);

/// The evidence about many files, gathered at once: every package database asked is asked a single time, about every
/// file of every chain.
pub(crate) struct Ownership<'a> {
    sources: &'a [Arc<dyn Source>],
    roots: Vec<(&'a dyn Source, PathBuf)>,
    listed: Vec<Listed<'a>>,
}

impl<'a> Ownership<'a> {
    /// Asks the package database of each source in `asked` that has one who owns `paths`. `sources` are all of the
    /// platform's, whose install folders count as evidence too. A database that cannot answer is left out and
    /// returned in `failures`.
    pub fn ask(
        host: &Host,
        sources: &'a [Arc<dyn Source>],
        asked: &[&'a dyn Source],
        paths: &[PathBuf],
        failures: &mut Vec<(SourceInfo, SourceError)>,
    ) -> Self {
        let roots = sources
            .iter()
            .flat_map(|source| {
                let source = source.as_ref();
                source.install_roots(host).into_iter().map(move |root| (source, root))
            })
            .collect();
        let mut listed = Vec::new();
        for source in asked {
            let Some(lookup) = source.owner_lookup() else {
                continue;
            };
            match lookup.owners(host, paths) {
                Ok(owners) => listed.push((*source, owners)),
                Err(error) => failures.push((source.info(), error)),
            }
        }
        Self { sources, roots, listed }
    }

    /// Whether this source's package database answered.
    pub fn answered(&self, id: &SourceId) -> bool {
        self.listed.iter().any(|(source, _)| &source.info().id == id)
    }

    /// The owner of the first file in `hops` (a file and the files its symlinks lead to, in order) that a source
    /// accounts for. For each file, in this order: a package database that lists it, a package whose install location
    /// contains it, a folder a source installs programs into. `packages` are the scanned packages.
    pub fn owner(&self, hops: &[PathBuf], packages: &[Package]) -> Option<Owner> {
        // The command as typed and every file name on the way: `rg`, or `tsc` for a link to `…/bin/tsc`.
        let names: Vec<&str> = hops
            .iter()
            .filter_map(|hop| hop.file_name().and_then(|name| name.to_str()))
            .collect();
        for (index, hop) in hops.iter().enumerate() {
            if let Some(owner) = self.listed_owner(hop, packages) {
                return Some(owner);
            }
            // An install location further down the chain can be more specific: a link inside `~/.nvm` (the nvm
            // install) that leads into `node_modules/<package>` belongs to that npm package.
            let install = hops[index..]
                .iter()
                .filter_map(|later| self.install_owner(later, packages, &names))
                .reduce(|best, other| {
                    if location_depth(&other) > location_depth(&best) {
                        other
                    } else {
                        best
                    }
                });
            if let Some(owner) = install.or_else(|| self.folder_owner(hop, packages, &names)) {
                return Some(owner);
            }
        }
        None
    }

    /// A package database that lists `hop` as a file. The package is named only when it is the one owner; a directory
    /// several packages share (`/usr/local/bin`) names the source alone.
    fn listed_owner(&self, hop: &Path, packages: &[Package]) -> Option<Owner> {
        self.listed.iter().find_map(|(source, owners)| {
            let names = owners.get(hop)?;
            let info = source.info();
            let package = match names.as_slice() {
                [name] => packages
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

    /// The package whose install location contains `hop` most closely. Packages sharing that location (everything
    /// `cargo install` puts in `~/.cargo/bin`) are told apart by their commands.
    fn install_owner(&self, hop: &Path, packages: &[Package], names: &[&str]) -> Option<Owner> {
        let inside: Vec<&Package> = packages
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

    /// The source whose install folder contains `hop` (the deepest folder wins), with its package of that command
    /// name.
    fn folder_owner(&self, hop: &Path, packages: &[Package], names: &[&str]) -> Option<Owner> {
        let (source, folder) = self
            .roots
            .iter()
            .filter(|(_, root)| hop.starts_with(root))
            .max_by_key(|(_, root)| root.components().count())?;
        let info = source.info();
        // Names in chain order: `/snap/bin/pinta` (a link to `/usr/bin/snap`) is the pinta snap, not snapd's `snap`.
        let package = names
            .iter()
            .find_map(|name| {
                packages.iter().find(|package| {
                    package.source == info.id
                        && (package.commands.iter().any(|command| command == name)
                            || *name == package.name
                            || *name == package.id)
                })
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

    fn source_info(&self, id: &SourceId) -> Option<SourceInfo> {
        self.sources
            .iter()
            .map(|source| source.info())
            .find(|info| &info.id == id)
    }
}

/// The symlinks from `path` to its real file, then the real file's canonical path if a linked directory on the way
/// makes it differ.
pub(crate) fn links(host: &Host, path: &Path) -> Vec<PathBuf> {
    let mut links = Vec::new();
    let mut current = path.to_path_buf();
    while links.len() < MAX_LINKS {
        let Some(target) = host.link_target(&current) else {
            break;
        };
        links.push(target.clone());
        current = target;
    }
    let real = host.canonical(path);
    if real != current {
        links.push(real);
    }
    links
}

/// `path` followed by its [`links`].
pub(crate) fn chain(host: &Host, path: &Path) -> Vec<PathBuf> {
    std::iter::once(path.to_path_buf()).chain(links(host, path)).collect()
}

/// Every path of every chain, once.
pub(crate) fn all_paths<'p>(chains: impl IntoIterator<Item = &'p Vec<PathBuf>>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    chains
        .into_iter()
        .flatten()
        .filter(|path| seen.insert(path.as_path()))
        .cloned()
        .collect()
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
