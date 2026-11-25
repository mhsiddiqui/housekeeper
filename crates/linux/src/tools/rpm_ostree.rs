//! rpm-ostree, which layers packages on the image of Fedora Silverblue, Kinoite, Bazzite and the other Fedora Atomic
//! systems (decision D3).

use housekeeper_core::{Host, InstallReason, Package, RemovalPlan, Remover, SourceError, Tier};

use super::removal;
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
}

impl Remover for RpmOstree {
    /// Only what the user layered (the rpm source marks it `Explicit`) is uninstalled, by the name it was layered
    /// with; the change applies to the next boot. The image's own packages can only be hidden (`override remove`),
    /// which is not removing what the user installed.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(match package.reason {
            InstallReason::Explicit => {
                removal("rpm-ostree", &["rpm-ostree", "uninstall"], &package.name).needs_restart()
            }
            InstallReason::System => RemovalPlan::new(Tier::PackageManager, Vec::new())
                .blocked("part of the system image; rpm-ostree only uninstalls packages layered on it"),
            InstallReason::Dependency | InstallReason::Unknown => RemovalPlan::new(Tier::PackageManager, Vec::new())
                .blocked("rpm-ostree did not say whether it was layered on the system image"),
        })
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
        let layered = RpmOstree
            .plan(&host, &package("htop", InstallReason::Explicit))
            .unwrap();
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
            let image = RpmOstree.plan(&host, &package("glibc", reason)).unwrap();
            assert!(image.steps.is_empty(), "{reason:?}");
            assert_eq!(image.blockers.len(), 1, "{reason:?}");
        }
    }
}
