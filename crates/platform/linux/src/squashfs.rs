//! What a squashfs image appended to a program holds, the way a type 2 AppImage carries its files, read with
//! `unsquashfs` (squashfs-tools) where the system has it. Nothing is extracted or run here.

use std::path::Path;

use housekeeper_core::{Cmd, Host, SourceError};

use crate::formats::path_arg;

/// How many symbolic links are followed inside an image before giving up.
const MAX_LINKS: usize = 8;

/// Where the image starts in a type 2 AppImage: right after its ELF part, whose section header table comes last
/// (`e_shoff + e_shentsize × e_shnum`). `None` when `head` (the file's first 64 bytes or more) is no ELF header.
pub(crate) fn elf_end(head: &[u8]) -> Option<u64> {
    if !head.starts_with(b"\x7fELF") {
        return None;
    }
    let little = match head.get(5)? {
        1 => true,
        2 => false,
        _ => return None,
    };
    let int = |at: usize, len: usize| -> Option<u64> {
        let bytes = head.get(at..at + len)?;
        let fold = |value: u64, byte: &u8| (value << 8) | u64::from(*byte);
        Some(if little {
            bytes.iter().rev().fold(0, fold)
        } else {
            bytes.iter().fold(0, fold)
        })
    };
    let (offset, entry_size, entries) = match head.get(4)? {
        1 => (int(0x20, 4)?, int(0x2e, 2)?, int(0x30, 2)?),
        2 => (int(0x28, 8)?, int(0x3a, 2)?, int(0x3c, 2)?),
        _ => return None,
    };
    offset.checked_add(entry_size.checked_mul(entries)?)
}

/// One entry of an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    /// Relative to the image's root, `usr/share/icons/app.png`.
    pub path: String,
    /// Where a symbolic link points, as written.
    pub link: Option<String>,
    /// A regular file (not a folder, a link or a device).
    pub file: bool,
}

/// Every entry of the image at `offset` in `file`, from `unsquashfs -lls`.
pub(crate) fn list(host: &Host, file: &Path, offset: u64) -> Result<Vec<Member>, SourceError> {
    let listing = host.run_ok(
        &Cmd::new("unsquashfs")
            .args(["-o", &offset.to_string(), "-lls"])
            .arg(path_arg(host, file)),
    )?;
    Ok(parse_listing(&listing))
}

/// `-rw-r--r-- root/root 29032 2026-11-12 15:41 squashfs-root/.DirIcon` and
/// `lrwxrwxrwx … squashfs-root/app.png -> usr/share/icons/app.png` lines; the root itself and other lines are skipped.
fn parse_listing(listing: &str) -> Vec<Member> {
    const ROOT: &str = " squashfs-root/";
    listing
        .lines()
        .filter_map(|line| {
            let at = line.find(ROOT)?;
            let rest = &line[at + ROOT.len()..];
            let (path, link) = match (line.starts_with('l'), rest.split_once(" -> ")) {
                (true, Some((path, target))) => (path, Some(target.to_string())),
                _ => (rest, None),
            };
            Some(Member {
                path: path.to_string(),
                link,
                file: line.starts_with('-'),
            })
        })
        .collect()
}

/// The text of `member` in the image at `offset` in `file` (`unsquashfs -cat`, which follows links).
pub(crate) fn cat(host: &Host, file: &Path, offset: u64, member: &str) -> Result<String, SourceError> {
    host.run_ok(
        &Cmd::new("unsquashfs")
            .args(["-o", &offset.to_string(), "-cat"])
            .arg(path_arg(host, file))
            .arg(member),
    )
}

/// The regular file `path` ends at inside the image, following its symbolic links (relative to the link's folder, or
/// to the image's root when absolute); `None` when it is missing, not a file, or leaves the image.
pub(crate) fn resolve(members: &[Member], path: &str) -> Option<String> {
    let mut path = path.to_string();
    for _ in 0..MAX_LINKS {
        let member = members.iter().find(|member| member.path == path)?;
        let Some(target) = &member.link else {
            return member.file.then_some(path);
        };
        let base = if target.starts_with('/') {
            ""
        } else {
            path.rsplit_once('/').map_or("", |(dir, _)| dir)
        };
        path = normalize(base, target)?;
    }
    None
}

/// `target` taken from the folder `base`, without `.` and `..`; `None` when it climbs above the root.
fn normalize(base: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first 64 bytes of an x86_64 AppImage whose section headers (64 × 30 bytes) start at 942712.
    fn header() -> Vec<u8> {
        let mut head = vec![0u8; 64];
        head[..4].copy_from_slice(b"\x7fELF");
        head[4] = 2;
        head[5] = 1;
        head[0x28..0x30].copy_from_slice(&942_712u64.to_le_bytes());
        head[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        head[0x3c..0x3e].copy_from_slice(&30u16.to_le_bytes());
        head
    }

    #[test]
    fn the_image_starts_where_the_elf_section_headers_end() {
        assert_eq!(elf_end(&header()), Some(942_712 + 64 * 30));
        let mut big_endian_32 = vec![0u8; 64];
        big_endian_32[..6].copy_from_slice(b"\x7fELF\x01\x02");
        big_endian_32[0x20..0x24].copy_from_slice(&1000u32.to_be_bytes());
        big_endian_32[0x2e..0x30].copy_from_slice(&40u16.to_be_bytes());
        big_endian_32[0x30..0x32].copy_from_slice(&3u16.to_be_bytes());
        assert_eq!(elf_end(&big_endian_32), Some(1120));
        assert_eq!(elf_end(&header()[..40]), None, "cut short");
        assert_eq!(elf_end(b"#!/bin/sh\n"), None);
    }

    const LISTING: &str = "\
drwxrwxr-x root/root                98 2026-11-12 15:41 squashfs-root
-rw-rw-r-- root/root             29032 2026-11-12 15:41 squashfs-root/.DirIcon
lrwxrwxrwx root/root                38 2026-11-12 15:41 squashfs-root/my app.desktop -> usr/share/applications/app.desktop
lrwxrwxrwx root/root                48 2026-11-12 15:41 squashfs-root/app.png -> usr/share/icons/hicolor/512x512/apps/app.png
lrwxrwxrwx root/root                 9 2026-11-12 15:41 squashfs-root/escape.png -> ../../etc
drwxrwxr-x root/root                63 2026-11-12 15:41 squashfs-root/usr/share/icons/hicolor/512x512/apps
lrwxrwxrwx root/root                 9 2026-11-12 15:41 squashfs-root/usr/share/icons/hicolor/512x512/apps/app.png -> ../../256x256/apps/app.png
-rw-rw-r-- root/root              6532 2026-11-12 15:41 squashfs-root/usr/share/icons/hicolor/256x256/apps/app.png
";

    #[test]
    fn links_inside_the_image_are_followed_to_the_file_they_end_at() {
        let members = parse_listing(LISTING);
        assert_eq!(members[1].link.as_deref(), Some("usr/share/applications/app.desktop"));
        assert_eq!(members[1].path, "my app.desktop");
        assert_eq!(
            resolve(&members, "app.png").as_deref(),
            Some("usr/share/icons/hicolor/256x256/apps/app.png")
        );
        assert_eq!(resolve(&members, ".DirIcon").as_deref(), Some(".DirIcon"));
        assert_eq!(resolve(&members, "escape.png"), None, "a link out of the image");
        assert_eq!(
            resolve(&members, "usr/share/icons/hicolor/512x512/apps"),
            None,
            "a folder"
        );
        assert_eq!(resolve(&members, "missing.png"), None);
    }
}
