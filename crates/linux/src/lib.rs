//! Linux platform for Housekeeper.
//!
//! Sources live in `src/sources/`, one file each, and register themselves (see [`provider`]). This file only picks
//! the native package manager for the running distribution and assembles everything; it never names a source.

mod appstream;
mod desktop;
pub mod distro;
mod files;
mod owner;
pub mod provider;
mod python;
pub mod sources;
mod xdg;

use std::path::PathBuf;
use std::sync::Arc;

use housekeeper_core::{Host, Launcher, Metainfo, Platform, ScanContext, Source, SourceKind, SystemInfo, path_dirs};

use crate::distro::Distro;
use crate::provider::{Context, Provider};
use crate::sources::PROVIDERS;

/// A Linux system and the sources that apply to it.
pub struct LinuxPlatform {
    distro: Distro,
    system: SystemInfo,
    sources: Vec<Arc<dyn Source>>,
}

impl LinuxPlatform {
    pub fn detect(host: &Host) -> Self {
        Self::assemble(Distro::detect(host), host, PROVIDERS)
    }

    fn assemble(distro: Distro, host: &Host, providers: &[Provider]) -> Self {
        let native = native_provider(&distro, host, providers);
        let family = native.map_or("unknown", |(_, family)| family);
        let context = Context { distro: &distro };
        let mut sources: Vec<Arc<dyn Source>> = native
            .map(|(provider, _)| provider)
            .into_iter()
            .chain(providers.iter().filter(|provider| provider.native.is_none()))
            .map(|provider| provider.create(&context))
            .collect();
        sources.sort_by_key(|source| {
            let info = source.info();
            (kind_order(info.kind), info.id)
        });

        let system = SystemInfo {
            os: "linux".into(),
            name: distro.name.clone(),
            id: distro.id.clone(),
            version: distro.version.clone(),
            family: family.into(),
            image_based: distro.image_based,
            arch: std::env::consts::ARCH.into(),
        };
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

    fn sources(&self) -> &[Arc<dyn Source>] {
        &self.sources
    }

    fn launchers(&self, host: &Host, scan: &ScanContext<'_>) -> Vec<Launcher> {
        desktop::launchers(host, &self.command_dirs(host), scan)
    }

    fn metainfo(&self, host: &Host, scan: &ScanContext<'_>) -> Vec<Metainfo> {
        appstream::metainfo(host, scan)
    }

    fn command_dirs(&self, host: &Host) -> Vec<PathBuf> {
        let mut dirs = path_dirs(host);
        for dir in STANDARD_COMMAND_DIRS.iter().map(PathBuf::from) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs
    }
}

/// Program folders every distribution has; a session's `PATH` can lack some (Debian leaves out `sbin` for users).
const STANDARD_COMMAND_DIRS: [&str; 8] = [
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
    "/usr/local/games",
    "/usr/games",
];

/// Exactly one native package manager (decision D2), with the family name it serves. The distribution's `ID`, then
/// each `ID_LIKE` entry, is matched against every native source's families; on a distribution none claims, the first
/// native source whose program is installed is used. A stray `rpm` on Ubuntu therefore never becomes a second system
/// source.
fn native_provider<'p>(
    distro: &Distro,
    host: &Host,
    providers: &'p [Provider],
) -> Option<(&'p Provider, &'static str)> {
    let natives = || {
        providers
            .iter()
            .filter_map(|p| p.native.as_ref().map(|native| (p, native)))
    };
    distro
        .ids()
        .find_map(|id| {
            natives().find_map(|(provider, native)| {
                native
                    .families
                    .iter()
                    .find(|family| family.distro_ids.contains(&id))
                    .map(|family| (provider, family.name))
            })
        })
        .or_else(|| {
            natives()
                .find(|(_, native)| host.has_program(native.program))
                .map(|(provider, _)| (provider, "unknown"))
        })
}

/// Display order: the system's own packages first, scripts and hand installs last.
fn kind_order(kind: SourceKind) -> u8 {
    match kind {
        SourceKind::System => 0,
        SourceKind::Universal => 1,
        SourceKind::Portable => 2,
        SourceKind::Language => 3,
        SourceKind::Manual => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    fn platform(os_release: &str, runner: FakeRunner) -> LinuxPlatform {
        let fixture = Fixture::new();
        fixture.file("/etc/os-release", os_release);
        LinuxPlatform::detect(&fixture.host(runner))
    }

    fn native(platform: &LinuxPlatform) -> Option<String> {
        let first = platform.sources().first()?.info();
        (first.kind == SourceKind::System).then(|| first.id.to_string())
    }

    #[test]
    fn native_manager_and_family_come_from_id_then_id_like() {
        let both = || FakeRunner::new().program("dpkg-query").program("rpm");
        let cases = [
            ("ID=ubuntu\nID_LIKE=debian\n", "dpkg", "debian"),
            ("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n", "dpkg", "debian"),
            ("ID=fedora\n", "rpm", "fedora"),
            ("ID=rocky\nID_LIKE=\"rhel centos fedora\"\n", "rpm", "fedora"),
            ("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n", "rpm", "suse"),
            ("ID=endeavouros\nID_LIKE=arch\n", "pacman", "arch"),
            ("ID=somethingnew\nID_LIKE=arch\n", "pacman", "arch"),
        ];
        for (os_release, id, family) in cases {
            let platform = platform(os_release, both());
            assert_eq!(native(&platform).as_deref(), Some(id), "{os_release:?}");
            assert_eq!(platform.system().family, family, "{os_release:?}");
        }
    }

    #[test]
    fn unknown_distros_fall_back_to_whatever_manager_exists() {
        let none = platform("ID=nixos\n", FakeRunner::new());
        assert_eq!(native(&none), None);
        assert_eq!(none.system().family, "unknown");
        let gentoo = platform("ID=gentoo\n", FakeRunner::new().program("pacman"));
        assert_eq!(native(&gentoo).as_deref(), Some("pacman"));
        assert_eq!(gentoo.system().family, "unknown");
    }

    #[test]
    fn every_registered_non_native_source_is_present_exactly_once_in_display_order() {
        let platform = platform("ID=ubuntu\nID_LIKE=debian\n", FakeRunner::new());
        let ids: Vec<String> = platform.sources().iter().map(|s| s.info().id.to_string()).collect();
        let natives = PROVIDERS.iter().filter(|p| p.native.is_some()).count();
        assert_eq!(
            ids.len(),
            PROVIDERS.len() - natives + 1,
            "one native plus every other source: {ids:?}"
        );
        assert_eq!(ids.first().map(String::as_str), Some("dpkg"));
        assert_eq!(ids.last().map(String::as_str), Some("manual"));
        let kinds: Vec<u8> = platform.sources().iter().map(|s| kind_order(s.info().kind)).collect();
        assert!(kinds.is_sorted(), "{ids:?}");
    }
}
