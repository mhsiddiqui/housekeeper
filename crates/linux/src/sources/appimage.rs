//! AppImages: single-file apps found in the places people usually keep them.

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use housekeeper_core::{
    Availability, FileType, Host, InstallReason, Package, PackageKind, ScanContext, Source, SourceError, SourceId,
    SourceInfo, SourceKind,
};

use crate::files::{is_appimage, looks_like_version, resolve_symlinks, scope_of};
use crate::provider::{Context, LinuxSource, register};

pub const ID: SourceId = SourceId::new("appimage");

/// Directories scanned for AppImages, relative to the home directory. Their files only, not subdirectories.
const HOME_DIRS: &[&str] = &["Applications", "AppImages", ".local/bin", "bin", "Downloads", "Desktop"];

/// Home directories that are on the PATH, so an AppImage in (or linked from) them is a command.
const PATH_DIRS: &[&str] = &[".local/bin", "bin"];

/// Architecture and OS words release names carry around the version.
const PLATFORM_WORDS: &[&str] = &[
    "x86_64", "x86-64", "x86", "amd64", "x64", "aarch64", "arm64", "armhf", "armv7l", "i386", "i686", "linux",
    "linux64", "glibc",
];

pub struct AppImageSource;

impl LinuxSource for AppImageSource {
    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(AppImageSource);

impl Source for AppImageSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("appimage", "AppImage", SourceKind::Portable)
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
}
