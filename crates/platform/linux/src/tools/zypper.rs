//! zypper, which changes the RPM database on openSUSE and SUSE Linux Enterprise.

use housekeeper_core::{
    FileInstaller, Host, InstallFile, InstallPlan, InstallSimulation, InstallSimulator, LockHolder, Package,
    PackageLock, PackageRemoval, RemovalPlan, Remover, Simulation, Simulator, SourceError,
};

use super::{RPM_LOCKS, command_for, installation, removal, rpm_install_simulation, rpm_installed, rpm_simulation};
use crate::lock::{self, LockFile};
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

    fn package_removal(&self) -> Option<&dyn PackageRemoval> {
        Some(self)
    }

    fn file_installer(&self) -> Option<&dyn FileInstaller> {
        Some(self)
    }

    fn lock(&self) -> Option<&dyn PackageLock> {
        Some(self)
    }
}

impl FileInstaller for Zypper {
    /// `zypper install` with the file's path, which installs what it requires from the repositories too.
    fn plan(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallPlan, SourceError> {
        Ok(installation(
            self,
            host,
            file,
            &["zypper", "--non-interactive", "install"],
        ))
    }

    fn simulator(&self) -> Option<&dyn InstallSimulator> {
        Some(self)
    }
}

/// zypper's `--dry-run` needs root, so the RPM database answers (D33).
impl InstallSimulator for Zypper {
    fn simulate(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallSimulation, SourceError> {
        rpm_install_simulation(host, file)
    }
}

/// libzypp's lock, held by zypper, YaST and PackageKit for their whole run, then the RPM database's.
impl PackageLock for Zypper {
    fn holder(&self, host: &Host) -> Option<LockHolder> {
        let [rpm, rpm_old] = RPM_LOCKS;
        lock::holder(host, &[LockFile::Pid("/run/zypp.pid"), rpm, rpm_old])
    }
}

impl PackageRemoval for Zypper {
    fn tool(&self) -> &'static str {
        "zypper"
    }

    fn is_installed(&self, host: &Host, package: &str) -> Result<bool, SourceError> {
        rpm_installed(host, package)
    }

    /// Without remote repositories: removing needs only what is installed, and zypper would otherwise refresh them
    /// first, failing when a mirror is down.
    fn command(&self, packages: &[String]) -> Vec<String> {
        command_for(&["zypper", "--non-interactive", "--no-remote", "remove"], packages)
    }
}

impl Remover for Zypper {
    /// By its full name-edition-arch (`bash-5.3.20-11.1.x86_64`), which zypper accepts, so one of several installed
    /// kernels goes and not all of them.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal(self, &package.id))
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
