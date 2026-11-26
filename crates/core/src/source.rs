//! The extension point for every way software gets installed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use crate::command::RunError;
use crate::host::Host;
use crate::leftovers::LeftoverKind;
use crate::model::{Availability, Package, SourceInfo};
use crate::remove::Remover;

/// One way software gets installed: a package manager, an app format, a language tool, or manual installs.
///
/// Sources only read. Abilities only some sources have are separate traits a source offers as optional components
/// (`owner_lookup`, `file_list`, `dependant_lookup` and `remover` today; installing and updating in later phases), so a
/// new ability never forces every source to implement or stub it.
pub trait Source: Send + Sync {
    fn info(&self) -> SourceInfo;

    /// Whether the source exists on this machine. Must be cheap: no package database reads.
    fn probe(&self, host: &Host) -> Availability;

    /// Everything this source has installed. Only called when `probe` reported the source available.
    fn inventory(&self, host: &Host, scan: &ScanContext<'_>) -> Result<Vec<Package>, SourceError>;

    /// "Which of these files did you install?", for sources that track individual files (package managers).
    fn owner_lookup(&self) -> Option<&dyn OwnerLookup> {
        None
    }

    /// "Which files did this package install?", for sources that record them (package managers, pip's `RECORD`).
    /// Without it, the files of a package are taken to be everything in its install location.
    fn file_list(&self) -> Option<&dyn FileList> {
        None
    }

    /// "Which installed packages need this one?", for sources that record dependencies.
    fn dependant_lookup(&self) -> Option<&dyn Dependants> {
        None
    }

    /// "How is this package removed?", for sources that know (decision D8). Without it, nothing records how its
    /// packages were installed. It only plans; nothing is removed until Phase 4.
    fn remover(&self) -> Option<&dyn Remover> {
        None
    }

    /// Directories this source installs programs into (logical, absolute). A program on the PATH that resolves into
    /// one of them is this source's, so a source that lists what nobody else installed (hand installs) leaves it out.
    fn install_roots(&self, _host: &Host) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Per-user folders only this source's items keep their state in, one subfolder per item, with what they hold.
    /// Removing an item leaves its subfolder behind, so it is one of the item's leftovers; other sources' items are
    /// never matched against these folders (D36).
    fn leftover_roots(&self, _host: &Host) -> Vec<(PathBuf, LeftoverKind)> {
        Vec::new()
    }
}

/// What a source sees while it is read: every source of the platform, and a place for problems that did not stop it.
///
/// Sources use each other only through this, at scan time; they never hold one another.
#[derive(Default)]
pub struct ScanContext<'a> {
    sources: &'a [Arc<dyn Source>],
    warnings: Mutex<Vec<String>>,
}

impl<'a> ScanContext<'a> {
    /// `sources` are all the platform's sources, including ones this scan does not read.
    pub fn new(sources: &'a [Arc<dyn Source>]) -> Self {
        Self {
            sources,
            warnings: Mutex::default(),
        }
    }

    pub fn sources(&self) -> impl Iterator<Item = &'a dyn Source> {
        self.sources.iter().map(|source| source.as_ref())
    }

    /// Reports something the user should know about a result that is still shown (it may be incomplete).
    pub fn warn(&self, message: impl Into<String>) {
        self.warnings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message.into());
    }

    pub fn into_warnings(self) -> Vec<String> {
        self.warnings.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Ability: tell which installed package owns a path.
pub trait OwnerLookup: Send + Sync {
    /// The packages that own each of `paths` (logical, absolute); paths nobody owns are absent. Each name is the
    /// owning package's `Package::id` or `Package::name` in this source's inventory. Exact matches only.
    /// Implementations ask about all paths in one call.
    fn owners(&self, host: &Host, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<String>>, SourceError>;
}

/// Ability: list the files a package installed.
pub trait FileList: Send + Sync {
    /// The files and directories `package` (one of this source's) installed, as logical absolute paths, in the
    /// source's order. Paths that no longer exist on disk are included; callers check.
    fn files(&self, host: &Host, package: &Package) -> Result<Vec<PathBuf>, SourceError>;
}

/// Ability: tell which installed packages directly need a package.
pub trait Dependants: Send + Sync {
    /// The `Package::id`s of this source's installed packages that directly depend on `package` (one of this
    /// source's), through its name or anything it provides, sorted and without repeats. Optional dependencies
    /// (recommends, suggests) do not count.
    fn dependants(&self, host: &Host, package: &Package) -> Result<Vec<String>, SourceError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error(transparent)]
    Run(#[from] RunError),
    #[error("`{command}` failed{}: {stderr}", code.map(|c| format!(" with exit code {c}")).unwrap_or_default())]
    CommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    #[error("could not read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unexpected data in {what}: {detail}")]
    Parse { what: String, detail: String },
}

impl SourceError {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub fn parse(what: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Parse {
            what: what.into(),
            detail: detail.into(),
        }
    }
}
