//! Stub sources and a stub platform for core's own tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::host::Host;
use crate::inventory::Housekeeper;
use crate::leftovers::LeftoverKind;
use crate::model::{Availability, Launcher, Metainfo, Package, Scope, SourceId, SourceInfo, SourceKind, SystemInfo};
use crate::platform::Platform;
use crate::remove::{RemovalPlan, Remover, Simulation, Simulator};
use crate::source::{Dependants, FileList, OwnerLookup, ScanContext, Source, SourceError};
use crate::testing::{FakeRunner, Fixture, HOME};

/// A source with fixed packages, install folders and package-database files.
#[derive(Default)]
pub struct Stub {
    pub id: &'static str,
    pub packages: Vec<Package>,
    pub roots: Vec<&'static str>,
    /// `(path, package)` pairs its package database lists.
    pub files: Vec<(&'static str, &'static str)>,
    pub broken: bool,
    /// Its package database cannot be asked who owns files.
    pub lookup_fails: bool,
    /// `(dependant, package)` pairs: the first needs the second (both package names).
    pub needs: Vec<(&'static str, &'static str)>,
    /// What its remover plans for every package; `None` with `plan_fails` false means it has no remover.
    pub plan: Option<RemovalPlan>,
    /// Its remover cannot make a plan.
    pub plan_fails: bool,
    /// What its remover's simulation answers for every package; `None` with `simulation_fails` false means it cannot
    /// simulate.
    pub simulation: Option<Simulation>,
    /// Its simulation fails.
    pub simulation_fails: bool,
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
        (!self.files.is_empty() || self.lookup_fails).then_some(self as &dyn OwnerLookup)
    }
    fn file_list(&self) -> Option<&dyn FileList> {
        (!self.files.is_empty()).then_some(self as &dyn FileList)
    }
    fn dependant_lookup(&self) -> Option<&dyn Dependants> {
        (!self.needs.is_empty()).then_some(self as &dyn Dependants)
    }
    fn remover(&self) -> Option<&dyn Remover> {
        (self.plan.is_some() || self.plan_fails).then_some(self as &dyn Remover)
    }
    fn install_roots(&self, _: &Host) -> Vec<PathBuf> {
        self.roots.iter().map(PathBuf::from).collect()
    }
}

impl OwnerLookup for Stub {
    fn owners(&self, _: &Host, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<String>>, SourceError> {
        let mut owners: HashMap<PathBuf, Vec<String>> = HashMap::new();
        if self.lookup_fails {
            return Err(SourceError::parse("ownership query", "garbage"));
        }
        for (path, package) in &self.files {
            if paths.iter().any(|asked| asked == Path::new(path)) {
                owners.entry(PathBuf::from(path)).or_default().push(package.to_string());
            }
        }
        Ok(owners)
    }
}

impl FileList for Stub {
    fn files(&self, _: &Host, package: &Package) -> Result<Vec<PathBuf>, SourceError> {
        Ok(self
            .files
            .iter()
            .filter(|(_, owner)| *owner == package.name)
            .map(|(path, _)| PathBuf::from(path))
            .collect())
    }
}

impl Dependants for Stub {
    fn dependants(&self, _: &Host, package: &Package) -> Result<Vec<String>, SourceError> {
        Ok(self
            .needs
            .iter()
            .filter(|(_, needed)| *needed == package.name)
            .filter_map(|(dependant, _)| self.packages.iter().find(|p| p.name == *dependant))
            .map(|p| p.id.clone())
            .collect())
    }
}

impl Remover for Stub {
    fn plan(&self, _: &Host, _: &Package) -> Result<RemovalPlan, SourceError> {
        match (&self.plan, self.plan_fails) {
            (Some(plan), false) => Ok(plan.clone()),
            _ => Err(SourceError::parse("removal plan", "garbage")),
        }
    }
    fn simulator(&self) -> Option<&dyn Simulator> {
        (self.simulation.is_some() || self.simulation_fails).then_some(self as &dyn Simulator)
    }
}

impl Simulator for Stub {
    fn simulate(&self, _: &Host, _: &Package) -> Result<Simulation, SourceError> {
        match (&self.simulation, self.simulation_fails) {
            (Some(simulation), false) => Ok(simulation.clone()),
            _ => Err(SourceError::parse("simulation", "garbage")),
        }
    }
}

struct TestPlatform {
    system: SystemInfo,
    sources: Vec<Arc<dyn Source>>,
    launchers: Vec<Launcher>,
    metainfo: Vec<Metainfo>,
}

impl Platform for TestPlatform {
    fn system(&self) -> &SystemInfo {
        &self.system
    }
    fn sources(&self) -> &[Arc<dyn Source>] {
        &self.sources
    }
    fn launchers(&self, _: &Host, _: &ScanContext<'_>) -> Vec<Launcher> {
        self.launchers.clone()
    }
    fn metainfo(&self, _: &Host, _: &ScanContext<'_>) -> Vec<Metainfo> {
        self.metainfo.clone()
    }
    fn leftover_roots(&self, _: &Host) -> Vec<(PathBuf, LeftoverKind)> {
        [
            (".config", LeftoverKind::Config),
            (".cache", LeftoverKind::Cache),
            (".local/share", LeftoverKind::Data),
        ]
        .into_iter()
        .map(|(dir, kind)| (Path::new(HOME).join(dir), kind))
        .collect()
    }
    fn shell_files(&self, _: &Host) -> Vec<PathBuf> {
        vec![Path::new(HOME).join(".bashrc")]
    }
}

/// `path` is the `PATH`, which is also where commands are looked for.
pub fn housekeeper(fixture: &Fixture, path: &str, sources: Vec<Stub>) -> Housekeeper {
    housekeeper_with(fixture, path, sources, Vec::new(), Vec::new())
}

/// Like [`housekeeper`], with a menu and catalog entries.
pub fn housekeeper_with(
    fixture: &Fixture,
    path: &str,
    sources: Vec<Stub>,
    launchers: Vec<Launcher>,
    metainfo: Vec<Metainfo>,
) -> Housekeeper {
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
    Housekeeper::new(
        Box::new(TestPlatform {
            system,
            sources,
            launchers,
            metainfo,
        }),
        host,
    )
}

pub fn package(source: &'static str, name: &str, location: Option<&str>, commands: &[&str]) -> Package {
    let mut package = Package::new(SourceId::new(source), name, name, Scope::User);
    package.location = location.map(PathBuf::from);
    package.commands = commands.iter().map(|c| c.to_string()).collect();
    package
}
