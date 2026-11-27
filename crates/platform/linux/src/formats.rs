//! Installer files (decision D45): telling a file's format from its name and first bytes, and the byte-level helpers
//! sources use to declare their formats ([`crate::provider::FileFormat`]).

use std::path::Path;

use housekeeper_core::{FilePackage, FormatInfo, Host, Recognised, SourceError, SourceId};

use crate::provider::{Family, FileFormat};

/// How many bytes of a file are read to tell its format: enough for a tar header's `ustar` mark (at byte 257) and a
/// `.flatpakref`'s group line anywhere in its first 256 bytes.
pub(crate) const HEAD_LEN: usize = 1024;

/// A format some source declares, and what installs it on this system.
pub(crate) struct Known {
    pub format: &'static FileFormat,
    pub owner: Owner,
}

#[derive(Clone)]
pub(crate) enum Owner {
    /// A source of this system lists what it installs.
    Source(SourceId),
    /// Another distribution's own package manager: the families it serves.
    Foreign(&'static [Family]),
}

/// The format among `known` that the file at `path` has: one of its name endings (the longest wins) and its first
/// bytes. What is inside is read by the format; a failure there is a warning, not an error.
pub(crate) fn recognise(known: &[Known], host: &Host, path: &Path) -> Result<Option<Recognised>, SourceError> {
    let head = host.read_head(path, HEAD_LEN)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut best: Option<(usize, &Known)> = None;
    for candidate in known {
        let Some(len) = suffix_len(candidate.format.suffixes, &name) else {
            continue;
        };
        if (candidate.format.matches)(&head) && best.is_none_or(|(best_len, _)| len > best_len) {
            best = Some((len, candidate));
        }
    }
    let Some((_, found)) = best else {
        return Ok(None);
    };
    let format = found.format;
    let mut warnings = Vec::new();
    let package = (format.read)(host, path, &head).unwrap_or_else(|e| {
        warnings.push(format!("could not read what is inside: {e}"));
        None
    });
    let (source, made_for) = match &found.owner {
        Owner::Source(id) => (Some(id.clone()), Vec::new()),
        Owner::Foreign(families) => (None, families.iter().map(|family| family.name.to_string()).collect()),
    };
    Ok(Some(Recognised {
        format: FormatInfo {
            id: format.id.into(),
            title: format.title.into(),
        },
        evidence: format.evidence.into(),
        source,
        made_for,
        package,
        warnings,
    }))
}

/// The length of the longest of `suffixes` that `name` ends with (and is longer than); 0 when there are none to match.
fn suffix_len(suffixes: &[&str], name: &str) -> Option<usize> {
    if suffixes.is_empty() {
        return Some(0);
    }
    suffixes
        .iter()
        .filter(|suffix| name.len() > suffix.len() && name.ends_with(*suffix))
        .map(|suffix| suffix.len())
        .max()
}

/// A processor as package formats name it (`amd64`, `aarch64`, `armv7h`), named like `SystemInfo::arch` (`x86_64`);
/// `None` for packages that run on any (`all`, `noarch`, `any`).
pub(crate) fn machine(arch: &str) -> Option<String> {
    let name = match arch {
        "" | "all" | "any" | "noarch" => return None,
        "amd64" | "x86_64" => "x86_64",
        "arm64" | "aarch64" => "aarch64",
        "i386" | "i486" | "i586" | "i686" | "x86" => "x86",
        "armhf" | "armel" | "armhfp" | "armv7h" | "armv7hl" | "arm" => "arm",
        "ppc64el" | "ppc64le" => "powerpc64",
        other => other,
    };
    Some(name.to_string())
}

/// The processor an ELF program is built for (its `e_machine`), named like `SystemInfo::arch`.
pub(crate) fn elf_machine(head: &[u8]) -> Option<String> {
    if !head.starts_with(b"\x7fELF") {
        return None;
    }
    let bytes = [*head.get(18)?, *head.get(19)?];
    let machine = match head.get(5)? {
        1 => u16::from_le_bytes(bytes),
        2 => u16::from_be_bytes(bytes),
        _ => return None,
    };
    let name = match machine {
        0x03 => "x86",
        0x28 => "arm",
        0x3e => "x86_64",
        0xb7 => "aarch64",
        0xf3 => "riscv64",
        _ => return None,
    };
    Some(name.into())
}

/// Compressed with gzip, xz, zstd or bzip2.
pub(crate) fn is_compressed(head: &[u8]) -> bool {
    [&b"\x1f\x8b"[..], b"\xfd7zXZ\x00", b"\x28\xb5\x2f\xfd", b"BZh"]
        .iter()
        .any(|magic| head.starts_with(magic))
}

/// A tar archive's first header (`ustar` at byte 257).
pub(crate) fn is_tar(head: &[u8]) -> bool {
    head.get(257..262) == Some(b"ustar")
}

/// A file's real path as a program argument, for the format's own tool.
pub(crate) fn path_arg(host: &Host, path: &Path) -> String {
    host.real_path(path).to_string_lossy().into_owned()
}

/// A package tool's `name<TAB>version<TAB>processor` line about a package file.
pub(crate) fn tab_package(output: &str) -> FilePackage {
    let mut fields = output.lines().next().unwrap_or_default().split('\t').map(str::trim);
    let mut next = || fields.next().filter(|field| !field.is_empty());
    let (name, version, arch) = (next(), next(), next());
    FilePackage {
        name: name.map(str::to_string),
        version: version.map(str::to_string),
        arch: arch.and_then(machine),
        ..FilePackage::default()
    }
}

/// Nothing to read: the format's file says nothing more than its format.
pub(crate) fn nothing_inside(_: &Host, _: &Path, _: &[u8]) -> Result<Option<FilePackage>, SourceError> {
    Ok(None)
}

/// `key: value` and `key = value` lines: the value of the first line whose key is `key`, trimmed, quotes removed.
pub(crate) fn field<'a>(text: &'a str, key: &str, separator: char) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(separator)?;
        (name.trim_end() == key && !name.starts_with(char::is_whitespace))
            .then(|| value.trim().trim_matches(|c| c == '\'' || c == '"'))
            .filter(|value| !value.is_empty())
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;

    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};
    use housekeeper_core::{Housekeeper, Inspection};

    use crate::LinuxPlatform;

    /// The file `name` in the Downloads folder, holding `content`, inspected on the system `os_release` describes.
    /// `runner` is given the file's real path, which the format's own tool is run with.
    pub(crate) fn inspect(
        os_release: &str,
        name: &str,
        content: &[u8],
        runner: impl FnOnce(&str) -> FakeRunner,
    ) -> Option<Inspection> {
        let fixture = Fixture::new();
        fixture.file("/etc/os-release", os_release);
        let path = format!("{HOME}/Downloads/{name}");
        fixture.file(&path, content);
        let real = fixture.host(FakeRunner::new()).real_path(&path);
        let host = fixture.host(runner(&real.to_string_lossy()));
        let platform = LinuxPlatform::detect(&host);
        Housekeeper::new(Box::new(platform), host)
            .inspect(Path::new(&path))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use housekeeper_core::Fit;
    use housekeeper_core::testing::FakeRunner;

    use super::test_support::inspect;

    const UBUNTU: &str = "ID=ubuntu\nID_LIKE=debian\n";
    const FEDORA: &str = "ID=fedora\n";

    fn format_of(name: &str, content: &[u8]) -> Option<String> {
        inspect(UBUNTU, name, content, |_| FakeRunner::new()).map(|inspection| inspection.format.id)
    }

    #[test]
    fn another_distributions_package_is_named_and_does_not_fit() {
        let deb = inspect(FEDORA, "app_1.0_amd64.deb", b"!<arch>\ndebian-binary   ", |_| {
            FakeRunner::new()
        })
        .unwrap();
        assert_eq!(deb.format.id, "deb");
        assert_eq!(deb.source, None);
        assert_eq!(
            deb.fit,
            Fit::OtherSystem {
                families: vec!["debian".into()]
            }
        );
    }

    #[test]
    fn first_bytes_decide_and_the_longest_name_ending_breaks_ties() {
        let zstd = b"\x28\xb5\x2f\xfd rest";
        assert_eq!(
            format_of("which-2.21-1-x86_64.pkg.tar.zst", zstd).as_deref(),
            Some("pacman")
        );
        assert_eq!(format_of("notes.tar.zst", zstd).as_deref(), Some("archive"));
        // A name that lies: a script called `.deb` is a script, and a renamed AppImage is still one.
        assert_eq!(
            format_of("setup.deb", b"#!/bin/sh\necho hi\n").as_deref(),
            Some("script")
        );
        let appimage = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00\x02\x00\x3e\x00";
        assert_eq!(format_of("krita", appimage).as_deref(), Some("appimage"));
        // The right name without the format's bytes is nothing known.
        assert_eq!(format_of("fake.AppImage", b"not an elf"), None);
        assert_eq!(format_of("fake.deb", b"PK\x03\x04"), None);
        assert_eq!(format_of("Cargo.toml", b"[workspace]\n"), None);
    }

    #[test]
    fn a_program_for_another_processor_does_not_fit() {
        let arm = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00\x02\x00\xb7\x00";
        let x86 = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00\x02\x00\x3e\x00";
        let (this, other) = if std::env::consts::ARCH == "aarch64" {
            (arm, x86)
        } else {
            (x86, arm)
        };
        let fits = inspect(UBUNTU, "Krita-5.2.6.AppImage", this, |_| FakeRunner::new()).unwrap();
        assert_eq!(fits.fit, Fit::Fits);
        let package = fits.package.unwrap();
        assert_eq!(
            (package.name.as_deref(), package.version.as_deref()),
            (Some("Krita"), Some("5.2.6"))
        );
        let other = inspect(UBUNTU, "Krita-5.2.6.AppImage", other, |_| FakeRunner::new()).unwrap();
        assert!(matches!(other.fit, Fit::OtherArch { .. }), "{:?}", other.fit);
    }
}
