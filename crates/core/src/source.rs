//! The extension point for every way software gets installed.

use std::path::{Path, PathBuf};

use crate::command::RunError;
use crate::host::Host;
use crate::model::{Availability, Package, SourceInfo};

/// One way software gets installed: a package manager, an app format, a language tool, or manual installs.
///
/// Sources only read. Capabilities that change the system (removing, installing, updating) are separate traits a
/// source exposes as optional components, added in later phases.
pub trait Source: Send + Sync {
    fn info(&self) -> SourceInfo;

    /// Whether the source exists on this machine. Must be cheap: no package database reads.
    fn probe(&self, host: &Host) -> Availability;

    /// Everything this source has installed. Only called when `probe` reported the source available.
    fn inventory(&self, host: &Host) -> Result<Vec<Package>, SourceError>;
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
