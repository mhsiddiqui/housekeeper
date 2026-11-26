//! `housekeeper show`: one installed item in full: details, launchers, other installs of the same app, files, the
//! packages that need it, how it would be removed and what it would leave behind.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::group::{Shared, keys};
use crate::host::{FileType, Host};
use crate::inventory::{Housekeeper, Selection, SourceStatus};
use crate::leftovers::{self, Leftovers};
use crate::model::{Package, SourceInfo};
use crate::remove::{self, RemovalPlan};

/// What a query names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(Box<Details>),
    /// Several installed items answer to the name; `source:id` picks one.
    Several(Vec<Package>),
    NotFound,
}

/// One installed item in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Details {
    pub package: Package,
    pub source: SourceInfo,
    /// Other installs of the same app (`housekeeper list --duplicates`), in list order.
    pub also_installed: Vec<Package>,
    /// What this install has in common with them.
    pub shared: Vec<Shared>,
    /// `None` when neither the source nor an install location tells.
    pub files: Option<Files>,
    /// Installed packages of the same source that directly need this one; `None` when the source cannot tell.
    pub needed_by: Option<Vec<Package>>,
    /// How it would be removed; `None` when the source could not make the plan (a warning says why).
    pub removal: Option<RemovalPlan>,
    /// What it leaves behind outside its own files (D36).
    pub leftovers: Leftovers,
    /// Sources that could not be read, and questions a source could not answer.
    pub warnings: Vec<String>,
}

/// The files of an installed item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Files {
    pub from: FilesFrom,
    /// Every path, directories included, as the source lists them or as found in the install location.
    pub paths: Vec<PathBuf>,
    /// Paths that are not directories and exist (symlinks count, at no size).
    pub count: usize,
    /// Listed paths no longer on disk.
    pub missing: usize,
    /// Size of the files that are not symlinks.
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FilesFrom {
    /// The source records which files the package installed (a package database, pip's `RECORD`).
    Recorded,
    /// Everything in the package's install location.
    Location { path: PathBuf },
}

impl Housekeeper {
    /// The installed item `query` names: `source:id` (`dpkg:firefox`), or an id, name or title of exactly one item.
    /// Reads every source, like a scan.
    pub fn show(&self, query: &str) -> Lookup {
        let scan = self.scan(&Selection::All);
        let mut matches: Vec<&Package> = scan.packages.iter().filter(|p| p.key() == query).collect();
        if matches.is_empty() {
            matches = scan
                .packages
                .iter()
                .filter(|p| p.id == query || p.name == query)
                .collect();
        }
        if matches.is_empty() {
            let wanted = query.to_lowercase();
            matches = scan
                .packages
                .iter()
                .filter(|p| {
                    p.name.to_lowercase() == wanted || p.title.as_ref().is_some_and(|t| t.to_lowercase() == wanted)
                })
                .collect();
        }
        let package = match matches.as_slice() {
            [] => return Lookup::NotFound,
            [one] => (*one).clone(),
            several => return Lookup::Several(several.iter().map(|p| (*p).clone()).collect()),
        };

        let mut warnings: Vec<String> = scan
            .sources
            .iter()
            .filter_map(|report| match &report.status {
                SourceStatus::Failed { error } => Some(format!("could not read {}: {error}", report.info.label)),
                _ => None,
            })
            .collect();
        let Some(source) = self
            .platform
            .sources()
            .iter()
            .find(|source| source.info().id == package.source)
        else {
            return Lookup::NotFound;
        };
        let info = source.info();

        let key = package.key();
        let group = scan.groups.iter().find(|group| group.packages.contains(&key));
        let also_installed: Vec<Package> = group
            .map(|group| {
                group
                    .packages
                    .iter()
                    .filter(|member| **member != key)
                    .filter_map(|member| scan.packages.iter().find(|p| p.key() == *member).cloned())
                    .collect()
            })
            .unwrap_or_default();
        // What this install shares with the others; the group's list also holds links between the others.
        let own = keys(&package);
        let shared = group
            .map(|group| group.shared.iter().filter(|key| own.contains(key)).cloned().collect())
            .unwrap_or_default();

        let files = match source.file_list() {
            Some(list) => match list.files(&self.host, &package) {
                Ok(paths) => Some(files(&self.host, FilesFrom::Recorded, paths)),
                Err(error) => {
                    warnings.push(format!(
                        "could not ask {} for the files of {}: {error}",
                        info.label, package.name
                    ));
                    None
                }
            },
            None => package.location.as_ref().map(|location| {
                let (paths, unreadable) = walk(&self.host, location);
                if unreadable > 0 {
                    warnings.push(format!(
                        "{unreadable} folders under {} could not be read; the file count leaves them out",
                        location.display()
                    ));
                }
                files(&self.host, FilesFrom::Location { path: location.clone() }, paths)
            }),
        };

        let needed_by =
            source
                .dependant_lookup()
                .and_then(|dependants| match dependants.dependants(&self.host, &package) {
                    Ok(ids) => Some(
                        scan.packages
                            .iter()
                            .filter(|p| p.source == package.source && ids.contains(&p.id))
                            .cloned()
                            .collect(),
                    ),
                    Err(error) => {
                        warnings.push(format!(
                            "could not ask {} what needs {}: {error}",
                            info.label, package.name
                        ));
                        None
                    }
                });

        let removal = match remove::plan(source.as_ref(), &self.host, &package, &scan.packages, &mut warnings) {
            Ok(plan) => Some(plan),
            Err(error) => {
                warnings.push(format!(
                    "could not plan the removal of {} from {}: {error}",
                    package.name, info.label
                ));
                None
            }
        };

        let leftovers = leftovers::find(self.platform.as_ref(), &self.host, &package, &scan.packages);
        Lookup::Found(Box::new(Details {
            package,
            source: info,
            also_installed,
            shared,
            files,
            needed_by,
            removal,
            leftovers,
            warnings,
        }))
    }
}

/// Counts what exists of `paths` and how big it is.
fn files(host: &Host, from: FilesFrom, paths: Vec<PathBuf>) -> Files {
    let mut count = 0;
    let mut missing = 0;
    let mut size_bytes = 0;
    for path in &paths {
        if host.link_target(path).is_some() {
            count += 1;
            continue;
        }
        match host.metadata(path) {
            None => missing += 1,
            Some(meta) if meta.is_dir() => {}
            Some(meta) => {
                count += 1;
                size_bytes += meta.len();
            }
        }
    }
    Files {
        from,
        paths,
        count,
        missing,
        size_bytes,
    }
}

/// `location` and everything below it, without following symlinked directories (a link is listed, not entered), and
/// how many folders could not be read. A file location is the file alone.
fn walk(host: &Host, location: &Path) -> (Vec<PathBuf>, usize) {
    let mut paths = vec![location.to_path_buf()];
    if host.link_target(location).is_some() || host.file_type(location) != Some(FileType::Dir) {
        return (paths, 0);
    }
    let mut unreadable = 0;
    let mut pending = vec![location.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = host.read_dir(&dir) else {
            unreadable += 1;
            continue;
        };
        for entry in entries {
            if entry.file_type == Some(FileType::Dir) && !entry.is_symlink {
                pending.push(entry.path.clone());
            }
            paths.push(entry.path);
        }
    }
    (paths, unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PackageKind;
    use crate::remove::{Actor, Simulation, Step, Tier};
    use crate::stub::{Stub, housekeeper, package};
    use crate::testing::{Fixture, HOME};

    fn found(lookup: Lookup) -> Details {
        match lookup {
            Lookup::Found(details) => *details,
            other => panic!("expected one item, got {other:?}"),
        }
    }

    fn keys(packages: &[Package]) -> Vec<String> {
        packages.iter().map(Package::key).collect()
    }

    #[test]
    fn a_query_names_one_item_by_key_id_name_or_title_or_several() {
        let fixture = Fixture::new();
        let mut code = package("dpkg", "code", None, &[]);
        code.title = Some("Visual Studio Code".into());
        let mut merge_deb = package("dpkg", "sublime-merge", None, &[]);
        merge_deb.kind = PackageKind::App;
        let mut merge_snap = package("snap", "sublime-merge", None, &[]);
        merge_snap.kind = PackageKind::App;
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![code, merge_deb],
                    ..Stub::default()
                },
                Stub {
                    id: "snap",
                    packages: vec![merge_snap],
                    ..Stub::default()
                },
            ],
        );

        assert_eq!(found(hk.show("dpkg:code")).package.name, "code");
        assert_eq!(found(hk.show("code")).package.name, "code");
        assert_eq!(found(hk.show("visual studio code")).package.name, "code");
        match hk.show("sublime-merge") {
            Lookup::Several(several) => assert_eq!(keys(&several), ["dpkg:sublime-merge", "snap:sublime-merge"]),
            other => panic!("expected several, got {other:?}"),
        }
        let merge = found(hk.show("snap:sublime-merge"));
        assert_eq!(keys(&merge.also_installed), ["dpkg:sublime-merge"]);
        assert_eq!(hk.show("nothing-like-this"), Lookup::NotFound);
    }

    #[test]
    fn files_come_from_the_source_or_else_from_the_install_location() {
        let fixture = Fixture::new();
        fixture.executable("/usr/bin/tool", "0123456789");
        fixture.symlink("/usr/bin/tool-alias", "tool");
        fixture.dir("/usr/share/doc/tool");
        let module = format!("{HOME}/.npm-global/lib/node_modules/thing");
        fixture.file(&format!("{module}/index.js"), "12345");
        fixture.file(&format!("{module}/lib/util.js"), "1234567");
        fixture.symlink(&format!("{module}/linked-dir"), "/usr/share");
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![package("dpkg", "tool", None, &[])],
                    files: vec![
                        ("/usr/bin/tool", "tool"),
                        ("/usr/bin/tool-alias", "tool"),
                        ("/usr/share/doc/tool", "tool"),
                        ("/usr/share/tool/removed-by-hand", "tool"),
                    ],
                    ..Stub::default()
                },
                Stub {
                    id: "npm",
                    packages: vec![package("npm", "thing", Some(&module), &[])],
                    ..Stub::default()
                },
            ],
        );

        let recorded = found(hk.show("dpkg:tool")).files.unwrap();
        assert_eq!(recorded.from, FilesFrom::Recorded);
        assert_eq!(
            (
                recorded.paths.len(),
                recorded.count,
                recorded.missing,
                recorded.size_bytes
            ),
            (4, 2, 1, 10),
            "the folder is not a file, the alias counts at no size, the removed file is missing"
        );

        let walked = found(hk.show("npm:thing")).files.unwrap();
        assert_eq!(
            walked.from,
            FilesFrom::Location {
                path: PathBuf::from(&module)
            }
        );
        let mut paths = walked.paths.clone();
        paths.sort();
        assert_eq!(
            paths,
            ["", "/index.js", "/lib", "/lib/util.js", "/linked-dir"]
                .map(|rest| PathBuf::from(format!("{module}{rest}")))
        );
        assert_eq!(
            (walked.count, walked.size_bytes),
            (3, 12),
            "the linked folder is listed, not entered"
        );
    }

    #[test]
    fn needed_by_names_the_dependants_and_shared_is_what_this_install_shares() {
        let fixture = Fixture::new();
        let app = |source: &'static str, id: &str, name: &str, commands: &[&str]| {
            let mut package = package(source, name, None, commands);
            package.id = id.into();
            package.kind = PackageKind::App;
            package
        };
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![
                        package("dpkg", "libfoo", None, &[]),
                        package("dpkg", "foo-tools", None, &[]),
                        package("dpkg", "unrelated", None, &[]),
                    ],
                    needs: vec![("foo-tools", "libfoo")],
                    ..Stub::default()
                },
                Stub {
                    id: "npm",
                    packages: vec![
                        app("npm", "v20/typescript", "typescript", &["tsc"]),
                        app("npm", "v22/typescript", "typescript", &["tsc"]),
                    ],
                    ..Stub::default()
                },
                Stub {
                    id: "manual",
                    packages: vec![app("manual", "/opt/tsc", "tsc-standalone", &["tsc"])],
                    ..Stub::default()
                },
            ],
        );

        let libfoo = found(hk.show("dpkg:libfoo"));
        assert_eq!(keys(libfoo.needed_by.as_deref().unwrap()), ["dpkg:foo-tools"]);
        assert_eq!(found(hk.show("dpkg:unrelated")).needed_by, Some(Vec::new()));
        assert_eq!(found(hk.show("npm:v20/typescript")).needed_by, None, "npm cannot tell");

        let standalone = found(hk.show("tsc-standalone"));
        assert_eq!(
            keys(&standalone.also_installed),
            ["npm:v20/typescript", "npm:v22/typescript"]
        );
        assert_eq!(
            standalone.shared,
            [Shared::Command("tsc".into())],
            "the two npm installs also share their name, which is not this install's"
        );
    }

    #[test]
    fn removal_comes_from_the_source_and_is_unknown_without_a_remover() {
        let fixture = Fixture::new();
        let plan = RemovalPlan::new(
            Tier::PackageManager,
            vec![Step::run(Actor::User, ["npm", "uninstall", "-g", "thing"])],
        );
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "npm",
                    packages: vec![package("npm", "thing", None, &[])],
                    plan: Some(plan.clone()),
                    ..Stub::default()
                },
                Stub {
                    id: "manual",
                    packages: vec![package("manual", "tool", None, &[])],
                    ..Stub::default()
                },
                Stub {
                    id: "dpkg",
                    packages: vec![package("dpkg", "broken", None, &[])],
                    plan_fails: true,
                    ..Stub::default()
                },
            ],
        );

        assert_eq!(found(hk.show("npm:thing")).removal, Some(plan));
        assert_eq!(found(hk.show("manual:tool")).removal, Some(RemovalPlan::unknown()));
        let broken = found(hk.show("dpkg:broken"));
        assert_eq!(broken.removal, None);
        assert!(
            broken
                .warnings
                .iter()
                .any(|w| w.contains("could not plan the removal of broken")),
            "{:?}",
            broken.warnings
        );
    }

    #[test]
    fn a_protected_package_blocks_its_plan_whatever_the_source_planned() {
        let fixture = Fixture::new();
        let apt = RemovalPlan::new(
            Tier::PackageManager,
            vec![Step::run(Actor::Helper, ["apt-get", "remove", "--yes", "init"])],
        );
        let protect = |mut package: Package| {
            package.protected = Some("dpkg marks it Protected".into());
            package
        };
        let hk = housekeeper(
            &fixture,
            "/usr/bin",
            vec![
                Stub {
                    id: "dpkg",
                    packages: vec![
                        protect(package("dpkg", "init", None, &[])),
                        package("dpkg", "vim", None, &[]),
                    ],
                    plan: Some(apt.clone()),
                    ..Stub::default()
                },
                Stub {
                    id: "manual",
                    packages: vec![protect(package("manual", "tool", None, &[]))],
                    ..Stub::default()
                },
            ],
        );

        assert_eq!(
            found(hk.show("dpkg:init")).removal,
            Some(apt.clone().blocked("protected: dpkg marks it Protected")),
            "the steps stay, to show what it would take"
        );
        assert_eq!(found(hk.show("dpkg:vim")).removal, Some(apt));
        assert_eq!(
            found(hk.show("manual:tool")).removal.map(|plan| plan.blockers),
            Some(vec!["protected: dpkg marks it Protected".to_string()])
        );
    }

    #[test]
    fn the_simulation_is_attached_and_blocks_when_refused_or_taking_a_protected_package() {
        let fixture = Fixture::new();
        let apt = RemovalPlan::new(
            Tier::PackageManager,
            vec![Step::run(Actor::Helper, ["apt-get", "remove", "--yes", "x"])],
        );
        let simulation = |removes: &[&str], refused: &[&str]| Simulation {
            by: "`apt-get --simulate remove`".into(),
            removes: removes.iter().map(|id| id.to_string()).collect(),
            refused: refused.iter().map(|line| line.to_string()).collect(),
        };
        let mut passwd = package("dpkg", "passwd", None, &[]);
        passwd.protected = Some("its priority is required".into());
        let stub = |packages: Vec<Package>, simulation: Option<Simulation>, simulation_fails| Stub {
            id: "dpkg",
            packages,
            plan: Some(apt.clone()),
            simulation,
            simulation_fails,
            ..Stub::default()
        };
        let show = |stub: Stub, query: &str| found(housekeeper(&fixture, "/usr/bin", vec![stub]).show(query));

        let leaf = show(
            stub(
                vec![package("dpkg", "leaf", None, &[])],
                Some(simulation(&["leaf"], &[])),
                false,
            ),
            "leaf",
        );
        let plan = leaf.removal.unwrap();
        assert!(plan.blockers.is_empty());
        assert_eq!(plan.simulation, Some(simulation(&["leaf"], &[])));

        let semanage = show(
            stub(
                vec![package("dpkg", "libsemanage2", None, &[]), passwd.clone()],
                Some(simulation(&["libsemanage2", "passwd"], &[])),
                false,
            ),
            "libsemanage2",
        );
        assert_eq!(
            semanage.removal.unwrap().blockers,
            ["it would also remove passwd, which is protected: its priority is required"]
        );

        let refused = show(
            stub(
                vec![package("dpkg", "gzip", None, &[])],
                Some(simulation(
                    &[],
                    &["removing gzip breaks dependency 'gzip' required by base"],
                )),
                false,
            ),
            "gzip",
        );
        assert_eq!(
            refused.removal.unwrap().blockers,
            ["the package manager refuses: removing gzip breaks dependency 'gzip' required by base"]
        );

        let protected = show(stub(vec![passwd], None, true), "passwd");
        assert_eq!(
            protected.removal.unwrap().simulation,
            None,
            "a protected package is not simulated, so its failing simulation is never run"
        );
        assert!(protected.warnings.is_empty(), "{:?}", protected.warnings);

        let failing = show(stub(vec![package("dpkg", "leaf", None, &[])], None, true), "leaf");
        let plan = failing.removal.unwrap();
        assert_eq!((plan.simulation, plan.blockers.len()), (None, 0));
        assert!(
            failing
                .warnings
                .iter()
                .any(|w| w.contains("could not simulate the removal of leaf")),
            "{:?}",
            failing.warnings
        );
    }
}
