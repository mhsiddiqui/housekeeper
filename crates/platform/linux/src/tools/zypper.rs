//! zypper, which changes the RPM database on openSUSE and SUSE Linux Enterprise.

use housekeeper_core::{Host, Package, RemovalPlan, Remover, Simulation, Simulator, SourceError};

use super::{removal, rpm_simulation};
use crate::provider::{LinuxTool, PackageTool, Serves, register_package_tool};

pub struct Zypper;

impl LinuxTool for Zypper {
    const SERVES: Serves = Serves {
        families: &["suse"],
        image_based: false,
    };

    fn create() -> Self {
        Self
    }
}
register_package_tool!(Zypper);

impl PackageTool for Zypper {
    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }
}

impl Remover for Zypper {
    /// By its full name-edition-arch (`bash-5.3.20-11.1.x86_64`), which zypper accepts, so one of several installed
    /// kernels goes and not all of them.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal(
            "zypper",
            &["zypper", "--non-interactive", "remove"],
            &package.id,
        ))
    }

    fn simulator(&self) -> Option<&dyn Simulator> {
        Some(self)
    }
}

/// zypper removes what needs a package along with it; its `--dry-run` needs root, so the RPM database answers.
impl Simulator for Zypper {
    fn simulate(&self, host: &Host, package: &Package) -> Result<Simulation, SourceError> {
        rpm_simulation(host, package)
    }
}
