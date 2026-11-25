//! pacman, which changes its own database on Arch Linux and its derivatives.

use housekeeper_core::{Host, Package, RemovalPlan, Remover, SourceError};

use super::removal;
use crate::provider::{LinuxTool, PackageTool, Serves, register_package_tool};

pub struct Pacman;

impl LinuxTool for Pacman {
    const SERVES: Serves = Serves {
        families: &["arch"],
        image_based: false,
    };

    fn create() -> Self {
        Self
    }
}
register_package_tool!(Pacman);

impl PackageTool for Pacman {
    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }
}

impl Remover for Pacman {
    /// Packages from the AUR are removed the same way: pacman installed them, whichever helper built them.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal("pacman", &["pacman", "--remove", "--noconfirm"], &package.id))
    }
}
