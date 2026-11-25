//! Which packages `housekeeper list` shows.

use std::collections::{HashMap, HashSet};

use housekeeper_core::{InstallReason, Package, PackageKind, Scan};

/// What `housekeeper list` shows: the filtered packages, or only the apps installed more than once.
pub struct View {
    pub filter: Filter,
    pub duplicates: bool,
}

pub struct Filter {
    /// Show everything, not just what someone installed on purpose.
    pub all: bool,
    pub search: Option<String>,
}

impl Filter {
    pub fn keeps(&self, package: &Package) -> bool {
        (self.all || is_users_choice(package)) && self.matches_search(package)
    }

    fn matches_search(&self, package: &Package) -> bool {
        let Some(needle) = self.search.as_deref().map(str::to_lowercase) else {
            return true;
        };
        [Some(&package.name), package.title.as_ref(), package.summary.as_ref()]
            .into_iter()
            .flatten()
            .any(|text| text.to_lowercase().contains(&needle))
    }
}

/// Installed on purpose and something a person uses directly.
fn is_users_choice(package: &Package) -> bool {
    matches!(package.reason, InstallReason::Explicit | InstallReason::Unknown)
        && !matches!(
            package.kind,
            PackageKind::Library | PackageKind::Runtime | PackageKind::Component
        )
}

/// Keeps the groups with at least one install the filter keeps, and all of their installs (the point is to see every
/// copy), in group order. Packages in no such group are dropped.
pub fn keep_duplicates(scan: &mut Scan, filter: &Filter) {
    let kept: HashSet<String> = scan
        .packages
        .iter()
        .filter(|package| filter.keeps(package))
        .map(Package::key)
        .collect();
    scan.groups
        .retain(|group| group.packages.iter().any(|key| kept.contains(key)));
    let place: HashMap<&str, (usize, usize)> = scan
        .groups
        .iter()
        .enumerate()
        .flat_map(|(g, group)| {
            group
                .packages
                .iter()
                .enumerate()
                .map(move |(m, key)| (key.as_str(), (g, m)))
        })
        .collect();
    let mut packages: Vec<((usize, usize), Package)> = std::mem::take(&mut scan.packages)
        .into_iter()
        .filter_map(|package| place.get(package.key().as_str()).map(|&at| (at, package)))
        .collect();
    packages.sort_by_key(|(at, _)| *at);
    scan.packages = packages.into_iter().map(|(_, package)| package).collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::{AppGroup, Scope, SourceId, SystemInfo};

    fn package(name: &str, reason: InstallReason, kind: PackageKind) -> Package {
        let mut package = Package::new(SourceId::new("test"), name, name, Scope::System);
        package.reason = reason;
        package.kind = kind;
        package
    }

    #[test]
    fn default_view_hides_dependencies_libraries_runtimes_components_and_os_packages() {
        let filter = Filter {
            all: false,
            search: None,
        };
        assert!(filter.keeps(&package("gimp", InstallReason::Explicit, PackageKind::Unknown)));
        assert!(filter.keeps(&package("tool", InstallReason::Unknown, PackageKind::App)));
        assert!(!filter.keeps(&package("libfoo", InstallReason::Explicit, PackageKind::Library)));
        assert!(!filter.keeps(&package(
            "gnome-platform",
            InstallReason::Explicit,
            PackageKind::Runtime
        )));
        assert!(!filter.keeps(&package(
            "fonts-noto-core",
            InstallReason::Explicit,
            PackageKind::Component
        )));
        assert!(!filter.keeps(&package("libbar", InstallReason::Dependency, PackageKind::Unknown)));
        assert!(!filter.keeps(&package("bash", InstallReason::System, PackageKind::Unknown)));

        let all = Filter {
            all: true,
            search: None,
        };
        assert!(all.keeps(&package("bash", InstallReason::System, PackageKind::Unknown)));
    }

    #[test]
    fn search_matches_name_title_and_summary_case_insensitively() {
        let mut godot = package("org.godotengine.Godot", InstallReason::Explicit, PackageKind::App);
        godot.title = Some("Godot Engine".into());
        godot.summary = Some("Multi-platform 2D and 3D game engine".into());
        let search = |text: &str| Filter {
            all: false,
            search: Some(text.into()),
        };
        assert!(search("godot engine").keeps(&godot));
        assert!(search("GAME").keeps(&godot));
        assert!(search("godotengine").keeps(&godot));
        assert!(!search("blender").keeps(&godot));
    }

    #[test]
    fn duplicates_show_every_install_of_a_group_any_install_matches_in_group_order() {
        let mut cups_snap = package("cups", InstallReason::Explicit, PackageKind::App);
        cups_snap.source = SourceId::new("snap");
        let cups_deb = package("cups", InstallReason::Dependency, PackageKind::App);
        let mut tsc_old = package("typescript", InstallReason::Explicit, PackageKind::App);
        tsc_old.id = "v20/typescript".into();
        let mut tsc_new = tsc_old.clone();
        tsc_new.id = "v22/typescript".into();
        let group = |name: &str, members: &[&Package]| AppGroup {
            name: name.into(),
            packages: members.iter().map(|p| p.key()).collect(),
            shared: Vec::new(),
        };
        let groups = vec![
            group("CUPS", &[&cups_deb, &cups_snap]),
            group("typescript", &[&tsc_old, &tsc_new]),
        ];
        let mut scan = Scan {
            schema: 1,
            system: SystemInfo {
                os: "linux".into(),
                name: "Test".into(),
                id: "test".into(),
                version: None,
                family: "test".into(),
                image_based: false,
                arch: "x86_64".into(),
            },
            sources: Vec::new(),
            packages: vec![
                tsc_new.clone(),
                cups_snap.clone(),
                package("gimp", InstallReason::Explicit, PackageKind::App),
                cups_deb.clone(),
                tsc_old.clone(),
            ],
            groups,
            warnings: Vec::new(),
        };
        let filter = Filter {
            all: false,
            search: Some("cups".into()),
        };
        keep_duplicates(&mut scan, &filter);

        let keys: Vec<String> = scan.packages.iter().map(Package::key).collect();
        // The .deb is a dependency the default view hides, but it is still a copy of CUPS.
        assert_eq!(keys, [cups_deb.key(), cups_snap.key()]);
        assert_eq!(scan.groups.len(), 1);
    }
}
