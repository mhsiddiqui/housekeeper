//! Shared plumbing for native package managers' "who owns these paths?" queries ([`OwnerLookup`]).
//!
//! Each native source knows its own command and output format; this runs the one batched call and keeps only exact
//! matches, with the packages that own them.
//!
//! [`OwnerLookup`]: housekeeper_core::OwnerLookup

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use housekeeper_core::{Cmd, Host, Output, SourceError};

/// A native package manager's ownership query.
pub(crate) struct OwnerQuery {
    /// The command for these paths (all of them, in one call).
    pub command: fn(&[PathBuf]) -> Cmd,
    /// Unowned paths make package managers exit non-zero, so stdout is authoritative unless this says the run failed.
    pub failed: fn(&Output) -> bool,
    /// `(path, owning package)` pairs from stdout. Lines about paths nobody asked for are dropped afterwards.
    pub parse: fn(&str) -> Vec<(&str, &str)>,
}

impl OwnerQuery {
    pub fn run(&self, host: &Host, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<String>>, SourceError> {
        let mut owners: HashMap<PathBuf, Vec<String>> = HashMap::new();
        if paths.is_empty() {
            return Ok(owners);
        }
        let wanted: HashSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let cmd = (self.command)(paths);
        let output = host.run(&cmd)?;
        if (self.failed)(&output) {
            return Err(SourceError::CommandFailed {
                command: cmd.to_string(),
                code: output.code,
                stderr: output.stderr.trim().to_string(),
            });
        }
        for (path, package) in (self.parse)(&output.stdout) {
            let path = Path::new(path);
            if !wanted.contains(path) {
                continue;
            }
            let names = owners.entry(path.to_path_buf()).or_default();
            if !names.iter().any(|name| name == package) {
                names.push(package.to_string());
            }
        }
        Ok(owners)
    }
}

/// Paths as command arguments.
pub(crate) fn args(paths: &[PathBuf]) -> impl Iterator<Item = String> + '_ {
    paths.iter().map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    pub fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    /// Owners sorted by path, for comparing with expected values.
    pub fn sorted(owners: HashMap<PathBuf, Vec<String>>) -> Vec<(PathBuf, Vec<String>)> {
        let mut list: Vec<_> = owners.into_iter().collect();
        list.sort();
        list
    }

    pub fn owned(path: &str, packages: &[&str]) -> (PathBuf, Vec<String>) {
        (PathBuf::from(path), packages.iter().map(|p| p.to_string()).collect())
    }

    pub fn exit(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            code: Some(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn nothing_to_ask_runs_nothing() {
        let query = OwnerQuery {
            command: |_| Cmd::new("never-run"),
            failed: |_| true,
            parse: |_| Vec::new(),
        };
        let host = Fixture::new().host(FakeRunner::new());
        assert!(query.run(&host, &[]).unwrap().is_empty());
    }
}
