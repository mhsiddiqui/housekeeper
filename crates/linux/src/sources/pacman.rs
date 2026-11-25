//! Arch packages (Arch, Manjaro, EndeavourOS…), read from pacman's local database files.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, Cmd, Dependants, FileList, Host, InstallReason, OwnerLookup, Package, PackageKind, ScanContext,
    Scope, Source, SourceError, SourceId, SourceInfo, SourceKind,
};

use crate::owner::{OwnerQuery, args};
use crate::provider::{Context, Family, LinuxSource, Native, register};

pub const ID: SourceId = SourceId::new("pacman");

const PACMAN_CONF: &str = "/etc/pacman.conf";

/// pacman's compiled-in database path, used when `pacman.conf` does not set `DBPath`.
const DEFAULT_DB_PATH: &str = "/var/lib/pacman/";

/// Origin of packages no sync repository provides.
const FOREIGN_ORIGIN: &str = "AUR or local build";

/// Packages whose removal leaves the system unbootable or unmanageable.
const CORE_PACKAGES: &[&str] = &[
    // Metapackage defining a minimal Arch install; removing it orphans the base system.
    "base",
    // C library every program links against.
    "glibc",
    // Init system and service manager.
    "systemd",
    // Directory layout and the base /etc files.
    "filesystem",
    // /bin/sh and the basic commands scripts rely on.
    "bash",
    "coreutils",
    // The package manager; without it nothing can be repaired.
    "pacman",
    // Official kernels; the bootable kernel must stay.
    "linux",
    "linux-lts",
    "linux-zen",
    "linux-hardened",
    "linux-rt",
    "linux-rt-lts",
];

pub struct PacmanSource;

impl LinuxSource for PacmanSource {
    const NATIVE: Option<Native> = Some(Native {
        families: &[Family {
            name: "arch",
            distro_ids: &[
                "arch",
                "manjaro",
                "endeavouros",
                "garuda",
                "cachyos",
                "artix",
                "arcolinux",
                "steamos",
            ],
        }],
        program: "pacman",
    });

    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(PacmanSource);

impl Source for PacmanSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("pacman", "Arch packages", SourceKind::System)
            .evidence("pacman's local database (`local/` under its `DBPath`)")
    }

    fn probe(&self, host: &Host) -> Availability {
        if !host.has_program("pacman") {
            return Availability::unavailable("pacman is not installed");
        }
        let version = host
            .run_ok(&Cmd::new("pacman").arg("--version"))
            .ok()
            .and_then(|out| parse_version(&out));
        Availability::Available { version }
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let local = local_db(host)?;
        // Exits 1 when there are no foreign packages; either way, origins are optional.
        let foreign: HashSet<String> = host
            .run_ok(&Cmd::new("pacman").arg("-Qmq"))
            .map(|out| {
                out.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut packages = Vec::new();
        for (desc_path, desc) in local_descs(host, &local)? {
            packages.push(parse_desc(&desc, &foreign).map_err(|detail| entry_error(&desc_path, detail))?);
        }
        Ok(packages)
    }

    fn owner_lookup(&self) -> Option<&dyn OwnerLookup> {
        Some(self)
    }

    fn file_list(&self) -> Option<&dyn FileList> {
        Some(self)
    }

    fn dependant_lookup(&self) -> Option<&dyn Dependants> {
        Some(self)
    }
}

impl FileList for PacmanSource {
    fn files(&self, host: &Host, package: &Package) -> Result<Vec<PathBuf>, SourceError> {
        let version = package
            .version
            .as_deref()
            .ok_or_else(|| SourceError::parse("pacman database", format!("{} has no version", package.id)))?;
        // Entries are `<name>-<version>` directories; the inventory's id is the name.
        let files_path = local_db(host)?
            .join(format!("{}-{version}", package.name))
            .join("files");
        let text = host
            .read_optional(&files_path)?
            .ok_or_else(|| SourceError::parse("pacman database", format!("{} is missing", files_path.display())))?;
        let sections = sections(&text).map_err(|detail| entry_error(&files_path, detail))?;
        Ok(sections
            .into_iter()
            .filter(|(name, _)| *name == "FILES")
            .flat_map(|(_, values)| values)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(|path| PathBuf::from(format!("/{}", path.trim_end_matches('/'))))
            .collect())
    }
}

impl Dependants for PacmanSource {
    fn dependants(&self, host: &Host, package: &Package) -> Result<Vec<String>, SourceError> {
        let mut entries = Vec::new();
        for (desc_path, desc) in local_descs(host, &local_db(host)?)? {
            entries.push(relations(&desc).map_err(|detail| entry_error(&desc_path, detail))?);
        }
        let mut targets: HashSet<&str> = HashSet::from([package.name.as_str()]);
        if let Some(target) = entries.iter().find(|entry| entry.name == package.name) {
            targets.extend(target.provides.iter().map(String::as_str));
        }
        let found: BTreeSet<&str> = entries
            .iter()
            .filter(|entry| entry.name != package.name)
            .filter(|entry| entry.depends.iter().any(|dep| targets.contains(dep.as_str())))
            .map(|entry| entry.name.as_str())
            .collect();
        Ok(found.into_iter().map(str::to_string).collect())
    }
}

/// The `local/` directory of pacman's database.
fn local_db(host: &Host) -> Result<PathBuf, SourceError> {
    Ok(db_path(host.read_optional(PACMAN_CONF)?.as_deref()).join("local"))
}

/// Every entry's `desc` path and text.
fn local_descs(host: &Host, local: &Path) -> Result<Vec<(PathBuf, String)>, SourceError> {
    let mut descs = Vec::new();
    for entry in host.read_dir(local)? {
        // `ALPM_DB_VERSION` is a plain file next to the package directories.
        if !host.is_dir(&entry.path) {
            continue;
        }
        let desc_path = entry.path.join("desc");
        let desc = host
            .read_optional(&desc_path)?
            .ok_or_else(|| SourceError::parse("pacman database", format!("{} is missing", desc_path.display())))?;
        descs.push((desc_path, desc));
    }
    Ok(descs)
}

fn entry_error(path: &Path, detail: String) -> SourceError {
    SourceError::parse(format!("pacman database entry {}", path.display()), detail)
}

/// A package's name and the names in its `%PROVIDES%` and `%DEPENDS%`, without versions or descriptions.
struct Relations {
    name: String,
    provides: Vec<String>,
    depends: Vec<String>,
}

fn relations(desc: &str) -> Result<Relations, String> {
    let (mut name, mut provides, mut depends) = (None, Vec::new(), Vec::new());
    for (section, values) in sections(desc)? {
        let names = || {
            values
                .iter()
                .map(|v| bare_name(v))
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        match section {
            "NAME" => name = values.first().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()),
            "PROVIDES" => provides.extend(names()),
            "DEPENDS" => depends.extend(names()),
            _ => {}
        }
    }
    Ok(Relations {
        name: name.ok_or("no %NAME%")?,
        provides,
        depends,
    })
}

/// `libfoo.so=1-64`, `python>=3.12` or `foo: why` → the bare name.
fn bare_name(entry: &str) -> &str {
    let entry = entry.split_once(':').map_or(entry, |(name, _)| name);
    entry.split(['<', '>', '=']).next().unwrap_or_default().trim()
}

impl OwnerLookup for PacmanSource {
    fn owners(&self, host: &Host, paths: &[PathBuf]) -> Result<HashMap<PathBuf, Vec<String>>, SourceError> {
        OwnerQuery {
            command: |paths| Cmd::new("pacman").arg("-Qo").args(args(paths)),
            // Unowned paths make pacman exit 1 (messages on stderr); only a killed pacman is a failure.
            failed: |output| output.code.is_none(),
            parse: owners,
        }
        .run(host, paths)
    }
}

/// Lines are `/path is owned by pkg version`, one per owner; directories get a trailing slash. Errors go to stderr.
fn owners(stdout: &str) -> Vec<(&str, &str)> {
    stdout
        .lines()
        .filter_map(|line| line.rsplit_once(" is owned by "))
        .filter_map(|(path, owner)| {
            let package = owner.split_whitespace().next()?;
            Some((path.trim_end_matches('/'), package))
        })
        .collect()
}

/// The `Pacman v7.0.0 - libalpm v15.0.0` token in `pacman --version`, which sits beside ASCII art.
fn parse_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut words = line.split_whitespace().skip_while(|word| *word != "Pacman");
        words.next()?;
        words.next()?.strip_prefix('v').map(str::to_string)
    })
}

/// `DBPath` from the `[options]` section of pacman.conf, or the default.
fn db_path(conf: Option<&str>) -> PathBuf {
    let mut in_options = false;
    for line in conf.unwrap_or_default().lines().map(str::trim) {
        if line.starts_with('[') {
            in_options = line == "[options]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if in_options && key.trim() == "DBPath" && !value.trim().is_empty() {
            return PathBuf::from(value.trim());
        }
    }
    PathBuf::from(DEFAULT_DB_PATH)
}

/// `%SECTION%` headers, each followed by value lines up to a blank line.
fn sections(text: &str) -> Result<Vec<(&str, Vec<&str>)>, String> {
    let mut sections = Vec::new();
    let mut lines = text.lines();
    while let Some(header) = lines.next() {
        let header = header.trim();
        if header.is_empty() {
            continue;
        }
        if !(header.len() > 2 && header.starts_with('%') && header.ends_with('%')) {
            return Err(format!("expected a %SECTION% header, found {header:?}"));
        }
        let value: Vec<&str> = lines.by_ref().take_while(|line| !line.is_empty()).collect();
        sections.push((&header[1..header.len() - 1], value));
    }
    Ok(sections)
}

/// One `desc` file.
fn parse_desc(desc: &str, foreign: &HashSet<String>) -> Result<Package, String> {
    let (mut name, mut version, mut summary, mut size, mut installed_at, mut reason) =
        (None, None, None, None, None, InstallReason::Explicit);
    for (section, value) in sections(desc)? {
        let first = value.first().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        match section {
            "NAME" => name = first,
            "VERSION" => version = first,
            "DESC" => summary = first,
            "SIZE" => size = first.and_then(|v| v.parse().ok()),
            "INSTALLDATE" => installed_at = first.and_then(|v| v.parse().ok()),
            "REASON" if first.as_deref() == Some("1") => reason = InstallReason::Dependency,
            _ => {}
        }
    }
    let name = name.ok_or("no %NAME%")?;
    let version = version.ok_or("no %VERSION%")?;
    let mut package = Package::new(ID, name.clone(), name.clone(), Scope::System);
    package.version = Some(version);
    package.summary = summary;
    package.size_bytes = size;
    package.installed_at = installed_at;
    package.reason = reason;
    package.protected = CORE_PACKAGES.contains(&name.as_str());
    package.origin = foreign.contains(&name).then(|| FOREIGN_ORIGIN.to_string());
    // Multilib and split-library names; plain `lib*` also covers apps such as libreoffice.
    if name.starts_with("lib32-") || name.ends_with("-libs") {
        package.kind = PackageKind::Library;
    }
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::Output;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const FIREFOX: &str = "\
%NAME%
firefox

%VERSION%
132.0.2-1

%BASE%
firefox

%DESC%
Fast, Private & Safe Web Browser

%URL%
https://www.mozilla.org/firefox/

%ARCH%
x86_64

%BUILDDATE%
1731500000

%INSTALLDATE%
1731600000

%PACKAGER%
Jan Alexander Steffens (heftig) <heftig@archlinux.org>

%SIZE%
254651234

%LICENSE%
MPL-2.0

%VALIDATION%
pgp

%DEPENDS%
dbus
gtk3
";

    const GLIBC: &str = "\
%NAME%
glibc

%VERSION%
2.40+r16+gaa533d58ff-2

%DESC%
GNU C Library

%INSTALLDATE%
1720000000

%SIZE%
48700000

%REASON%
1

%GROUPS%
base-devel
";

    const LIB32_MESA: &str = "\
%NAME%
lib32-mesa

%VERSION%
1:24.2.7-1

%DESC%
Open-source OpenGL drivers (32-bit)

%SIZE%
120000000

%REASON%
1
";

    const YAY: &str = "\
%NAME%
yay

%VERSION%
12.4.2-1

%DESC%
Yet another yogurt. Pacman wrapper and AUR helper written in go.

%INSTALLDATE%
1730000000

%SIZE%
9876543

%REASON%
0
";

    fn database(fixture: &Fixture, db: &str) {
        fixture.file(&format!("{db}local/ALPM_DB_VERSION"), "9\n");
        fixture.file(&format!("{db}local/firefox-132.0.2-1/desc"), FIREFOX);
        fixture.file(&format!("{db}local/firefox-132.0.2-1/files"), "%FILES%\nusr/\n");
        fixture.file(&format!("{db}local/glibc-2.40+r16+gaa533d58ff-2/desc"), GLIBC);
        fixture.file(&format!("{db}local/lib32-mesa-1:24.2.7-1/desc"), LIB32_MESA);
        fixture.file(&format!("{db}local/yay-12.4.2-1/desc"), YAY);
    }

    fn scan(fixture: &Fixture) -> Vec<Package> {
        let runner = FakeRunner::new().respond("pacman -Qmq", "yay\n");
        PacmanSource
            .inventory(&fixture.host(runner), &ScanContext::default())
            .unwrap()
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn reads_the_local_database() {
        let fixture = Fixture::new();
        database(&fixture, DEFAULT_DB_PATH);
        let packages = scan(&fixture);
        let ids: Vec<_> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["firefox", "glibc", "lib32-mesa", "yay"]);
        let firefox = find(&packages, "firefox");
        assert_eq!(firefox.version.as_deref(), Some("132.0.2-1"));
        assert_eq!(firefox.summary.as_deref(), Some("Fast, Private & Safe Web Browser"));
        assert_eq!(firefox.size_bytes, Some(254_651_234));
        assert_eq!(firefox.installed_at, Some(1_731_600_000));
        assert_eq!(find(&packages, "lib32-mesa").version.as_deref(), Some("1:24.2.7-1"));
    }

    #[test]
    fn reason_and_foreign_origin() {
        let fixture = Fixture::new();
        database(&fixture, DEFAULT_DB_PATH);
        let packages = scan(&fixture);
        assert_eq!(find(&packages, "firefox").reason, InstallReason::Explicit);
        assert_eq!(find(&packages, "yay").reason, InstallReason::Explicit);
        assert_eq!(find(&packages, "glibc").reason, InstallReason::Dependency);
        assert_eq!(find(&packages, "lib32-mesa").reason, InstallReason::Dependency);
        assert_eq!(find(&packages, "yay").origin.as_deref(), Some(FOREIGN_ORIGIN));
        assert_eq!(find(&packages, "firefox").origin, None);
    }

    #[test]
    fn no_foreign_packages_is_not_an_error() {
        let fixture = Fixture::new();
        database(&fixture, DEFAULT_DB_PATH);
        let runner = FakeRunner::new().respond_with(
            "pacman -Qmq",
            Output {
                code: Some(1),
                ..Output::default()
            },
        );
        let packages = PacmanSource
            .inventory(&fixture.host(runner), &ScanContext::default())
            .unwrap();
        assert_eq!(packages.len(), 4);
        assert!(packages.iter().all(|p| p.origin.is_none()));
    }

    #[test]
    fn honours_db_path_from_pacman_conf() {
        let fixture = Fixture::new();
        fixture.file(
            PACMAN_CONF,
            "[options]\n#DBPath = /var/lib/pacman/\nDBPath      = /srv/pacman/\nHoldPkg = pacman glibc\n\n\
             [core]\nInclude = /etc/pacman.d/mirrorlist\n",
        );
        database(&fixture, "/srv/pacman/");
        fixture.file(
            "/var/lib/pacman/local/stale-1-1/desc",
            "%NAME%\nstale\n\n%VERSION%\n1-1\n",
        );
        let ids: Vec<_> = scan(&fixture).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["firefox", "glibc", "lib32-mesa", "yay"]);
    }

    #[test]
    fn db_path_outside_options_is_ignored() {
        assert_eq!(
            db_path(Some("[custom]\nDBPath = /x/\n")),
            PathBuf::from(DEFAULT_DB_PATH)
        );
        assert_eq!(db_path(None), PathBuf::from(DEFAULT_DB_PATH));
    }

    #[test]
    fn malformed_desc_is_an_error() {
        let fixture = Fixture::new();
        database(&fixture, DEFAULT_DB_PATH);
        fixture.file("/var/lib/pacman/local/broken-1-1/desc", "%VERSION%\n1-1\n");
        let runner = FakeRunner::new().respond("pacman -Qmq", "");
        assert!(matches!(
            PacmanSource.inventory(&fixture.host(runner), &ScanContext::default()),
            Err(SourceError::Parse { .. })
        ));
        assert!(parse_desc("garbage\n", &HashSet::new()).is_err());
    }

    #[test]
    fn core_packages_are_protected_and_multilib_is_library() {
        let fixture = Fixture::new();
        database(&fixture, DEFAULT_DB_PATH);
        let packages = scan(&fixture);
        assert!(find(&packages, "glibc").protected);
        assert!(!find(&packages, "firefox").protected);
        assert_eq!(find(&packages, "lib32-mesa").kind, PackageKind::Library);
        assert_eq!(find(&packages, "firefox").kind, PackageKind::Unknown);
    }

    #[test]
    fn probe_reads_the_version_next_to_the_logo() {
        let output = "\n .--.                  Pacman v7.0.0 - libalpm v15.0.0\n\
                      / _.-' .-.  .-.  .-.   Copyright (C) 2006-2024 Pacman Development Team\n";
        let runner = FakeRunner::new().respond("pacman --version", output);
        assert_eq!(
            PacmanSource.probe(&Fixture::new().host(runner)),
            Availability::Available {
                version: Some("7.0.0".into())
            }
        );
    }

    #[test]
    fn owner_lookup_reads_owner_lines_and_directory_slashes() {
        use crate::owner::tests::{exit, owned, paths, sorted};
        let runner = FakeRunner::new().respond_with(
            "pacman -Qo /opt/visual-studio-code /usr/local/bin/tool /opt/manual",
            exit(
                1,
                "/opt/visual-studio-code/ is owned by visual-studio-code-bin 1.95.3-1\n\
                 /usr/local/bin/tool is owned by tool-git r12.abc-1\n",
                "error: No package owns /opt/manual\n",
            ),
        );
        let host = Fixture::new().host(runner);
        let owners = PacmanSource
            .owners(
                &host,
                &paths(&["/opt/visual-studio-code", "/usr/local/bin/tool", "/opt/manual"]),
            )
            .unwrap();
        assert_eq!(
            sorted(owners),
            [
                owned("/opt/visual-studio-code", &["visual-studio-code-bin"]),
                owned("/usr/local/bin/tool", &["tool-git"]),
            ]
        );
    }

    #[test]
    fn files_are_absolute_without_directory_slashes() {
        let fixture = Fixture::new();
        database(&fixture, DEFAULT_DB_PATH);
        fixture.file(
            "/var/lib/pacman/local/lib32-mesa-1:24.2.7-1/files",
            "%FILES%\nusr/\nusr/lib32/\nusr/lib32/libGL.so.1\n\n%BACKUP%\netc/drirc\tabc\n",
        );
        let packages = scan(&fixture);
        let host = fixture.host(FakeRunner::new());
        let files = PacmanSource.files(&host, find(&packages, "lib32-mesa")).unwrap();
        let expected = ["/usr", "/usr/lib32", "/usr/lib32/libGL.so.1"].map(PathBuf::from);
        assert_eq!(files, expected);
    }

    #[test]
    fn dependants_through_name_and_provides_but_not_optdepends() {
        let fixture = Fixture::new();
        database(&fixture, DEFAULT_DB_PATH);
        let local = "/var/lib/pacman/local";
        fixture.file(
            &format!("{local}/gtk3-1:3.24.43-4/desc"),
            "%NAME%\ngtk3\n\n%VERSION%\n1:3.24.43-4\n\n%PROVIDES%\ngtk3-print-backends\nlibgtk-3.so=0-64\n\n\
             %DEPENDS%\nglibc\n",
        );
        fixture.file(
            &format!("{local}/zenity-4.0.2-1/desc"),
            "%NAME%\nzenity\n\n%VERSION%\n4.0.2-1\n\n%DEPENDS%\ngtk3>=3.24\n",
        );
        fixture.file(
            &format!("{local}/gimp-2.10.38-1/desc"),
            "%NAME%\ngimp\n\n%VERSION%\n2.10.38-1\n\n%DEPENDS%\nlibgtk-3.so=0-64\n",
        );
        fixture.file(
            &format!("{local}/vlc-3.0.21-1/desc"),
            "%NAME%\nvlc\n\n%VERSION%\n3.0.21-1\n\n%OPTDEPENDS%\ngtk3: dialogs\n",
        );
        let packages = scan(&fixture);
        let host = fixture.host(FakeRunner::new());
        let dependants = PacmanSource.dependants(&host, find(&packages, "gtk3")).unwrap();
        assert_eq!(dependants, ["firefox", "gimp", "zenity"]);
    }
}
