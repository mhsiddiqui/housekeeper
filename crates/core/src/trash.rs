//! Moving files and folders to the freedesktop Trash, never deleting them (decisions D7, D43). The user's file manager
//! lists what was moved there and can put it back.
//!
//! Follows the freedesktop Trash specification for the home trash, `$XDG_DATA_HOME/Trash`: the item goes to `files/`
//! under a name nothing else there has, and `info/<name>.trashinfo` records where it came from and when. Items are
//! moved with a rename, so only items on the home trash's file system can go; anything else is refused, never copied
//! and deleted.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::source::SourceError;
use crate::text::utc_timestamp;

/// Where an item went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trashed {
    /// Where it was.
    pub from: PathBuf,
    /// Where it is now, in the Trash's `files` folder.
    pub to: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum TrashError {
    #[error("{0} is not there")]
    Missing(PathBuf),
    #[error("{0} is on another file system than the Trash; it was left where it is")]
    OtherFileSystem(PathBuf),
    #[error("{path} was not moved: {reason}")]
    Changed { path: PathBuf, reason: String },
    #[error(transparent)]
    Io(#[from] SourceError),
}

/// Names tried for one item before giving up: `name`, `name.2`, … `name.1000`.
const MAX_NAMES: usize = 1000;

/// Moves `path` (logical, absolute) to the home trash of `host`'s user, at `now` (seconds since the Unix epoch).
pub fn trash(host: &Host, path: &Path, now: i64) -> Result<Trashed, TrashError> {
    if host.symlink_metadata(path).is_none() {
        return Err(TrashError::Missing(path.to_path_buf()));
    }
    let name = path
        .file_name()
        .ok_or_else(|| TrashError::Missing(path.to_path_buf()))?
        .to_string_lossy()
        .into_owned();
    let trash = host.data_home().join("Trash");
    let info = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        escape(path),
        utc_timestamp(now)
    );
    for n in 1..=MAX_NAMES {
        let candidate = if n == 1 { name.clone() } else { format!("{name}.{n}") };
        let info_file = trash.join("info").join(format!("{candidate}.trashinfo"));
        let target = trash.join("files").join(&candidate);
        // The info file claims the name (the specification's order): it is created only if no other has it.
        if !host.create_new(&info_file, info.as_bytes())? {
            continue;
        }
        if host.symlink_metadata(&target).is_some() {
            host.remove_file(&info_file)?;
            continue;
        }
        if let Some(files) = target.parent() {
            // `create_new` made `info/`; `files/` may not exist yet.
            host.create_dir_all(files)?;
        }
        return match host.rename(path, &target) {
            Ok(()) => Ok(Trashed {
                from: path.to_path_buf(),
                to: target,
            }),
            Err(e) => {
                let _ = host.remove_file(&info_file);
                if e.kind() == io::ErrorKind::CrossesDevices {
                    Err(TrashError::OtherFileSystem(path.to_path_buf()))
                } else {
                    Err(SourceError::io(path, e).into())
                }
            }
        };
    }
    Err(TrashError::Changed {
        path: path.to_path_buf(),
        reason: format!("the Trash already holds {MAX_NAMES} items named {name}"),
    })
}

/// `path` as the `Path=` of a `.trashinfo`: percent-encoded like a URL path, `/` kept.
fn escape(path: &Path) -> String {
    let mut out = String::new();
    for byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(char::from(*byte)),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, Fixture, HOME};

    const NOW: i64 = 1_794_206_843;

    fn home(path: &str) -> String {
        format!("{HOME}/{path}")
    }

    #[test]
    fn an_item_moves_to_the_trash_with_where_it_came_from() {
        let fixture = Fixture::new();
        fixture.file(&home(".cache/My Tool/blob"), "x");
        let host = fixture.host(FakeRunner::new());
        let trashed = trash(&host, Path::new(&home(".cache/My Tool")), NOW).unwrap();
        assert_eq!(trashed.to, Path::new(&home(".local/share/Trash/files/My Tool")));
        assert!(!host.exists(home(".cache/My Tool")), "moved, not copied");
        assert_eq!(
            host.read_to_string(home(".local/share/Trash/files/My Tool/blob"))
                .unwrap(),
            "x"
        );
        assert_eq!(
            host.read_to_string(home(".local/share/Trash/info/My Tool.trashinfo"))
                .unwrap(),
            "[Trash Info]\nPath=/home/user/.cache/My%20Tool\nDeletionDate=2026-11-09T06:47:23\n"
        );
    }

    #[test]
    fn a_name_the_trash_already_has_gets_a_number() {
        let fixture = Fixture::new();
        fixture
            .file(&home(".cache/tool/a"), "new")
            .file(&home(".local/share/Trash/files/tool/a"), "old")
            .file(&home(".local/share/Trash/info/tool.trashinfo"), "[Trash Info]\n")
            // A leftover info file without its item still holds the name.
            .file(&home(".local/share/Trash/info/tool.2.trashinfo"), "[Trash Info]\n");
        let host = fixture.host(FakeRunner::new());
        let trashed = trash(&host, Path::new(&home(".cache/tool")), NOW).unwrap();
        assert_eq!(trashed.to, Path::new(&home(".local/share/Trash/files/tool.3")));
        assert_eq!(
            host.read_to_string(home(".local/share/Trash/files/tool/a")).unwrap(),
            "old",
            "nothing in the Trash is overwritten"
        );
    }

    #[test]
    fn a_missing_item_is_an_error_and_leaves_no_info_file() {
        let fixture = Fixture::new();
        let host = fixture.host(FakeRunner::new());
        assert!(matches!(
            trash(&host, Path::new(&home(".cache/gone")), NOW),
            Err(TrashError::Missing(_))
        ));
        assert!(!host.exists(home(".local/share/Trash/info/gone.trashinfo")));
    }
}
