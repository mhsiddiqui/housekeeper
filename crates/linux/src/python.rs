//! Python packaging helpers shared by the sources that read `.dist-info` directories (PEP 376, PEP 503).

use std::path::{Path, PathBuf};

use housekeeper_core::{FileType, Host, SourceError};

/// `(pythonX.Y, <prefix>/lib/pythonX.Y/site-packages)` for each Python 3 that has one.
pub(crate) fn site_packages(host: &Host, prefix: &Path) -> Result<Vec<(String, PathBuf)>, SourceError> {
    Ok(host
        .read_dir(prefix.join("lib"))?
        .into_iter()
        .filter(|entry| entry.name.starts_with("python3.") && entry.file_type == Some(FileType::Dir))
        .map(|entry| (entry.name, entry.path.join("site-packages")))
        .filter(|(_, dir)| host.is_dir(dir))
        .collect())
}

/// `(name, version)` from a `<name>-<version>.dist-info` directory name. Versions never contain `-`.
pub(crate) fn split_dist_info(dir_name: &str) -> Option<(&str, &str)> {
    let (name, version) = dir_name.strip_suffix(".dist-info")?.rsplit_once('-')?;
    (!name.is_empty() && !version.is_empty()).then_some((name, version))
}

/// PEP 503 normalized form: lowercase, runs of `-`, `_` and `.` collapsed to `-`.
pub(crate) fn normalize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_helpers() {
        assert_eq!(
            split_dist_info("zope.interface-7.2.dist-info"),
            Some(("zope.interface", "7.2"))
        );
        assert_eq!(
            split_dist_info("graphifyy-0.9.100.dist-info"),
            Some(("graphifyy", "0.9.100"))
        );
        assert_eq!(split_dist_info("noversion.dist-info"), None);
        assert_eq!(split_dist_info("foo-1.0.egg-info"), None);
        assert_eq!(normalize_name("Zope.Interface"), "zope-interface");
        assert_eq!(normalize_name("ruamel__yaml"), "ruamel-yaml");
    }
}
