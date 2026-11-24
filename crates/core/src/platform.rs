//! An operating system: what it is and which sources it has.

use std::path::PathBuf;
use std::sync::Arc;

use crate::host::Host;
use crate::model::{Launcher, SystemInfo};
use crate::source::{ScanContext, Source};

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

    /// Directories whose programs are commands (logical, absolute): the `PATH`, and on systems that have them the
    /// standard program directories a session's `PATH` sometimes lacks.
    fn command_dirs(&self, host: &Host) -> Vec<PathBuf> {
        path_dirs(host)
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
