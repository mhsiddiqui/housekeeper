//! Shared plumbing for native package managers' "who owns these paths?" queries ([`OwnerLookup`]).
//!
//! Each native source knows its own command and output format; this runs the one batched call and keeps only exact
//! matches.
//!
//! [`OwnerLookup`]: housekeeper_core::OwnerLookup

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use housekeeper_core::{Cmd, Host, Output, SourceError};

/// A native package manager's ownership query.
pub(crate) struct OwnerQuery {
    /// The command for these paths (all of them, in one call).
    pub command: fn(&[PathBuf]) -> Cmd,
    /// Unowned paths make package managers exit non-zero, so stdout is authoritative unless this says the run failed.
    pub failed: fn(&Output) -> bool,
    /// Owned paths from stdout, limited to `wanted`.
    pub parse: fn(&str, &HashSet<&Path>) -> HashSet<PathBuf>,
}

impl OwnerQuery {
    pub fn run(&self, host: &Host, paths: &[PathBuf]) -> Result<HashSet<PathBuf>, SourceError> {
        if paths.is_empty() {
            return Ok(HashSet::new());
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
        Ok((self.parse)(&output.stdout, &wanted))
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

    pub fn sorted(set: HashSet<PathBuf>) -> Vec<PathBuf> {
        let mut list: Vec<_> = set.into_iter().collect();
        list.sort();
        list
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
            parse: |_, _| HashSet::new(),
        };
        let host = Fixture::new().host(FakeRunner::new());
        assert!(query.run(&host, &[]).unwrap().is_empty());
    }
}
