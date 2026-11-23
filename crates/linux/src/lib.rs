//! Linux platform for Housekeeper.

pub mod distro;
pub mod sources;

use housekeeper_core::{Host, Platform, Source, SystemInfo};

use crate::distro::{Distro, Family};
use crate::sources::{dpkg::DpkgSource, flatpak::FlatpakSource, snap::SnapSource};

/// A Linux system and the sources that apply to it.
pub struct LinuxPlatform {
    distro: Distro,
    system: SystemInfo,
    sources: Vec<Box<dyn Source>>,
}

impl LinuxPlatform {
    pub fn detect(host: &Host) -> Self {
        let distro = Distro::detect(host);
        let system = SystemInfo {
            os: "linux".into(),
            name: distro.name.clone(),
            id: distro.id.clone(),
            version: distro.version.clone(),
            family: distro.family.as_str().into(),
            image_based: distro.image_based,
            arch: std::env::consts::ARCH.into(),
        };
        let mut sources: Vec<Box<dyn Source>> = Vec::new();
        if let Some(native) = native_source(&distro, host) {
            sources.push(native);
        }
        sources.push(Box::new(FlatpakSource));
        sources.push(Box::new(SnapSource));
        Self {
            distro,
            system,
            sources,
        }
    }

    pub fn distro(&self) -> &Distro {
        &self.distro
    }
}

impl Platform for LinuxPlatform {
    fn system(&self) -> &SystemInfo {
        &self.system
    }

    fn sources(&self) -> &[Box<dyn Source>] {
        &self.sources
    }
}

/// Exactly one native package manager (decision D2): the family's own, or the first one found on unknown distros.
/// A stray `rpm` on Ubuntu therefore never shows up as a second system source.
fn native_source(distro: &Distro, host: &Host) -> Option<Box<dyn Source>> {
    match distro.family {
        Family::Debian => Some(Box::new(DpkgSource)),
        Family::Fedora | Family::Suse | Family::Arch => None,
        Family::Unknown => host
            .has_program("dpkg-query")
            .then(|| Box::new(DpkgSource) as Box<dyn Source>),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::SourceKind;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    fn native_id(os_release: &str, runner: FakeRunner) -> Option<String> {
        let fixture = Fixture::new();
        fixture.file("/etc/os-release", os_release);
        let platform = LinuxPlatform::detect(&fixture.host(runner));
        let first = platform.sources().first()?.info();
        (first.kind == SourceKind::System).then(|| first.id.to_string())
    }

    #[test]
    fn debian_family_uses_dpkg_even_when_rpm_is_present() {
        let runner = FakeRunner::new().program("dpkg-query").program("rpm");
        assert_eq!(
            native_id("ID=ubuntu\nID_LIKE=debian\n", runner).as_deref(),
            Some("dpkg")
        );
    }

    #[test]
    fn unknown_distros_fall_back_to_whatever_manager_exists() {
        assert_eq!(native_id("ID=nixos\n", FakeRunner::new()), None);
        assert_eq!(
            native_id("ID=gentoo\n", FakeRunner::new().program("dpkg-query")).as_deref(),
            Some("dpkg")
        );
    }
}
