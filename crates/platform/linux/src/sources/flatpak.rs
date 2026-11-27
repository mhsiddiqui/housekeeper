//! Flatpak apps and runtimes from every installation (system, user and custom), read through `flatpak list`.

use std::path::{Path, PathBuf};

use housekeeper_core::text::parse_human_size;
use housekeeper_core::{
    Actor, Availability, Cmd, Dependants, FilePackage, Host, InstallReason, LeftoverKind, Package, PackageKind,
    RemovalPlan, Remover, ScanContext, Scope, Source, SourceError, SourceId, SourceInfo, SourceKind, Step, Tier,
};

use super::version_line;
use crate::formats::{field, nothing_inside};
use crate::provider::{Context, FileFormat, LinuxSource, register};

pub const ID: SourceId = SourceId::new("flatpak");

/// One line per installed ref, tab-separated, in this column order.
const COLUMNS: &str = "--columns=application,name,description,version,branch,arch,origin,installation,size,options";

/// One line per installed app, tab-separated: enough to rebuild its id, plus the runtime it runs on.
const APP_RUNTIME_COLUMNS: &str = "--columns=application,arch,branch,installation,runtime";

/// Where the default system installation keeps its refs.
const SYSTEM_DIR: &str = "/var/lib/flatpak";

pub struct FlatpakSource;

impl LinuxSource for FlatpakSource {
    const FILES: &'static [FileFormat] = &[
        FileFormat {
            id: "flatpakref",
            title: "Flatpak reference",
            suffixes: &[".flatpakref"],
            matches: is_flatpakref,
            evidence: "a `[Flatpak Ref]` group near its start",
            read: read_flatpakref,
        },
        FileFormat {
            id: "flatpak-bundle",
            title: "Flatpak bundle",
            suffixes: &[".flatpak"],
            matches: is_bundle,
            evidence: "Flatpak's bundle header (`flatpak` and its version bytes)",
            read: nothing_inside,
        },
    ];

    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(FlatpakSource);

/// How much of a `.flatpakref` is read: its keys and an embedded signing key fit in a few kilobytes.
const FLATPAKREF_LEN: usize = 64 * 1024;

/// A `.flatpakref` is a key file whose `[Flatpak Ref]` group starts in its first 256 bytes.
fn is_flatpakref(head: &[u8]) -> bool {
    head[..head.len().min(256)]
        .windows(b"[Flatpak Ref]".len())
        .any(|window| window == b"[Flatpak Ref]")
}

/// A single-file bundle (`flatpak build-bundle`) starts with `flatpak` (`xdg-app` before 0.6.10) and fixed bytes.
fn is_bundle(head: &[u8]) -> bool {
    [&b"flatpak\x00\x01\x00\x89\xe5"[..], b"xdg-app\x00\x01\x00\x89\xe5"]
        .iter()
        .any(|magic| head.starts_with(magic))
}

/// The app (or runtime) a `.flatpakref` points to, its title and the repository it would come from.
fn read_flatpakref(host: &Host, path: &Path, _: &[u8]) -> Result<Option<FilePackage>, SourceError> {
    let bytes = host.read_head(path, FLATPAKREF_LEN)?;
    let text = String::from_utf8_lossy(&bytes);
    let value = |key: &str| field(&text, key, '=').map(str::to_string);
    Ok(Some(FilePackage {
        name: value("Name"),
        title: value("Title"),
        origin: value("Url"),
        ..FilePackage::default()
    }))
}

impl Source for FlatpakSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("flatpak", "Flatpak", SourceKind::Universal).evidence("`flatpak list`")
    }

    fn probe(&self, host: &Host) -> Availability {
        if !host.has_program("flatpak") {
            return Availability::unavailable("flatpak is not installed");
        }
        let version = host
            .run_ok(&Cmd::new("flatpak").arg("--version"))
            .ok()
            .and_then(|out| version_line(&out))
            .and_then(|line| line.split_whitespace().nth(1).map(str::to_string));
        Availability::Available { version }
    }

    /// Where the system and user installations export their launchers.
    fn install_roots(&self, host: &Host) -> Vec<PathBuf> {
        vec![
            PathBuf::from("/var/lib/flatpak/exports"),
            host.data_home().join("flatpak/exports"),
        ]
    }

    /// Each app's settings, caches and data in `~/.var/app/<id>`, which `flatpak uninstall` keeps. Flatpak points an
    /// app's XDG folders there, so the shared `~/.config` and friends are not its (packages are sandboxed).
    fn leftover_roots(&self, host: &Host) -> Vec<(PathBuf, LeftoverKind)> {
        vec![(host.home().join(".var/app"), LeftoverKind::Data)]
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let listing = host.run_ok(&Cmd::new("flatpak").args(["list", COLUMNS]))?;
        let user_dir = host.data_home().join("flatpak");
        parse_listing(&listing, &user_dir)
    }

    fn dependant_lookup(&self) -> Option<&dyn Dependants> {
        Some(self)
    }

    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }
}

impl Remover for FlatpakSource {
    /// `flatpak uninstall` of the exact installed ref in its installation, as the user: flatpak's system helper asks
    /// for authorisation itself. Runtimes are planned the same way.
    fn plan(&self, _host: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        let Some((installation, flatpak_ref)) = package.id.split_once('/') else {
            return Err(SourceError::parse(
                "flatpak package id",
                format!("no installation: {:?}", package.id),
            ));
        };
        let installation_flag = match installation {
            "user" => "--user".to_string(),
            "system" => "--system".to_string(),
            name => format!("--installation={name}"),
        };
        Ok(RemovalPlan::new(
            Tier::PackageManager,
            vec![Step::run(
                Actor::User,
                [
                    "flatpak".to_string(),
                    "uninstall".to_string(),
                    "--noninteractive".to_string(),
                    installation_flag,
                    flatpak_ref.to_string(),
                ],
            )],
        ))
    }
}

impl Dependants for FlatpakSource {
    /// The apps that run on `package` when it is a runtime, in whichever installation they are (flatpak looks
    /// runtimes up across installations). Limit: runtime extensions (GL drivers, locales, codecs) attach to a runtime
    /// without any app naming them, so they are not followed.
    fn dependants(&self, host: &Host, package: &Package) -> Result<Vec<String>, SourceError> {
        let parts: Vec<&str> = package.id.split('/').collect();
        let [_, "runtime", runtime, arch, branch] = parts[..] else {
            // Nothing runs on an app.
            return Ok(Vec::new());
        };
        let runtime_ref = format!("{runtime}/{arch}/{branch}");
        let listing = host.run_ok(&Cmd::new("flatpak").args(["list", "--app", APP_RUNTIME_COLUMNS]))?;
        apps_on_runtime(&listing, &runtime_ref)
    }
}

/// The ids of the apps in `listing` (`APP_RUNTIME_COLUMNS` output) running on `runtime_ref` (`<id>/<arch>/<branch>`).
fn apps_on_runtime(listing: &str, runtime_ref: &str) -> Result<Vec<String>, SourceError> {
    let mut apps = Vec::new();
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [application, arch, branch, installation, runtime] = fields[..] else {
            return Err(SourceError::parse(
                "flatpak list --app output",
                format!("expected 5 fields: {line:?}"),
            ));
        };
        if runtime.trim() == runtime_ref {
            apps.push(format!("{installation}/app/{application}/{arch}/{branch}"));
        }
    }
    apps.sort();
    apps.dedup();
    Ok(apps)
}

/// `user_dir` is the logical path of the per-user installation.
fn parse_listing(listing: &str, user_dir: &Path) -> Result<Vec<Package>, SourceError> {
    let mut packages = Vec::new();
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [
            application,
            name,
            description,
            version,
            branch,
            arch,
            origin,
            installation,
            size,
            options,
        ] = fields[..]
        else {
            return Err(SourceError::parse(
                "flatpak list output",
                format!("expected 10 fields: {line:?}"),
            ));
        };
        let runtime = options.split(',').any(|option| option == "runtime");
        let ref_kind = if runtime { "runtime" } else { "app" };
        let scope = if installation == "user" {
            Scope::User
        } else {
            Scope::System
        };
        let id = format!("{installation}/{ref_kind}/{application}/{arch}/{branch}");

        let mut package = Package::new(ID, id, application, scope);
        package.title = non_empty(name);
        package.summary = non_empty(description);
        package.version = non_empty(version);
        package.size_bytes = parse_human_size(size);
        package.origin = non_empty(origin);
        package.sandboxed = true;
        (package.kind, package.reason) = if runtime {
            (PackageKind::Runtime, InstallReason::Dependency)
        } else {
            // An app's Flatpak ID is its AppStream ID.
            package.app_ids = vec![application.to_string()];
            (PackageKind::App, InstallReason::Explicit)
        };
        let installation_dir = match installation {
            "system" => Some(Path::new(SYSTEM_DIR)),
            "user" => Some(user_dir),
            // Custom installations live wherever their configuration in /etc/flatpak/installations.d says.
            _ => None,
        };
        package.location = installation_dir.map(|dir| dir.join(ref_kind).join(application));
        packages.push(package);
    }
    Ok(packages)
}

fn non_empty(field: &str) -> Option<String> {
    Some(field.trim().to_string()).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};

    // Sizes use a no-break space between number and unit, as flatpak prints them.
    const LISTING: &str = "\
org.godotengine.Godot\tGodot Engine\tEasily create 2D and 3D games\t4.7.2\tstable\tx86_64\tflathub\tsystem\t171.8\u{a0}MB\tsystem,current
org.freedesktop.Platform.GL.default\tMesa\tMesa - The 3D Graphics Library\t26.2.7\t25.08\tx86_64\tflathub\tsystem\t462.0\u{a0}MB\tsystem,runtime
org.freedesktop.Platform.GL.default\tMesa (Extra)\tMesa - The 3D Graphics Library\t26.2.7\t25.08-extra\tx86_64\tflathub\tsystem\t462.1\u{a0}MB\tsystem,runtime
org.freedesktop.Platform.GL.nvidia-595-99-02\tnvidia-595-99-02\t\t\t1.4\tx86_64\tflathub\tsystem\t821.7\u{a0}MB\tsystem,runtime
com.github.tchx84.Flatseal\tFlatseal\tManage Flatpak permissions\t2.3.0\tstable\tx86_64\tflathub\tuser\t1.2\u{a0}MB\tuser,current
org.gnome.Platform\tGNOME Application Platform version 48\tShared libraries used by GNOME applications\t\t48\tx86_64\tflathub\tuser\t1.1\u{a0}GB\tuser,runtime
org.mozilla.firefox\tFirefox\tFast, Private & Safe Web Browser\t145.0\tstable\taarch64\tflathub\textra-disk\t270.3\u{a0}MB\textra-disk,current
";

    fn scan_with(vars: &[(&str, &str)]) -> Vec<Package> {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(&format!("flatpak list {COLUMNS}"), LISTING);
        FlatpakSource
            .inventory(&fixture.host_with_vars(runner, vars), &ScanContext::default())
            .unwrap()
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn apps_are_explicit_and_runtimes_are_dependencies() {
        let packages = scan_with(&[]);
        assert_eq!(packages.len(), 7);
        let godot = find(&packages, "system/app/org.godotengine.Godot/x86_64/stable");
        assert_eq!(godot.name, "org.godotengine.Godot");
        assert_eq!(godot.title.as_deref(), Some("Godot Engine"));
        assert_eq!(godot.summary.as_deref(), Some("Easily create 2D and 3D games"));
        assert_eq!(godot.version.as_deref(), Some("4.7.2"));
        assert_eq!(godot.origin.as_deref(), Some("flathub"));
        assert_eq!(godot.size_bytes, Some(171_800_000));
        assert_eq!((godot.kind, godot.reason), (PackageKind::App, InstallReason::Explicit));
        assert_eq!(godot.scope, Scope::System);
        assert_eq!(
            godot.location.as_deref(),
            Some(Path::new("/var/lib/flatpak/app/org.godotengine.Godot"))
        );

        let mesa = find(
            &packages,
            "system/runtime/org.freedesktop.Platform.GL.default/x86_64/25.08",
        );
        assert_eq!(
            (mesa.kind, mesa.reason),
            (PackageKind::Runtime, InstallReason::Dependency)
        );
        assert_eq!(
            mesa.location.as_deref(),
            Some(Path::new(
                "/var/lib/flatpak/runtime/org.freedesktop.Platform.GL.default"
            ))
        );
    }

    #[test]
    fn branches_of_the_same_ref_stay_separate() {
        let packages = scan_with(&[]);
        let extra = find(
            &packages,
            "system/runtime/org.freedesktop.Platform.GL.default/x86_64/25.08-extra",
        );
        assert_eq!(extra.title.as_deref(), Some("Mesa (Extra)"));
        assert_eq!(extra.size_bytes, Some(462_100_000));
    }

    #[test]
    fn empty_columns_are_unknown() {
        let packages = scan_with(&[]);
        let nvidia = find(
            &packages,
            "system/runtime/org.freedesktop.Platform.GL.nvidia-595-99-02/x86_64/1.4",
        );
        assert_eq!(nvidia.version, None);
        assert_eq!(nvidia.summary, None);
        assert_eq!(nvidia.title.as_deref(), Some("nvidia-595-99-02"));
    }

    #[test]
    fn user_installation_lives_in_the_data_home() {
        let packages = scan_with(&[]);
        let flatseal = find(&packages, "user/app/com.github.tchx84.Flatseal/x86_64/stable");
        assert_eq!(flatseal.scope, Scope::User);
        assert_eq!(
            flatseal.location,
            Some(Path::new(HOME).join(".local/share/flatpak/app/com.github.tchx84.Flatseal"))
        );
        let gnome = find(&packages, "user/runtime/org.gnome.Platform/x86_64/48");
        assert_eq!((gnome.kind, gnome.scope), (PackageKind::Runtime, Scope::User));

        let packages = scan_with(&[("XDG_DATA_HOME", "/data/user")]);
        let flatseal = find(&packages, "user/app/com.github.tchx84.Flatseal/x86_64/stable");
        assert_eq!(
            flatseal.location.as_deref(),
            Some(Path::new("/data/user/flatpak/app/com.github.tchx84.Flatseal"))
        );
    }

    #[test]
    fn custom_installations_are_system_wide_without_a_known_location() {
        let packages = scan_with(&[]);
        let firefox = find(&packages, "extra-disk/app/org.mozilla.firefox/aarch64/stable");
        assert_eq!(firefox.scope, Scope::System);
        assert_eq!(firefox.location, None);
        assert_eq!(firefox.kind, PackageKind::App);
    }

    #[test]
    fn no_installed_refs_is_an_empty_inventory() {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(&format!("flatpak list {COLUMNS}"), "");
        assert!(
            FlatpakSource
                .inventory(&fixture.host(runner), &ScanContext::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_output_is_an_error() {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(&format!("flatpak list {COLUMNS}"), "org.example.App\tExample\n");
        assert!(matches!(
            FlatpakSource.inventory(&fixture.host(runner), &ScanContext::default()),
            Err(SourceError::Parse { .. })
        ));
    }

    #[test]
    fn a_runtimes_dependants_are_the_apps_on_its_branch() {
        let packages = scan_with(&[]);
        let apps = "\
org.godotengine.Godot\tx86_64\tstable\tsystem\torg.freedesktop.Platform/x86_64/25.08
com.github.tchx84.Flatseal\tx86_64\tstable\tuser\torg.gnome.Platform/x86_64/48
org.gnome.Builder\tx86_64\tstable\tsystem\torg.gnome.Platform/x86_64/47
";
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond(&format!("flatpak list --app {APP_RUNTIME_COLUMNS}"), apps);
        let gnome = find(&packages, "user/runtime/org.gnome.Platform/x86_64/48");
        assert_eq!(
            Dependants::dependants(&FlatpakSource, &fixture.host(runner), gnome).unwrap(),
            ["user/app/com.github.tchx84.Flatseal/x86_64/stable"]
        );
    }

    #[test]
    fn apps_have_no_dependants() {
        let packages = scan_with(&[]);
        let fixture = Fixture::new();
        let godot = find(&packages, "system/app/org.godotengine.Godot/x86_64/stable");
        assert!(
            Dependants::dependants(&FlatpakSource, &fixture.host(FakeRunner::new()), godot)
                .unwrap()
                .is_empty()
        );
    }

    fn removal_argv(id: &str) -> Vec<Step> {
        let packages = scan_with(&[]);
        let fixture = Fixture::new();
        let plan = FlatpakSource
            .plan(&fixture.host(FakeRunner::new()), find(&packages, id))
            .unwrap();
        assert_eq!(plan.tier, Tier::PackageManager);
        assert!(plan.blockers.is_empty());
        plan.steps
    }

    #[test]
    fn removal_uninstalls_the_exact_ref_from_its_installation() {
        assert_eq!(
            removal_argv("user/app/com.github.tchx84.Flatseal/x86_64/stable"),
            [Step::run(
                Actor::User,
                [
                    "flatpak",
                    "uninstall",
                    "--noninteractive",
                    "--user",
                    "app/com.github.tchx84.Flatseal/x86_64/stable"
                ]
            )]
        );
        assert_eq!(
            removal_argv("system/runtime/org.freedesktop.Platform.GL.default/x86_64/25.08-extra"),
            [Step::run(
                Actor::User,
                [
                    "flatpak",
                    "uninstall",
                    "--noninteractive",
                    "--system",
                    "runtime/org.freedesktop.Platform.GL.default/x86_64/25.08-extra"
                ]
            )]
        );
        assert_eq!(
            removal_argv("extra-disk/app/org.mozilla.firefox/aarch64/stable"),
            [Step::run(
                Actor::User,
                [
                    "flatpak",
                    "uninstall",
                    "--noninteractive",
                    "--installation=extra-disk",
                    "app/org.mozilla.firefox/aarch64/stable"
                ]
            )]
        );
    }

    #[test]
    fn probe_reads_the_version() {
        let fixture = Fixture::new();
        let runner = FakeRunner::new().respond("flatpak --version", "Flatpak 1.16.1\n");
        assert_eq!(
            FlatpakSource.probe(&fixture.host(runner)),
            Availability::Available {
                version: Some("1.16.1".into())
            }
        );
        assert!(matches!(
            FlatpakSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Unavailable { .. }
        ));
    }

    mod files {
        use super::*;
        use crate::formats::test_support::inspect;
        use housekeeper_core::Fit;

        const REF: &str = "[Flatpak Ref]\nName=org.gnome.Calculator\nBranch=stable\nTitle=Calculator\n\
                           Url=https://dl.flathub.org/repo/\nIsRuntime=false\n";

        #[test]
        fn a_flatpakref_names_the_app_and_its_repository() {
            let found = inspect("ID=fedora\n", "calc.flatpakref", REF.as_bytes(), |_| {
                FakeRunner::new().respond("flatpak --version", "Flatpak 1.16.0\n")
            })
            .unwrap();
            assert_eq!(found.fit, Fit::Fits);
            let package = found.package.unwrap();
            assert_eq!(package.name.as_deref(), Some("org.gnome.Calculator"));
            assert_eq!(package.title.as_deref(), Some("Calculator"));
            assert_eq!(package.origin.as_deref(), Some("https://dl.flathub.org/repo/"));
        }

        #[test]
        fn without_flatpak_a_flatpakref_does_not_fit() {
            let found = inspect("ID=ubuntu\n", "calc.flatpakref", REF.as_bytes(), |_| FakeRunner::new()).unwrap();
            assert!(matches!(found.fit, Fit::SourceMissing { .. }), "{:?}", found.fit);
        }
    }
}
