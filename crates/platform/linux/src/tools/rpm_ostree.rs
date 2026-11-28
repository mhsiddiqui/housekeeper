//! rpm-ostree, which layers packages on the image of Fedora Silverblue, Kinoite, Bazzite and the other Fedora Atomic
//! systems (decision D3).

use std::path::Path;

use housekeeper_core::{
    FileInstaller, Host, InstallFile, InstallPlan, InstallReason, InstallSimulation, InstallSimulator, Package,
    PackageInstall, PackageRemoval, RemovalPlan, Remover, SourceError, Tier,
};

use super::{command_for, install_command_for, installation, removal, rpm_install_simulation, rpm_installed};
use crate::provider::{LinuxTool, PackageTool, Serves, register_package_tool};

pub struct RpmOstree;

impl LinuxTool for RpmOstree {
    const SERVES: Serves = Serves {
        families: &["fedora"],
        image_based: true,
    };

    fn create() -> Self {
        Self
    }
}
register_package_tool!(RpmOstree);

impl PackageTool for RpmOstree {
    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }

    fn package_removal(&self) -> Option<&dyn PackageRemoval> {
        Some(self)
    }

    fn package_install(&self) -> Option<&dyn PackageInstall> {
        Some(self)
    }

    fn file_installer(&self) -> Option<&dyn FileInstaller> {
        Some(self)
    }
}

impl PackageRemoval for RpmOstree {
    fn tool(&self) -> &'static str {
        "rpm-ostree"
    }

    /// The booted deployment's RPM database lists it (a layered package is in it, by name).
    fn is_installed(&self, host: &Host, package: &str) -> Result<bool, SourceError> {
        rpm_installed(host, package)
    }

    fn command(&self, packages: &[String]) -> Vec<String> {
        command_for(&["rpm-ostree", "uninstall"], packages)
    }
}

impl Remover for RpmOstree {
    /// Only what the user layered (the rpm source marks it `Explicit`) is uninstalled, by the name it was layered
    /// with; the change applies to the next boot. The image's own packages can only be hidden (`override remove`),
    /// which is not removing what the user installed.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(match package.reason {
            InstallReason::Explicit => removal(self, &package.name).needs_restart(),
            InstallReason::System => RemovalPlan::new(Tier::PackageManager, Vec::new())
                .blocked("part of the system image; rpm-ostree only uninstalls packages layered on it"),
            InstallReason::Dependency | InstallReason::Unknown => RemovalPlan::new(Tier::PackageManager, Vec::new())
                .blocked("rpm-ostree did not say whether it was layered on the system image"),
        })
    }
}

/// Layered on the image with `rpm-ostree install`, which imports the file then, so the copy the helper installs may go
/// before the restart.
impl PackageInstall for RpmOstree {
    fn formats(&self) -> &'static [&'static str] {
        &["rpm"]
    }

    fn install_command(&self, host: &Host, file: &Path) -> Result<Vec<String>, SourceError> {
        Ok(install_command_for(&["rpm-ostree", "install"], host, file))
    }
}

impl FileInstaller for RpmOstree {
    /// Effective after a restart. The product's advice for image-based systems is a Flatpak for apps (D3).
    fn plan(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallPlan, SourceError> {
        Ok(installation(self, host, file)?
            .needs_restart()
            .caution("layered packages slow down every system update; for an app, a Flatpak is the usual choice here"))
    }

    fn simulator(&self) -> Option<&dyn InstallSimulator> {
        Some(self)
    }
}

/// rpm-ostree resolves against the image's RPM database, which `rpm -U --test` reads without root.
impl InstallSimulator for RpmOstree {
    fn simulate(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallSimulation, SourceError> {
        rpm_install_simulation(host, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};
    use housekeeper_core::{Action, Actor, Scope, SourceId};

    fn package(name: &str, reason: InstallReason) -> Package {
        let mut package = Package::new(
            SourceId::new("rpm"),
            format!("{name}-1.0-1.fc41.x86_64"),
            name,
            Scope::System,
        );
        package.reason = reason;
        package
    }

    #[test]
    fn layered_packages_are_uninstalled_by_name_after_a_restart_and_image_packages_are_blocked() {
        let host = Fixture::new().host(FakeRunner::new());
        let layered = Remover::plan(&RpmOstree, &host, &package("htop", InstallReason::Explicit)).unwrap();
        assert!(layered.restart && layered.blockers.is_empty());
        assert_eq!(layered.steps.len(), 1);
        assert_eq!(layered.steps[0].by, Actor::Helper);
        assert_eq!(
            layered.steps[0].action,
            Action::RemovePackages {
                tool: "rpm-ostree".into(),
                packages: vec!["htop".into()],
                argv: vec!["rpm-ostree".into(), "uninstall".into(), "htop".into()],
            }
        );

        for reason in [InstallReason::System, InstallReason::Unknown] {
            let image = Remover::plan(&RpmOstree, &host, &package("glibc", reason)).unwrap();
            assert!(image.steps.is_empty(), "{reason:?}");
            assert_eq!(image.blockers.len(), 1, "{reason:?}");
        }
    }
}
