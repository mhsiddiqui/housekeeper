//! "What is this file?": a file someone wants to install, as the platform recognises it, and whether it is made for
//! this system (decision D45).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::host::FileType;
use crate::inventory::Housekeeper;
use crate::model::{Availability, SourceId, SourceInfo};
use crate::source::SourceError;

/// A kind of file that installs software, as the platform names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatInfo {
    /// Stable key, `deb`.
    pub id: String,
    /// For people, `Debian package`.
    pub title: String,
}

/// What the platform recognised in a file, before Housekeeper checks it against this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recognised {
    pub format: FormatInfo,
    /// How the format was told, for people: "an `ar` archive whose first member is `debian-binary`".
    pub evidence: String,
    /// The source that lists what the file installs, on this system. `None` for another system's own package format.
    pub source: Option<SourceId>,
    /// The groups of systems (`SystemInfo::family`) whose own package format this is, when it is not this system's
    /// (`debian` for a `.deb` on Fedora). Empty otherwise.
    pub made_for: Vec<String>,
    /// What is inside, as far as it can be read here.
    pub package: Option<FilePackage>,
    /// Problems reading what is inside; the format is still known.
    pub warnings: Vec<String>,
}

/// The software inside an installer file, as far as the file or its format's own tool says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePackage {
    pub name: Option<String>,
    pub version: Option<String>,
    /// A display name, when the file has one besides its package name (a `.flatpakref`'s `Title`).
    pub title: Option<String>,
    /// The processor it is built for, named like `SystemInfo::arch` (`x86_64`); `None` when it runs on any or the
    /// file does not say.
    pub arch: Option<String>,
    /// Where it would be downloaded from, for a file that only points there (a `.flatpakref`'s repository).
    pub origin: Option<String>,
}

/// Whether the file is made for this system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Fit {
    /// Made for this system and processor, and what installs it is here.
    Fits,
    /// Another system's own package format: a `.deb` on Fedora.
    OtherSystem { families: Vec<String> },
    /// What installs the format is not on this system: a `.snap` without snapd.
    SourceMissing { reason: String },
    /// Built for another processor.
    OtherArch { arch: String },
}

/// A file someone wants to install: what it is, what would list it once installed, and whether it fits here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inspection {
    pub path: PathBuf,
    pub format: FormatInfo,
    pub evidence: String,
    /// The source that would list it once installed; `None` for another system's own package format.
    pub source: Option<SourceInfo>,
    pub package: Option<FilePackage>,
    pub fit: Fit,
    pub warnings: Vec<String>,
}

/// Why a file could not be inspected.
#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    #[error("no file at {}", .0.display())]
    Missing(PathBuf),
    #[error("{} is not a file", .0.display())]
    NotAFile(PathBuf),
    #[error(transparent)]
    Read(#[from] SourceError),
}

impl Housekeeper {
    /// What the file at `path` (logical, absolute) is, and whether it is made for this system: its own package
    /// format, the processor it is built for, and the source that installs it being here, in that order. `Ok(None)`
    /// when the platform knows no installer format the file has. Nothing is installed or run from the file.
    pub fn inspect(&self, path: &Path) -> Result<Option<Inspection>, InspectError> {
        match self.host.file_type(path) {
            None => return Err(InspectError::Missing(path.to_path_buf())),
            Some(FileType::File { .. }) => {}
            Some(_) => return Err(InspectError::NotAFile(path.to_path_buf())),
        }
        let Some(recognised) = self.platform.recognise_file(&self.host, path)? else {
            return Ok(None);
        };
        let source = recognised
            .source
            .as_ref()
            .and_then(|id| self.platform.sources().iter().find(|source| source.info().id == *id));
        let fit = if !recognised.made_for.is_empty() {
            Fit::OtherSystem {
                families: recognised.made_for,
            }
        } else if let Some(Availability::Unavailable { reason }) = source.map(|source| source.probe(&self.host)) {
            Fit::SourceMissing { reason }
        } else {
            match recognised.package.as_ref().and_then(|package| package.arch.as_ref()) {
                Some(arch) if *arch != self.system().arch => Fit::OtherArch { arch: arch.clone() },
                _ => Fit::Fits,
            }
        };
        Ok(Some(Inspection {
            path: path.to_path_buf(),
            format: recognised.format,
            evidence: recognised.evidence,
            source: source.map(|source| source.info()),
            package: recognised.package,
            fit,
            warnings: recognised.warnings,
        }))
    }
}
