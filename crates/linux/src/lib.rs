//! Linux platform for Housekeeper.

pub mod distro;
pub mod sources;

use housekeeper_core::{Host, Platform, Source, SystemInfo};

use crate::distro::{Distro, Family};
use crate::sources::{
    dpkg::DpkgSource, flatpak::FlatpakSource, pacman::PacmanSource, rpm::RpmSource, snap::SnapSource,
};

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
    let rpm = || Box::new(RpmSource::new(distro.image_based)) as Box<dyn Source>;
    match distro.family {
        Family::Debian => Some(Box::new(DpkgSource)),
        Family::Fedora | Family::Suse => Some(rpm()),
        Family::Arch => Some(Box::new(PacmanSource)),
        Family::Unknown => {
            if host.has_program("dpkg-query") {
                Some(Box::new(DpkgSource))
            } else if host.has_program("rpm") {
                Some(rpm())
            } else if host.has_program("pacman") {
                Some(Box::new(PacmanSource))
            } else {
                None
            }
        }
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
    fn picks_the_familys_manager_even_when_another_is_present() {
        let both = || FakeRunner::new().program("dpkg-query").program("rpm");
        assert_eq!(
            native_id("ID=ubuntu\nID_LIKE=debian\n", both()).as_deref(),
            Some("dpkg")
        );
        assert_eq!(native_id("ID=fedora\n", both()).as_deref(), Some("rpm"));
        assert_eq!(native_id("ID=opensuse-tumbleweed\n", both()).as_deref(), Some("rpm"));
        assert_eq!(
            native_id("ID=endeavouros\nID_LIKE=arch\n", both()).as_deref(),
            Some("pacman")
        );
    }

    #[test]
    fn unknown_distros_fall_back_to_whatever_manager_exists() {
        assert_eq!(native_id("ID=nixos\n", FakeRunner::new()), None);
        let pacman_only = FakeRunner::new().program("pacman");
        assert_eq!(native_id("ID=gentoo\n", pacman_only).as_deref(), Some("pacman"));
    }
}
