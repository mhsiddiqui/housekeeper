//! Which packages `housekeeper list` shows.

use housekeeper_core::{InstallReason, Package, PackageKind};

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

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::{Scope, SourceId};

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
}
