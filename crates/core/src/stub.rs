//! Stub sources and a stub platform for core's own tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::host::Host;
use crate::inventory::Housekeeper;
use crate::model::{Availability, Launcher, Metainfo, Package, Scope, SourceId, SourceInfo, SourceKind, SystemInfo};
use crate::platform::Platform;
use crate::source::{OwnerLookup, ScanContext, Source, SourceError};
use crate::testing::{FakeRunner, Fixture};

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
