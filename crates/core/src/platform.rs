//! An operating system: what it is and which sources it has.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::helper::{PackageRemoval, RootHelper};
use crate::host::Host;
use crate::inspect::Recognised;
use crate::leftovers::LeftoverKind;
use crate::model::{Launcher, Metainfo, SystemInfo};
use crate::queue::PackageLock;
use crate::source::{ScanContext, Source, SourceError};

/// Implemented once per operating system (`LinuxPlatform`, later macOS and Windows).
pub trait Platform: Send + Sync {
    fn system(&self) -> &SystemInfo;

    /// Every source that applies to this system, in display order. Sources still probe for availability.
    /// Shared (`Arc`) so a source can be given another one to work with, such as the native package manager.
    fn sources(&self) -> &[Arc<dyn Source>];

    /// Every entry of the application menu, as the menu sees it: one per id, the one that takes precedence.
    /// Entries that could not be read are reported through `scan.warn` and left out.
    fn launchers(&self, _host: &Host, _scan: &ScanContext<'_>) -> Vec<Launcher> {
        Vec::new()
    }

    /// Every app catalog entry installed on disk. Entries that could not be read are reported through `scan.warn`
    /// and left out.
    fn metainfo(&self, _host: &Host, _scan: &ScanContext<'_>) -> Vec<Metainfo> {
        Vec::new()
    }

    /// Directories whose programs are commands (logical, absolute): the `PATH`, and on systems that have them the
    /// standard program directories a session's `PATH` sometimes lacks.
    fn command_dirs(&self, host: &Host) -> Vec<PathBuf> {
        path_dirs(host)
    }

    /// The per-user folders programs keep their state in, one subfolder per program, with what they hold
    /// (`~/.config` holds settings). A subfolder named after an item outside a sandbox is one of its leftovers; a
    /// source's own such folders come from [`Source::leftover_roots`].
    fn leftover_roots(&self, _host: &Host) -> Vec<(PathBuf, LeftoverKind)> {
        Vec::new()
    }

    /// Names (lowercase) of subfolders of the leftover roots that several programs share (`fonts`), so never one
    /// item's leftover even when an item carries the name.
    fn shared_state_folders(&self) -> &'static [&'static str] {
        &[]
    }

    /// How the system's package tool removes packages, for the root helper (D25); `None` when the system has no tool
    /// Housekeeper can use.
    fn package_removal(&self) -> Option<&dyn PackageRemoval> {
        None
    }

    /// The lock of `backend` (a package tool's name, `apt`) that changes must wait for while another program holds it
    /// (D6); `None` when the backend has none Housekeeper can see.
    fn package_lock(&self, _backend: &str) -> Option<&dyn PackageLock> {
        None
    }

    /// How front ends reach the root helper (D4, D40); `None` when the system has none.
    fn root_helper(&self) -> Option<&dyn RootHelper> {
        None
    }

    /// Folders the system is made of (`/usr`, `/etc`): the root helper never trashes one of them, nor a folder that
    /// holds one.
    fn system_paths(&self) -> &'static [&'static str] {
        &[]
    }

    /// Folders that hold users' own files (`/home`): the root helper never trashes anything inside them, since what is
    /// there is removed by its user.
    fn home_paths(&self) -> &'static [&'static str] {
        &[]
    }

    /// The user's shell startup files (logical paths), whose lines can load an installed tool.
    fn shell_files(&self, _host: &Host) -> Vec<PathBuf> {
        Vec::new()
    }

    /// What an installer file is (`path` logical, absolute, a regular file), told from its name and first bytes, with
    /// what is inside as far as the file or its format's own tool says (D45); `Ok(None)` when it has no installer
    /// format the platform knows. Nothing in the file is run.
    fn recognise_file(&self, _host: &Host, _path: &Path) -> Result<Option<Recognised>, SourceError> {
        Ok(None)
    }
}

/// The absolute directories of the `PATH`, in order, without repeats.
pub fn path_dirs(host: &Host) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in host.var("PATH").unwrap_or_default().split(':') {
        let dir = PathBuf::from(dir);
        if dir.starts_with("/") && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}
