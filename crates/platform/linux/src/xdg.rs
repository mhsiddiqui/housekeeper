//! The freedesktop.org base directories that desktop entries and AppStream metainfo live under.

use std::path::PathBuf;

use housekeeper_core::Host;

/// `$XDG_DATA_DIRS` when it is not set.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share/:/usr/share/";

/// `$XDG_DATA_HOME`, then each `$XDG_DATA_DIRS` entry, most important first, without repeats.
pub(crate) fn data_dirs(host: &Host) -> Vec<PathBuf> {
    let data_dirs = host.var("XDG_DATA_DIRS").unwrap_or(DEFAULT_DATA_DIRS);
    let mut dirs: Vec<PathBuf> = vec![host.data_home()];
    for dir in data_dirs
        .split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(PathBuf::from)
    {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}
