//! File helpers shared by the sources that scan the file system for apps: AppImage detection, symlinks, scope and
//! version-looking words.

use std::path::{Path, PathBuf};

use housekeeper_core::{Host, Scope};

/// ELF identification bytes 8–10 hold `AI` and the AppImage type (1 or 2).
const MAGIC_LEN: usize = 11;

/// Whether a file is an AppImage (type 1 or 2), judged by its ELF header. Unreadable files are not.
pub(crate) fn is_appimage(host: &Host, path: &Path) -> bool {
    host.read_head(path, MAGIC_LEN)
        .is_ok_and(|head| is_appimage_head(&head))
}

/// Whether a file's first bytes are an AppImage's ELF header.
pub(crate) fn is_appimage_head(head: &[u8]) -> bool {
    head.starts_with(b"\x7fELF") && matches!(head.get(8..MAGIC_LEN), Some(b"AI\x01" | b"AI\x02"))
}

/// Follows a chain of symlinks to the file it ends at (logical path).
pub(crate) fn resolve_symlinks(host: &Host, path: &Path) -> PathBuf {
    let mut path = path.to_path_buf();
    for _ in 0..16 {
        match host.link_target(&path) {
            Some(target) => path = target,
            None => break,
        }
    }
    path
}

/// User scope for anything under the home directory, system otherwise.
pub(crate) fn scope_of(host: &Host, path: &Path) -> Scope {
    if path.starts_with(host.home()) {
        Scope::User
    } else {
        Scope::System
    }
}

/// Starts with a digit, or `v` and a digit: `1.6.7`, `v2`, `18`.
pub(crate) fn looks_like_version(word: &str) -> bool {
    let digits = word.strip_prefix(['v', 'V']).unwrap_or(word);
    digits.starts_with(|c: char| c.is_ascii_digit())
}
