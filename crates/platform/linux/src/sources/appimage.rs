//! AppImages: single-file apps found in the places people usually keep them, and installed by keeping them in
//! `~/Applications` with a menu entry and their icon (decision D48).

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use housekeeper_core::{
    Actor, Availability, FileInstaller, FilePackage, FileType, Host, InstallFile, InstallPlan, InstallReason, Package,
    PackageKind, RemovalPlan, Remover, ScanContext, Source, SourceError, SourceId, SourceInfo, SourceKind, Step, Tier,
};

use crate::desktop::{appimage_entry, icon_of};
use crate::files::{is_appimage, is_appimage_head, looks_like_version, resolve_symlinks, scope_of};
use crate::formats::{elf_machine, path_arg};
use crate::provider::{Context, FileFormat, LinuxSource, register};
use crate::squashfs;

pub const ID: SourceId = SourceId::new("appimage");

/// Directories scanned for AppImages, relative to the home directory. Their files only, not subdirectories.
const HOME_DIRS: &[&str] = &["Applications", "AppImages", ".local/bin", "bin", "Downloads", "Desktop"];

/// Where an installed AppImage is kept, relative to the home directory.
const APPLICATIONS: &str = "Applications";

/// Below `$XDG_DATA_HOME`: the icons taken out of the AppImages Housekeeper installs, one folder each.
const ICONS: &str = "icons/appimage";

/// Where an AppImage's own icon may be, at the root of its image, after the name its entry's `Icon` gives.
const ICON_EXTENSIONS: [&str; 4] = ["png", "svg", "svgz", "xpm"];

/// Home directories that are on the PATH, so an AppImage in (or linked from) them is a command.
const PATH_DIRS: &[&str] = &[".local/bin", "bin"];

/// Architecture and OS words release names carry around the version.
const PLATFORM_WORDS: &[&str] = &[
    "x86_64", "x86-64", "x86", "amd64", "x64", "aarch64", "arm64", "armhf", "armv7l", "i386", "i686", "linux",
    "linux64", "glibc",
];

pub struct AppImageSource;

impl LinuxSource for AppImageSource {
    const FILES: &'static [FileFormat] = &[FileFormat {
        id: "appimage",
        title: "AppImage",
        suffixes: &[],
        matches: is_appimage_head,
        evidence: "an ELF program marked `AI` in its header, whatever its name",
        read: read_appimage_file,
    }];

    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(AppImageSource);

/// The name and version its file name gives (as the source lists it once kept), and the processor its ELF header
/// names. Nothing inside is run or unpacked.
fn read_appimage_file(_: &Host, path: &Path, head: &[u8]) -> Result<Option<FilePackage>, SourceError> {
    let file_name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
    let (name, version) = name_and_version(&file_name);
    Ok(Some(FilePackage {
        name: Some(name),
        version,
        arch: elf_machine(head),
        ..FilePackage::default()
    }))
}

impl Source for AppImageSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("appimage", "AppImage", SourceKind::Portable)
            .evidence("an AppImage file (told by its ELF header) in a folder where AppImages are kept")
    }

    fn probe(&self, _host: &Host) -> Availability {
        Availability::Available { version: None }
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let home = host.home();
        let mut dirs: Vec<PathBuf> = HOME_DIRS.iter().map(|dir| home.join(dir)).collect();
        for entry in host.read_dir(home.join("Applications"))? {
            if entry.file_type == Some(FileType::Dir) {
                dirs.push(entry.path);
            }
        }
        dirs.push(PathBuf::from("/opt"));
        let path_dirs: Vec<PathBuf> = PATH_DIRS.iter().map(|dir| home.join(dir)).collect();

        let mut packages: Vec<Package> = Vec::new();
        let mut by_target: HashMap<PathBuf, usize> = HashMap::new();
        for dir in &dirs {
            for entry in host.read_dir(dir)? {
                if !matches!(entry.file_type, Some(FileType::File { .. })) || !is_appimage(host, &entry.path) {
                    continue;
                }
                let target = resolve_symlinks(host, &entry.path);
                let index = match by_target.get(&target) {
                    Some(&index) => index,
                    None => {
                        packages.push(package(host, &target));
                        by_target.insert(target, packages.len() - 1);
                        packages.len() - 1
                    }
                };
                if path_dirs.contains(dir) && !packages[index].commands.contains(&entry.name) {
                    packages[index].commands.push(entry.name);
                }
            }
        }
        Ok(packages)
    }

    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }

    fn file_installer(&self) -> Option<&dyn FileInstaller> {
        Some(self)
    }
}

/// An AppImage is removed by its convention: the file itself, then the launchers made for it (those that start it,
/// directly or through a symlink) and the icon folders Housekeeper made for those. Launcher files they hide are left
/// alone: they were not made for this file and may belong to a package.
impl Remover for AppImageSource {
    fn plan(&self, host: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        let file = package.location.clone().unwrap_or_else(|| PathBuf::from(&package.id));
        let by = |path: &Path| {
            if path.starts_with(host.home()) {
                Actor::User
            } else {
                Actor::Helper
            }
        };
        let mut steps = vec![Step::trash(by(&file), &file)];
        let launchers = package.launchers.iter().filter(|launcher| {
            launcher
                .program
                .as_deref()
                .is_some_and(|program| program == file || resolve_symlinks(host, program) == file)
        });
        let icons = host.data_home().join(ICONS);
        let mut folders: Vec<PathBuf> = Vec::new();
        for launcher in launchers {
            steps.push(Step::trash(by(&launcher.path), &launcher.path));
            let folder = launcher
                .icon
                .as_deref()
                .and_then(|icon| Path::new(icon).strip_prefix(&icons).ok())
                .and_then(|inside| inside.components().next())
                .map(|first| icons.join(first));
            if let Some(folder) = folder.filter(|folder| !folders.contains(folder) && host.exists(folder)) {
                folders.push(folder);
            }
        }
        steps.extend(folders.iter().map(|folder| Step::trash(Actor::User, folder)));
        Ok(RemovalPlan::new(Tier::FormatConvention, steps))
    }
}

/// An AppImage is installed by its convention, as the user (D48): moved into `~/Applications` (unless it is there
/// already), made executable, its icon taken out of its image into a folder of its own, and a menu entry written that
/// starts it, made from the entry it carries. Installing it does not touch other copies of the same app.
impl FileInstaller for AppImageSource {
    fn plan(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallPlan, SourceError> {
        let applications = host.home().join(APPLICATIONS);
        let file_name = file
            .path
            .file_name()
            .ok_or_else(|| SourceError::parse("AppImage path", file.path.display().to_string()))?;
        let target = if file.path.starts_with(&applications) {
            file.path.to_path_buf()
        } else {
            applications.join(file_name)
        };
        let name = file
            .package
            .and_then(|package| package.name.clone())
            .unwrap_or_else(|| file_name.to_string_lossy().into_owned());
        let slug = slug(&name);
        let entry_path = host
            .data_home()
            .join("applications")
            .join(format!("appimage-{slug}.desktop"));
        let icon_dir = host.data_home().join(ICONS).join(&slug);
        let mut steps = Vec::new();
        let mut blockers = Vec::new();
        let mut cautions = Vec::new();
        let taken = |path: &Path| format!("{} is there already; remove what is there first", path.display());
        if target != file.path {
            if host.symlink_metadata(&target).is_some() {
                blockers.push(taken(&target));
            } else if !same_file_system(host, file.path, &applications) {
                blockers.push(format!(
                    "it is on another file system than {}, where it would be kept; move it into your home folder first",
                    applications.display()
                ));
            }
            steps.push(Step::move_file(Actor::User, file.path, &target));
        }
        steps.push(Step::run(
            Actor::User,
            ["chmod".to_string(), "u+x".into(), path_arg(host, &target)],
        ));
        let inside = inside(host, file.path).unwrap_or_else(|why| {
            cautions.push(format!("its menu entry is named after its file and has no icon: {why}"));
            Inside::default()
        });
        let icon = inside.icon.map(|(offset, member)| {
            if host.symlink_metadata(&icon_dir).is_some() {
                blockers.push(taken(&icon_dir));
            }
            // unsquashfs makes its destination folder, but not the folders above it.
            let icons = host.data_home().join(ICONS);
            steps.push(Step::run(
                Actor::User,
                ["mkdir".to_string(), "-p".into(), path_arg(host, &icons)],
            ));
            steps.push(Step::run(
                Actor::User,
                [
                    "unsquashfs".to_string(),
                    "-o".into(),
                    offset.to_string(),
                    "-q".into(),
                    "-n".into(),
                    "-no-xattrs".into(),
                    "-d".into(),
                    path_arg(host, &icon_dir),
                    path_arg(host, &target),
                    member.clone(),
                ],
            ));
            icon_dir.join(member)
        });
        if host.symlink_metadata(&entry_path).is_some() {
            blockers.push(taken(&entry_path));
        }
        let contents = appimage_entry(inside.entry.as_deref(), &target, &name, icon.as_deref());
        steps.push(Step::write_file(Actor::User, &entry_path, contents));
        let mut plan = InstallPlan::new(steps);
        plan.blockers = blockers;
        plan.cautions = cautions;
        Ok(plan)
    }

    fn replaces_installed(&self) -> bool {
        false
    }
}

/// What an AppImage carries that its menu entry uses: its own desktop entry, and its icon as a file of its image with
/// the image's offset.
#[derive(Debug, Default)]
struct Inside {
    entry: Option<String>,
    icon: Option<(u64, String)>,
}

/// Reads, with `unsquashfs`, the desktop entry at the root of a type 2 AppImage's image and the icon its `Icon` names
/// there (any of [`ICON_EXTENSIONS`]), or else its `.DirIcon`; links inside the image are followed. `Err` says why
/// nothing could be read.
fn inside(host: &Host, path: &Path) -> Result<Inside, String> {
    if !host.has_program("unsquashfs") {
        return Err("unsquashfs (from squashfs-tools) is not installed to read inside it".into());
    }
    let head = host.read_head(path, 64).map_err(|e| e.to_string())?;
    if head.get(10) != Some(&2) {
        return Err("it is a type 1 AppImage, whose ISO 9660 image Housekeeper does not read".into());
    }
    let offset = squashfs::elf_end(&head).ok_or("its ELF header ends early")?;
    let members = squashfs::list(host, path, offset).map_err(|e| e.to_string())?;
    let entry = members
        .iter()
        .find(|member| !member.path.contains('/') && member.path.ends_with(".desktop"))
        .map(|member| squashfs::cat(host, path, offset, &member.path))
        .transpose()
        .map_err(|e| e.to_string())?;
    let named = entry
        .as_deref()
        .and_then(icon_of)
        .filter(|icon| !icon.contains('/'))
        .map(|icon| ICON_EXTENSIONS.map(|extension| format!("{icon}.{extension}")))
        .into_iter()
        .flatten();
    let icon = named
        .chain([".DirIcon".to_string()])
        .find_map(|candidate| squashfs::resolve(&members, &candidate))
        .map(|member| (offset, member));
    Ok(Inside { entry, icon })
}

/// Whether `file` is on the file system of `dir`, or of the nearest folder above it that exists; `true` when that
/// cannot be told.
fn same_file_system(host: &Host, file: &Path, dir: &Path) -> bool {
    let device = |path: &Path| host.metadata(path).map(|meta| meta.dev());
    let folder = dir.ancestors().find(|folder| host.is_dir(folder));
    match (device(file), folder.and_then(device)) {
        (Some(file), Some(folder)) => file == folder,
        _ => true,
    }
}

/// A name for files made for an app: lowercase letters and digits, other runs of characters as one `-`.
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() { "app".into() } else { slug }
}

fn package(host: &Host, path: &Path) -> Package {
    let file_name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let (name, version) = name_and_version(&file_name);
    let mut package = Package::new(ID, path.to_string_lossy(), name, scope_of(host, path));
    package.version = version;
    package.kind = PackageKind::App;
    package.reason = InstallReason::Explicit;
    package.location = Some(path.to_path_buf());
    if let Some(meta) = host.metadata(path) {
        package.size_bytes = Some(meta.len());
        package.installed_at = Some(meta.mtime());
    }
    package
}

/// Name and version from a release file name: `Joplin-3.0.15-x86_64.AppImage` → (`Joplin`, `3.0.15`).
///
/// Words are separated by `-` or `_`. The version starts at the first later word that begins with a digit (or `v` and
/// a digit) and runs until an architecture or OS word; those words are dropped from the name.
fn name_and_version(file_name: &str) -> (String, Option<String>) {
    let stem = strip_suffix_ignore_case(file_name, ".appimage").unwrap_or(file_name);
    let words = words(stem);
    let is_platform = |&(start, end): &(usize, usize)| {
        PLATFORM_WORDS
            .iter()
            .any(|word| stem[start..end].eq_ignore_ascii_case(word))
    };
    let version_at = words
        .iter()
        .enumerate()
        .skip(1)
        .find(|&(_, &(start, end))| looks_like_version(&stem[start..end]))
        .map(|(index, _)| index);

    let name_words = &words[..version_at.unwrap_or(words.len())];
    let name = match name_words.iter().rposition(|word| !is_platform(word)) {
        Some(last) => &stem[name_words[0].0..name_words[last].1],
        None => stem,
    };
    let version = version_at.map(|first| {
        let last = words[first..]
            .iter()
            .position(is_platform)
            .map_or(words.len() - 1, |offset| first + offset - 1);
        let text = &stem[words[first].0..words[last].1];
        text.strip_prefix(['v', 'V']).unwrap_or(text).to_string()
    });
    (name.to_string(), version)
}

/// Byte ranges of the `-`/`_`-separated words, keeping `x86_64` and `x86-64` whole.
fn words(stem: &str) -> Vec<(usize, usize)> {
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for (index, c) in stem.char_indices().chain([(stem.len(), '-')]) {
        if c != '-' && c != '_' {
            continue;
        }
        if index > start {
            match words.last_mut() {
                Some(last) if stem[last.0..last.1].eq_ignore_ascii_case("x86") && &stem[start..index] == "64" => {
                    last.1 = index;
                }
                _ => words.push((start, index)),
            }
        }
        start = index + 1;
    }
    words
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let split = text.len().checked_sub(suffix.len())?;
    let (head, tail) = (text.get(..split)?, text.get(split..)?);
    (tail.eq_ignore_ascii_case(suffix) && !head.is_empty()).then_some(head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::Scope;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};

    /// Start of an ELF header with the AppImage type 2 marker.
    const TYPE2: &[u8] = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00";
    const TYPE1: &[u8] = b"\x7fELF\x02\x01\x01\x00AI\x01\x00\x00\x00\x00\x00";
    /// A plain ELF executable: zero padding where AppImages put their marker.
    const PLAIN_ELF: &[u8] = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";

    fn scan(fixture: &Fixture) -> Vec<Package> {
        AppImageSource
            .inventory(&fixture.host(FakeRunner::new()), &ScanContext::default())
            .unwrap()
    }

    fn ids(packages: &[Package]) -> Vec<&str> {
        packages.iter().map(|p| p.id.as_str()).collect()
    }

    #[test]
    fn file_names_give_name_and_version() {
        let cases = [
            ("Obsidian-1.6.7.AppImage", "Obsidian", Some("1.6.7")),
            ("Joplin-3.0.15-x86_64.AppImage", "Joplin", Some("3.0.15")),
            ("nvim.appimage", "nvim", None),
            ("appimagetool-x86_64.AppImage", "appimagetool", None),
            ("LM-Studio-0.3.5-2-x64.AppImage", "LM-Studio", Some("0.3.5-2")),
            (
                "FreeCAD_1.0.0-conda-Linux-x86_64-py311.AppImage",
                "FreeCAD",
                Some("1.0.0-conda"),
            ),
            ("tool-linux-amd64-v2.4.AppImage", "tool", Some("2.4")),
            (
                "Cursor-0.42.3-build-241016kz7gs-x86_64.AppImage",
                "Cursor",
                Some("0.42.3-build-241016kz7gs"),
            ),
            ("0ad-0.0.26-x86-64.AppImage", "0ad", Some("0.0.26")),
            ("krita", "krita", None),
            (".AppImage", ".AppImage", None),
        ];
        for (file, name, version) in cases {
            let (got_name, got_version) = name_and_version(file);
            assert_eq!((got_name.as_str(), got_version.as_deref()), (name, version), "{file}");
        }
    }

    #[test]
    fn only_files_with_the_appimage_marker_are_listed() {
        let fixture = Fixture::new();
        fixture.executable(&format!("{HOME}/Applications/Obsidian-1.6.7.AppImage"), TYPE2);
        fixture.executable(&format!("{HOME}/Applications/old/Legacy-1.0.AppImage"), TYPE1);
        fixture.executable(&format!("{HOME}/Applications/old/deeper/Hidden-1.0.AppImage"), TYPE2);
        fixture.executable(&format!("{HOME}/.local/bin/rg"), PLAIN_ELF);
        fixture.file(&format!("{HOME}/Downloads/notes.txt"), "AI\x02");
        fixture.file(&format!("{HOME}/Downloads/tiny"), b"\x7fEL");
        fixture.file(&format!("{HOME}/Downloads/truncated"), b"\x7fELF\x02");
        fixture.file(&format!("{HOME}/Downloads/Joplin-3.0.15-x86_64.AppImage"), TYPE2);
        fixture.executable("/opt/Tool.AppImage", TYPE2);
        fixture.executable("/opt/nested/Inner.AppImage", TYPE2);
        let packages = scan(&fixture);
        assert_eq!(
            ids(&packages),
            [
                "/home/user/Applications/Obsidian-1.6.7.AppImage",
                "/home/user/Downloads/Joplin-3.0.15-x86_64.AppImage",
                "/home/user/Applications/old/Legacy-1.0.AppImage",
                "/opt/Tool.AppImage",
            ]
        );
        let obsidian = &packages[0];
        assert_eq!(obsidian.name, "Obsidian");
        assert_eq!(obsidian.version.as_deref(), Some("1.6.7"));
        assert_eq!(obsidian.size_bytes, Some(TYPE2.len() as u64));
        assert!(obsidian.installed_at.is_some());
        assert_eq!(obsidian.scope, Scope::User);
        assert_eq!(obsidian.reason, InstallReason::Explicit);
        assert_eq!(packages[3].scope, Scope::System);
    }

    #[test]
    fn symlinks_are_followed_and_deduplicated() {
        let fixture = Fixture::new();
        fixture.executable(&format!("{HOME}/Applications/Obsidian-1.6.7.AppImage"), TYPE2);
        fixture.symlink(
            &format!("{HOME}/.local/bin/obsidian"),
            "../../Applications/Obsidian-1.6.7.AppImage",
        );
        fixture.symlink(
            &format!("{HOME}/Desktop/Obsidian"),
            "/home/user/Applications/Obsidian-1.6.7.AppImage",
        );
        fixture.executable(&format!("{HOME}/Tools/Zettlr-3.2.0.AppImage"), TYPE2);
        fixture.symlink(&format!("{HOME}/bin/zettlr"), "/home/user/Tools/Zettlr-3.2.0.AppImage");
        fixture.symlink(&format!("{HOME}/bin/broken"), "/home/user/Tools/missing.AppImage");
        let packages = scan(&fixture);
        assert_eq!(
            ids(&packages),
            [
                "/home/user/Applications/Obsidian-1.6.7.AppImage",
                "/home/user/Tools/Zettlr-3.2.0.AppImage",
            ]
        );
        assert_eq!(packages[0].commands, ["obsidian"]);
        assert_eq!(packages[1].name, "Zettlr");
        assert_eq!(packages[1].commands, ["zettlr"]);
    }

    #[test]
    fn nothing_installed_is_empty() {
        assert!(scan(&Fixture::new()).is_empty());
    }

    fn launcher(path: &str, program: &str) -> housekeeper_core::Launcher {
        housekeeper_core::Launcher {
            id: "app.desktop".into(),
            name: "App".into(),
            icon: None,
            path: path.into(),
            shadows: Vec::new(),
            program: Some(program.into()),
            hidden: false,
        }
    }

    #[test]
    fn home_appimage_trashes_its_file_its_own_launchers_and_the_icons_made_for_them_as_the_user() {
        let fixture = Fixture::new();
        let file = format!("{HOME}/Applications/Obsidian-1.6.7.AppImage");
        let icons = format!("{HOME}/.local/share/icons/appimage");
        fixture.executable(&file, TYPE2);
        fixture.symlink(&format!("{HOME}/.local/bin/obsidian"), &file);
        fixture.file(&format!("{icons}/obsidian/obsidian.png"), "png");
        fixture.file(&format!("{icons}/other/other.png"), "png");
        let host = fixture.host(FakeRunner::new());
        let mut package = package(&host, Path::new(&file));
        let mut own = launcher(&format!("{HOME}/.local/share/applications/obsidian.desktop"), &file);
        own.shadows = vec![PathBuf::from("/usr/share/applications/obsidian.desktop")];
        own.icon = Some(format!("{icons}/obsidian/obsidian.png"));
        let mut other = launcher(
            &format!("{HOME}/.local/share/applications/other.desktop"),
            "/usr/bin/other",
        );
        other.icon = Some(format!("{icons}/other/other.png"));
        package.launchers = vec![
            own,
            launcher(
                &format!("{HOME}/.local/share/applications/cli.desktop"),
                &format!("{HOME}/.local/bin/obsidian"),
            ),
            other,
        ];
        let plan = Remover::plan(&AppImageSource, &host, &package).unwrap();
        assert_eq!(plan.tier, Tier::FormatConvention);
        assert_eq!(
            plan.steps,
            [
                Step::trash(Actor::User, &file),
                Step::trash(
                    Actor::User,
                    format!("{HOME}/.local/share/applications/obsidian.desktop")
                ),
                Step::trash(Actor::User, format!("{HOME}/.local/share/applications/cli.desktop")),
                Step::trash(Actor::User, format!("{icons}/obsidian")),
            ]
        );
    }

    #[test]
    fn opt_appimage_is_trashed_by_the_helper() {
        let fixture = Fixture::new();
        fixture.executable("/opt/Tool.AppImage", TYPE2);
        let host = fixture.host(FakeRunner::new());
        let package = package(&host, Path::new("/opt/Tool.AppImage"));
        let plan = Remover::plan(&AppImageSource, &host, &package).unwrap();
        assert_eq!(plan.steps, [Step::trash(Actor::Helper, "/opt/Tool.AppImage")]);
    }

    mod install {
        use std::sync::Arc;

        use housekeeper_core::{Action, CommandRunner, Housekeeper, InstallPlan};

        use super::*;
        use crate::LinuxPlatform;

        const FILE: &str = "Krita-5.2.6-x86_64.AppImage";
        /// Where the image starts in [`appimage`]: section headers at 1000, 10 of 64 bytes.
        const OFFSET: u64 = 1640;

        /// The first bytes of a type 2 AppImage built for this computer's processor.
        fn appimage() -> Vec<u8> {
            let mut head = vec![0u8; 64];
            head[..11].copy_from_slice(b"\x7fELF\x02\x01\x01\x00AI\x02");
            let machine: u16 = if std::env::consts::ARCH == "aarch64" {
                0xb7
            } else {
                0x3e
            };
            head[18..20].copy_from_slice(&machine.to_le_bytes());
            head[0x28..0x30].copy_from_slice(&1000u64.to_le_bytes());
            head[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
            head[0x3c..0x3e].copy_from_slice(&10u16.to_le_bytes());
            head
        }

        /// The plan for `~/Downloads/<FILE>` on Ubuntu, with `prepare` adding files first; `runner` is given the real
        /// path of the fixture's root.
        fn plan(prepare: impl FnOnce(&Fixture), runner: impl FnOnce(&str) -> FakeRunner) -> InstallPlan {
            let fixture = Fixture::new();
            fixture.file("/etc/os-release", "ID=ubuntu\nID_LIKE=debian\n");
            let path = format!("{HOME}/Downloads/{FILE}");
            fixture.file(&path, appimage());
            prepare(&fixture);
            let root = fixture.host(FakeRunner::new()).real_path("/");
            let runner: Arc<dyn CommandRunner> = Arc::new(runner(root.to_string_lossy().trim_end_matches('/')));
            let vars = [("HOME".to_string(), HOME.to_string())].into_iter().collect();
            let host = Host::new(root, HOME, vars, runner);
            let platform = LinuxPlatform::detect(&host);
            let inspection = Housekeeper::new(Box::new(platform), host)
                .inspect(Path::new(&path))
                .unwrap()
                .unwrap();
            inspection.install.unwrap()
        }

        const LISTING: &str = "\
-rw-r--r-- root/root 29032 2026-11-12 15:41 squashfs-root/.DirIcon
lrwxrwxrwx root/root    38 2026-11-12 15:41 squashfs-root/org.kde.krita.desktop -> usr/share/applications/org.kde.krita.desktop
lrwxrwxrwx root/root    48 2026-11-12 15:41 squashfs-root/krita.png -> usr/share/icons/hicolor/256x256/apps/krita.png
-rw-r--r-- root/root  6532 2026-11-12 15:41 squashfs-root/usr/share/icons/hicolor/256x256/apps/krita.png
";

        const ENTRY: &str = "[Desktop Entry]\nType=Application\nName=Krita\nExec=krita %F\nTryExec=krita\nIcon=krita\n\
                             Categories=Qt;KDE;Graphics;\n\n[Desktop Action new-window]\nName=New Window\n\
                             Exec=krita --new-window %F\n";

        #[test]
        fn a_downloaded_appimage_is_kept_in_applications_with_its_own_menu_entry_and_icon() {
            let plan = plan(
                |_| {},
                |root| {
                    let real = format!("{root}{HOME}/Downloads/{FILE}");
                    FakeRunner::new()
                        .respond(&format!("unsquashfs -o {OFFSET} -lls {real}"), LISTING)
                        .respond(
                            &format!("unsquashfs -o {OFFSET} -cat {real} org.kde.krita.desktop"),
                            ENTRY,
                        )
                },
            );
            assert!(plan.blockers.is_empty() && plan.cautions.is_empty(), "{plan:?}");
            assert_eq!(
                plan.steps[0],
                Step::move_file(
                    Actor::User,
                    format!("{HOME}/Downloads/{FILE}"),
                    format!("{HOME}/Applications/{FILE}")
                )
            );
            let argv = |n: usize| match &plan.steps[n].action {
                Action::Run { argv } => argv.iter().map(|arg| arg.rsplit(HOME).next().unwrap_or(arg)).collect(),
                other => panic!("{other:?}"),
            };
            let chmod: Vec<&str> = argv(1);
            assert_eq!(chmod, ["chmod", "u+x", &format!("/Applications/{FILE}")]);
            let mkdir: Vec<&str> = argv(2);
            assert_eq!(mkdir, ["mkdir", "-p", "/.local/share/icons/appimage"]);
            let unsquashfs: Vec<&str> = argv(3);
            assert_eq!(
                unsquashfs,
                [
                    "unsquashfs",
                    "-o",
                    "1640",
                    "-q",
                    "-n",
                    "-no-xattrs",
                    "-d",
                    "/.local/share/icons/appimage/krita",
                    &format!("/Applications/{FILE}"),
                    "usr/share/icons/hicolor/256x256/apps/krita.png",
                ],
                "the icon the entry names, followed to the file its link ends at"
            );
            let Action::WriteFile { path, contents } = &plan.steps[4].action else {
                panic!("{:?}", plan.steps[4]);
            };
            assert_eq!(
                path,
                Path::new(&format!("{HOME}/.local/share/applications/appimage-krita.desktop"))
            );
            let lines: Vec<&str> = contents.lines().collect();
            let exec = format!("Exec={HOME}/Applications/{FILE}");
            assert!(lines.contains(&format!("{exec} %F").as_str()), "{contents}");
            assert!(
                lines.contains(&format!("{exec} --new-window %F").as_str()),
                "{contents}"
            );
            assert!(
                lines.contains(
                    &format!(
                        "Icon={HOME}/.local/share/icons/appimage/krita/usr/share/icons/hicolor/256x256/apps/krita.png"
                    )
                    .as_str()
                ),
                "{contents}"
            );
            assert!(!contents.contains("TryExec"), "{contents}");
            assert!(lines.contains(&"Categories=Qt;KDE;Graphics;"), "{contents}");
        }

        #[test]
        fn without_unsquashfs_the_entry_is_plain_and_what_is_in_the_way_blocks_it() {
            let plan = plan(
                |fixture| {
                    fixture.executable(&format!("{HOME}/Applications/{FILE}"), appimage());
                    fixture.executable(&format!("{HOME}/Applications/Krita-5.1.0-x86_64.AppImage"), appimage());
                },
                |_| FakeRunner::new(),
            );
            assert_eq!(
                plan.blockers,
                [format!(
                    "{HOME}/Applications/{FILE} is there already; remove what is there first"
                )]
            );
            assert!(
                plan.cautions[0].contains("unsquashfs (from squashfs-tools) is not installed"),
                "{:?}",
                plan.cautions
            );
            assert!(
                plan.cautions[1..].iter().any(|caution| caution.starts_with(&format!(
                    "Krita 5.1.0 stays installed at {HOME}/Applications/Krita-5.1.0-x86_64.AppImage"
                ))),
                "other copies stay, and the file being installed is not one: {:?}",
                plan.cautions
            );
            assert_eq!(plan.replaces, None);
            let kinds: Vec<&str> = plan
                .steps
                .iter()
                .map(|step| match &step.action {
                    Action::Move { .. } => "move",
                    Action::Run { .. } => "run",
                    Action::WriteFile { .. } => "write",
                    _ => "other",
                })
                .collect();
            assert_eq!(kinds, ["move", "run", "write"]);
            let Action::WriteFile { contents, .. } = &plan.steps[2].action else {
                unreachable!()
            };
            assert!(contents.contains("\nName=Krita\n"), "{contents}");
            assert!(!contents.contains("Icon="), "{contents}");
        }
    }
}
