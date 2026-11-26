//! dnf, which changes the RPM database on Fedora, RHEL and their rebuilds.

use housekeeper_core::{
    Host, LockHolder, Package, PackageLock, PackageRemoval, RemovalPlan, Remover, Simulation, Simulator, SourceError,
};

use super::{RPM_LOCKS, command_for, removal, rpm_installed, rpm_simulation};
use crate::lock;
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

    fn package_removal(&self) -> Option<&dyn PackageRemoval> {
        Some(self)
    }

    fn lock(&self) -> Option<&dyn PackageLock> {
        Some(self)
    }
}

/// The RPM database's lock, which dnf (and anything else changing packages) holds through its transaction.
impl PackageLock for Dnf {
    fn holder(&self, host: &Host) -> Option<LockHolder> {
        lock::holder(host, &RPM_LOCKS)
    }
}

impl PackageRemoval for Dnf {
    fn tool(&self) -> &'static str {
        "dnf"
    }

    fn is_installed(&self, host: &Host, package: &str) -> Result<bool, SourceError> {
        rpm_installed(host, package)
    }

    /// Without repositories: removing needs only what is installed, and a mirror that is down or slow must not stop it.
    fn command(&self, packages: &[String]) -> Vec<String> {
        command_for(&["dnf", "remove", "--assumeyes", "--disablerepo=*"], packages)
    }
}

impl Remover for Dnf {
    /// By its full NEVRA, so one of several installed kernels goes and not all of them.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal(self, &package.id))
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
