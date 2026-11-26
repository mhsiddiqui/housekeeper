//! The running kernel, which the native package managers protect (dnf's `protect_running_kernel`; apt and pacman
//! warn), while older kernels installed next to it can go.

use std::collections::HashSet;
use std::path::PathBuf;

use housekeeper_core::{Host, Package, ScanContext, SourceError};

/// The reason given for protecting the running kernel's packages.
const RUNNING_KERNEL: &str = "it is the running kernel";

/// Marks the packages that installed the running kernel as protected, unless something already protects them.
///
/// The kernel is found by its release (`/proc/sys/kernel/osrelease`) and its image: under its modules folder (Fedora,
/// Arch, openSUSE) or in `/boot` (Debian, Ubuntu). `owners` gives the `Package::id`s of the packages that own the
/// images that exist; it must name the exact install, because several kernels of one name can be installed. When it
/// fails the scan goes on with a warning, without that protection.
pub(crate) fn protect_running_kernel(
    host: &Host,
    scan: &ScanContext<'_>,
    packages: &mut [Package],
    owners: impl FnOnce(&[PathBuf]) -> Result<HashSet<String>, SourceError>,
) {
    let release = host
        .read_optional("/proc/sys/kernel/osrelease")
        .ok()
        .flatten()
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    if release.is_empty() || release.contains('/') {
        return;
    }
    let images: Vec<PathBuf> = [
        format!("/usr/lib/modules/{release}/vmlinuz"),
        format!("/lib/modules/{release}/vmlinuz"),
        format!("/boot/vmlinuz-{release}"),
    ]
    .into_iter()
    .map(PathBuf::from)
    .filter(|path| host.exists(path))
    .collect();
    if images.is_empty() {
        return;
    }
    let ids = match owners(&images) {
        Ok(ids) => ids,
        Err(error) => {
            scan.warn(format!(
                "could not tell which package installed the running kernel {release}, so it is not protected: {error}"
            ));
            return;
        }
    };
    for package in packages.iter_mut().filter(|p| ids.contains(&p.id)) {
        package.protected.get_or_insert_with(|| RUNNING_KERNEL.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};
    use housekeeper_core::{Scope, SourceId};

    fn kernel(id: &str) -> Package {
        Package::new(SourceId::new("rpm"), id, "kernel-core", Scope::System)
    }

    #[test]
    fn only_the_running_kernel_is_protected_and_a_failed_lookup_is_a_warning() {
        let fixture = Fixture::new();
        fixture
            .file("/proc/sys/kernel/osrelease", "6.11.6-300.fc41.x86_64\n")
            .file("/usr/lib/modules/6.11.6-300.fc41.x86_64/vmlinuz", "")
            .file("/usr/lib/modules/6.10.0-100.fc41.x86_64/vmlinuz", "");
        let host = fixture.host(FakeRunner::new());
        let mut packages = vec![
            kernel("kernel-core-6.10.0-100.fc41.x86_64"),
            kernel("kernel-core-6.11.6-300.fc41.x86_64"),
        ];
        let scan = ScanContext::default();
        protect_running_kernel(&host, &scan, &mut packages, |images| {
            assert_eq!(
                images,
                [PathBuf::from("/usr/lib/modules/6.11.6-300.fc41.x86_64/vmlinuz")]
            );
            Ok(HashSet::from(["kernel-core-6.11.6-300.fc41.x86_64".to_string()]))
        });
        assert_eq!(packages[0].protected, None, "an older kernel can go");
        assert_eq!(packages[1].protected.as_deref(), Some(RUNNING_KERNEL));

        let mut packages = vec![kernel("kernel-core-6.11.6-300.fc41.x86_64")];
        protect_running_kernel(&host, &scan, &mut packages, |_| {
            Err(SourceError::parse("rpm", "garbage"))
        });
        assert_eq!(packages[0].protected, None);
        assert_eq!(scan.into_warnings().len(), 1);
    }
}
