//! Operating-system independent core of Housekeeper.
//!
//! See `docs/architecture/README.md` for how platforms, sources and the host fit together.

pub mod command;
pub mod host;
pub mod inventory;
pub mod model;
pub mod platform;
pub mod source;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod text;
pub mod who;

pub use command::{Cmd, CommandRunner, Output, RunError, SystemRunner};
pub use host::{Entry, FileType, Host};
pub use inventory::{Housekeeper, Scan, Selection, SourceReport, SourceStatus};
pub use model::{
    Availability, InstallReason, Package, PackageKind, Scope, SourceId, SourceInfo, SourceKind, SystemInfo,
};
pub use platform::Platform;
pub use source::{OwnerLookup, ScanContext, Source, SourceError};
pub use who::{Evidence, Found, Owner, Who};
