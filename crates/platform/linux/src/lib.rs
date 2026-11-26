//! Linux platform for Housekeeper.
//!
//! Sources live in `src/sources/` and package tools in `src/tools/`, one file each, and register themselves (see
//! [`provider`]). This file only picks the native package manager and its tool for the running distribution and
//! assembles everything; it never names a source or a tool.

mod appstream;
mod desktop;
pub mod distro;
mod files;
mod kernel;
mod lock;
mod owner;
pub mod provider;
mod python;
mod recipe;
mod rpmdb;
pub mod sources;
mod tools;
mod xdg;

use std::path::PathBuf;
use std::sync::Arc;

use housekeeper_core::{
    Host, Launcher, LeftoverKind, Metainfo, PackageLock, PackageRemoval, Platform, ScanContext, Source, SourceKind,
    SystemInfo, path_dirs,
};

use crate::distro::Distro;
use crate::provider::{Context, PackageTool, Provider, ToolProvider};
use crate::sources::PROVIDERS;
use crate::tools::TOOLS;

/// A Linux system and the sources that apply to it.
pub struct LinuxPlatform {
    distro: Distro,
    system: SystemInfo,
    sources: Vec<Arc<dyn Source>>,
    /// What changes the native package database (D24); `None` on a distribution no tool serves.
    tool: Option<Arc<dyn PackageTool>>,
}

impl LinuxPlatform {
    pub fn detect(host: &Host) -> Self {
        Self::assemble(Distro::detect(host), host, PROVIDERS, TOOLS)
    }

    fn assemble(distro: Distro, host: &Host, providers: &[Provider], tools: &[ToolProvider]) -> Self {
        let native = native_provider(&distro, host, providers);
        let family = native.map_or("unknown", |(_, family)| family);
        let tool = package_tool(family, distro.image_based, tools);
        let context = Context {
            distro: &distro,
            tool: tool.as_ref(),
        };
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
            tool,
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

    fn package_removal(&self) -> Option<&dyn PackageRemoval> {
        self.tool.as_deref().and_then(PackageTool::package_removal)
    }

    /// The package tool's lock, for changes made with that tool (`backend` is its name in plans, `apt`).
    fn package_lock(&self, backend: &str) -> Option<&dyn PackageLock> {
        let tool = self.tool.as_deref()?;
        let name = tool.package_removal()?.tool();
        (name == backend).then(|| tool.lock()).flatten()
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

    /// The XDG base directories (settings, caches, data, state). Flatpak's `~/.var/app` is the Flatpak source's own;
    /// snap's `~/snap/<name>` is no one's, since `snap remove` takes it along.
    fn leftover_roots(&self, host: &Host) -> Vec<(PathBuf, LeftoverKind)> {
        vec![
            (host.dir_from_var("XDG_CONFIG_HOME", ".config"), LeftoverKind::Config),
            (host.dir_from_var("XDG_CACHE_HOME", ".cache"), LeftoverKind::Cache),
            (host.data_home(), LeftoverKind::Data),
            (host.state_home(), LeftoverKind::State),
        ]
    }

    /// freedesktop folders every desktop program reads or writes: menus, fonts, themes, GTK's settings, the dconf
    /// database of every GNOME app's settings (Fedora, openSUSE and Arch have a package named `dconf`), the keyring,
    /// container images podman, buildah and skopeo share.
    fn shared_state_folders(&self) -> &'static [&'static str] {
        SHARED_STATE_FOLDERS
    }

    /// bash's, zsh's (in `$ZDOTDIR` when set), the login shell's and fish's startup files.
    fn shell_files(&self, host: &Host) -> Vec<PathBuf> {
        let home = host.home();
        let zdotdir = host
            .var("ZDOTDIR")
            .filter(|dir| dir.starts_with('/'))
            .map_or_else(|| home.to_path_buf(), PathBuf::from);
        [".bashrc", ".bash_profile", ".bash_login", ".profile"]
            .iter()
            .map(|file| home.join(file))
            .chain(
                [".zshenv", ".zprofile", ".zshrc", ".zlogin"]
                    .iter()
                    .map(|file| zdotdir.join(file)),
            )
            .chain(std::iter::once(
                host.dir_from_var("XDG_CONFIG_HOME", ".config").join("fish/config.fish"),
            ))
            .collect()
    }
}

/// Lowercase, sorted.
const SHARED_STATE_FOLDERS: &[&str] = &[
    "applications",
    "autostart",
    "containers",
    "dbus-1",
    "dconf",
    "desktop-directories",
    "environment.d",
    "fonts",
    "gtk-2.0",
    "gtk-3.0",
    "gtk-4.0",
    "icons",
    "keyrings",
    "menus",
    "mime",
    "pulse",
    "sounds",
    "systemd",
    "themes",
    "trash",
];

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

/// The tool that changes the native package database (decision D24): the first whose families include the one the
/// native source was picked for, and that serves image-based systems exactly when this is one. A distribution no
/// family claims has none, so nothing guesses how to change its packages.
fn package_tool(family: &str, image_based: bool, tools: &[ToolProvider]) -> Option<Arc<dyn PackageTool>> {
    tools
        .iter()
        .find(|tool| tool.serves.image_based == image_based && tool.serves.families.contains(&family))
        .map(ToolProvider::create)
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
    use housekeeper_core::{Action, InstallReason, Package, Scope};

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

    /// The command the native source plans for removing a package it explicitly installed, if any. The root helper
    /// must build the very command the plan shows, for the tool the plan names.
    fn removal_command(os_release: &str, image_based: bool) -> Option<String> {
        let fixture = Fixture::new();
        fixture.file("/etc/os-release", os_release);
        if image_based {
            fixture.file("/run/ostree-booted", "");
        }
        let host = fixture.host(FakeRunner::new().program("dpkg-query").program("rpm"));
        let platform = LinuxPlatform::detect(&host);
        let native = platform
            .sources()
            .first()
            .filter(|s| s.info().kind == SourceKind::System)?;
        let mut package = Package::new(native.info().id, "htop-3.3.0-1.x86_64", "htop", Scope::System);
        package.reason = InstallReason::Explicit;
        let plan = native.remover()?.plan(&host, &package).ok()?;
        match &plan.steps.first()?.action {
            Action::RemovePackages { tool, packages, argv } => {
                let helper = platform
                    .package_removal()
                    .expect("a tool that plans removals removes them too");
                assert_eq!((helper.tool(), helper.command(packages)), (tool.as_str(), argv.clone()));
                Some(argv[..argv.len() - 1].join(" "))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn native_packages_are_removed_with_the_distributions_own_tool() {
        let cases = [
            ("ID=ubuntu\nID_LIKE=debian\n", false, Some("apt-get remove --yes")),
            ("ID=fedora\n", false, Some("dnf remove --assumeyes --disablerepo=*")),
            (
                "ID=almalinux\nID_LIKE=\"rhel centos fedora\"\n",
                false,
                Some("dnf remove --assumeyes --disablerepo=*"),
            ),
            ("ID=fedora\nVARIANT_ID=silverblue\n", true, Some("rpm-ostree uninstall")),
            (
                "ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n",
                false,
                Some("zypper --non-interactive --no-remote remove"),
            ),
            (
                "ID=endeavouros\nID_LIKE=arch\n",
                false,
                Some("pacman --remove --noconfirm"),
            ),
            // A package manager found on a distribution no family claims: nothing guesses which tool changes it.
            ("ID=nixos\n", false, None),
        ];
        for (os_release, image_based, expected) in cases {
            assert_eq!(
                removal_command(os_release, image_based).as_deref(),
                expected,
                "{os_release:?}"
            );
        }
    }
}
