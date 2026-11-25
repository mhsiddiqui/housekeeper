//! apt, which changes the Debian package database (Debian, Ubuntu, Mint…).

use housekeeper_core::{Host, Package, RemovalPlan, Remover, SourceError};

use super::removal;
use crate::provider::{LinuxTool, PackageTool, Serves, register_package_tool};

pub struct Apt;

impl LinuxTool for Apt {
    const SERVES: Serves = Serves {
        families: &["debian"],
        image_based: false,
    };

    fn create() -> Self {
        Self
    }
}
register_package_tool!(Apt);

impl PackageTool for Apt {
    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }
}

impl Remover for Apt {
    /// By its dpkg id, which carries the architecture when several are installed (`libc6:i386`).
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal("apt", &["apt-get", "remove", "--yes"], &package.id))
    }
}
