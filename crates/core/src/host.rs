//! The machine sources read from.
//!
//! Every path a source works with is a *logical* absolute path (`/var/lib/dpkg/status`, `/home/ana/.cargo`).
//! `Host` maps it onto the real file system under its root, which is `/` normally and a fixture directory in tests.
//! Paths a source reports back (`Package::location`) are therefore always logical.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::command::{Cmd, CommandRunner, Output, RunError, SystemRunner};
use crate::source::SourceError;

/// A directory entry, with a logical path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    /// What the entry is, following symlinks; `None` for a dangling symlink.
    pub file_type: Option<FileType>,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File { executable: bool },
    Dir,
    Other,
}

pub struct Host {
    root: PathBuf,
    home: PathBuf,
    vars: HashMap<String, String>,
    runner: Arc<dyn CommandRunner>,
}

impl Host {
    /// The machine this process runs on, as the current user.
    pub fn local() -> Self {
        let vars: HashMap<String, String> = std::env::vars().collect();
        let home = vars.get("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let runner = SystemRunner::new(vars.get("PATH").map(String::as_str));
        Self::new("/", home, vars, Arc::new(runner))
    }

    pub fn new(
        root: impl Into<PathBuf>,
        home: impl Into<PathBuf>,
        vars: HashMap<String, String>,
        runner: Arc<dyn CommandRunner>,
    ) -> Self {
        Self {
            root: root.into(),
            home: home.into(),
            vars,
            runner,
        }
    }

    /// The user's home directory (logical path).
    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    /// A directory from an environment variable if set to an absolute path, otherwise `home/fallback`.
    pub fn dir_from_var(&self, var: &str, fallback: &str) -> PathBuf {
        match self.var(var) {
            Some(value) if value.starts_with('/') => PathBuf::from(value),
            _ => self.home.join(fallback),
        }
    }

    /// `$XDG_DATA_HOME`, defaulting to `~/.local/share`.
    pub fn data_home(&self) -> PathBuf {
        self.dir_from_var("XDG_DATA_HOME", ".local/share")
    }

    /// Real path of a logical path. Only for code that must hand a path to the OS (sockets, mmap).
    pub fn real_path(&self, logical: impl AsRef<Path>) -> PathBuf {
        let relative: PathBuf = logical
            .as_ref()
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .collect();
        self.root.join(relative)
    }

    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        self.resolve(path).exists()
    }

    pub fn is_dir(&self, path: impl AsRef<Path>) -> bool {
        self.resolve(path).is_dir()
    }

    pub fn read_to_string(&self, path: impl AsRef<Path>) -> Result<String, SourceError> {
        let path = path.as_ref();
        fs::read_to_string(self.resolve(path)).map_err(|e| SourceError::io(path, e))
    }

    /// Like `read_to_string`, but a missing file is `Ok(None)`.
    pub fn read_optional(&self, path: impl AsRef<Path>) -> Result<Option<String>, SourceError> {
        let path = path.as_ref();
        match fs::read_to_string(self.resolve(path)) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(SourceError::io(path, e)),
        }
    }

    /// First `len` bytes of a file (fewer if the file is shorter).
    pub fn read_head(&self, path: impl AsRef<Path>, len: usize) -> Result<Vec<u8>, SourceError> {
        let path = path.as_ref();
        let file = fs::File::open(self.resolve(path)).map_err(|e| SourceError::io(path, e))?;
        let mut head = Vec::with_capacity(len);
        file.take(len as u64)
            .read_to_end(&mut head)
            .map_err(|e| SourceError::io(path, e))?;
        Ok(head)
    }

    /// Real path of a logical path, following a symlink in the last component within the root, so absolute link
    /// targets do not escape it.
    fn resolve(&self, path: impl AsRef<Path>) -> PathBuf {
        let mut path = path.as_ref().to_path_buf();
        for _ in 0..16 {
            match self.link_target(&path) {
                Some(target) => path = target,
                None => break,
            }
        }
        self.real_path(path)
    }

    /// Entries of a directory, sorted by name. A missing directory has no entries.
    pub fn read_dir(&self, dir: impl AsRef<Path>) -> Result<Vec<Entry>, SourceError> {
        let dir = dir.as_ref();
        let reader = match fs::read_dir(self.resolve(dir)) {
            Ok(reader) => reader,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(SourceError::io(dir, e)),
        };
        let mut entries = Vec::new();
        for entry in reader {
            let entry = entry.map_err(|e| SourceError::io(dir, e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_symlink = entry.file_type().is_ok_and(|t| t.is_symlink());
            let file_type = self.metadata(dir.join(&name)).map(|meta| file_type_of(&meta));
            entries.push(Entry {
                path: dir.join(&name),
                name,
                file_type,
                is_symlink,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Where a symlink points, as a logical absolute path. `None` if `path` is not a symlink.
    pub fn link_target(&self, path: impl AsRef<Path>) -> Option<PathBuf> {
        let path = path.as_ref();
        let target = fs::read_link(self.real_path(path)).ok()?;
        let joined = if target.is_absolute() {
            target
        } else {
            path.parent().unwrap_or(Path::new("/")).join(target)
        };
        Some(normalize(&joined))
    }

    /// Size, type and modification time, following symlinks within the root.
    pub fn metadata(&self, path: impl AsRef<Path>) -> Option<fs::Metadata> {
        fs::metadata(self.resolve(path)).ok()
    }

    pub fn has_program(&self, name: &str) -> bool {
        self.runner.find_program(name).is_some()
    }

    pub fn run(&self, cmd: &Cmd) -> Result<Output, RunError> {
        self.runner.run(cmd)
    }

    /// Runs a command that must succeed and returns its standard output.
    pub fn run_ok(&self, cmd: &Cmd) -> Result<String, SourceError> {
        let output = self.runner.run(cmd)?;
        if output.success() {
            Ok(output.stdout)
        } else {
            Err(SourceError::CommandFailed {
                command: cmd.to_string(),
                code: output.code,
                stderr: output.stderr.trim().to_string(),
            })
        }
    }
}

fn file_type_of(meta: &fs::Metadata) -> FileType {
    if meta.is_dir() {
        FileType::Dir
    } else if meta.is_file() {
        FileType::File {
            executable: is_executable(meta),
        }
    } else {
        FileType::Other
    }
}

/// Whether a file may be run: any of the Unix execute bits is set.
#[cfg(unix)]
pub(crate) fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

/// Whether a file may be run. Systems without execute bits have nothing finer to check, so every file counts.
#[cfg(not(unix))]
pub(crate) fn is_executable(meta: &fs::Metadata) -> bool {
    meta.is_file()
}

/// Resolves `.` and `..` without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, Fixture};

    #[test]
    fn logical_paths_resolve_under_the_root() {
        let fixture = Fixture::new();
        fixture.file("/etc/os-release", "ID=test\n");
        let host = fixture.host(FakeRunner::new());
        assert_eq!(host.read_to_string("/etc/os-release").unwrap(), "ID=test\n");
        assert!(!host.exists("/etc/missing"));
        assert_eq!(host.read_optional("/etc/missing").unwrap(), None);
    }

    #[test]
    fn symlink_targets_are_logical_and_normalized() {
        let fixture = Fixture::new();
        fixture.file("/home/u/tools/pg/bin/psql", "");
        fixture.symlink("/home/u/.local/bin/psql", "../../tools/pg/bin/psql");
        fixture.symlink("/home/u/.local/bin/abs", "/home/u/tools/pg/bin/psql");
        let host = fixture.host(FakeRunner::new());
        assert_eq!(
            host.link_target("/home/u/.local/bin/psql").unwrap(),
            PathBuf::from("/home/u/tools/pg/bin/psql")
        );
        assert_eq!(
            host.link_target("/home/u/.local/bin/abs").unwrap(),
            PathBuf::from("/home/u/tools/pg/bin/psql")
        );
        let entries = host.read_dir("/home/u/.local/bin").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.is_symlink));
    }

    #[test]
    fn missing_directory_has_no_entries() {
        let host = Fixture::new().host(FakeRunner::new());
        assert!(host.read_dir("/opt").unwrap().is_empty());
    }
}
