//! Operating-system independent core of Housekeeper.
//!
//! See `docs/architecture/README.md` for how platforms, sources and the host fit together.

pub mod command;
pub mod group;
pub mod host;
mod identity;
pub mod inventory;
pub mod model;
mod ownership;
pub mod platform;
pub mod remove;
pub mod show;
pub mod source;
#[cfg(test)]
mod stub;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod text;
pub mod who;

pub use command::{Cmd, CommandRunner, Output, RunError, SystemRunner};
pub use group::{AppGroup, Shared};
pub use host::{Entry, FileType, Host};
pub use inventory::{Housekeeper, Scan, Selection, SourceReport, SourceStatus};
pub use model::{
    Availability, InstallReason, Launcher, Metainfo, Package, PackageKind, Scope, SourceId, SourceInfo, SourceKind,
    SystemInfo,
};
pub use platform::{Platform, path_dirs};
pub use remove::{Action, Actor, RemovalPlan, Remover, Simulation, Simulator, Step, Tier};
pub use show::{Details, Files, FilesFrom, Lookup};
pub use source::{Dependants, FileList, OwnerLookup, ScanContext, Source, SourceError};
pub use who::{Evidence, Found, Owner, Who};
