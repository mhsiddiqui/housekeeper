//! dnf, which changes the RPM database on Fedora, RHEL and their rebuilds.

use housekeeper_core::{Host, Package, RemovalPlan, Remover, Simulation, Simulator, SourceError};

use super::{removal, rpm_simulation};
use crate::provider::{LinuxTool, PackageTool, Serves, register_package_tool};

pub struct Dnf;

impl LinuxTool for Dnf {
    const SERVES: Serves = Serves {
        families: &["fedora"],
        image_based: false,
    };

    fn create() -> Self {
        Self
    }
}
register_package_tool!(Dnf);

impl PackageTool for Dnf {
    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }
}

impl Remover for Dnf {
    /// By its full NEVRA, so one of several installed kernels goes and not all of them.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal("dnf", &["dnf", "remove", "--assumeyes"], &package.id))
    }

    fn simulator(&self) -> Option<&dyn Simulator> {
        Some(self)
    }
}

/// dnf removes what needs a package along with it; dnf4 will not simulate without root, so the RPM database answers.
impl Simulator for Dnf {
    fn simulate(&self, host: &Host, package: &Package) -> Result<Simulation, SourceError> {
        rpm_simulation(host, package)
    }
}
