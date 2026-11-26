//! Which Linux distribution this is, from `os-release`.
//!
//! This module only reads what the distribution says about itself. Which package manager serves which distribution is
//! declared by the native sources ([`crate::provider::Native`]), so supporting a new distribution never touches this
//! file.

use housekeeper_core::Host;
use housekeeper_core::text::key_values;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distro {
    /// `ID`, e.g. `ubuntu`.
    pub id: String,
    /// `ID_LIKE`, closest relative first, e.g. `["ubuntu", "debian"]` for Linux Mint.
    pub id_like: Vec<String>,
    pub name: String,
    pub version: Option<String>,
    /// Booted from an OSTree image (Fedora Silverblue, Kinoite, Bazzite…).
    pub image_based: bool,
}

impl Distro {
    pub fn detect(host: &Host) -> Self {
        let text = ["/etc/os-release", "/usr/lib/os-release"]
            .into_iter()
            .find_map(|path| host.read_optional(path).ok().flatten())
            .unwrap_or_default();
        let mut distro = Self::parse(&text);
        distro.image_based = host.exists("/run/ostree-booted");
        distro
    }

    /// Parses `os-release`. `image_based` is left false; it comes from the running system, not the file.
    pub fn parse(os_release: &str) -> Self {
        let (mut id, mut id_like, mut name, mut pretty, mut version) = (None, None, None, None, None);
        for (key, value) in key_values(os_release) {
            match key {
                "ID" => id = Some(value),
                "ID_LIKE" => id_like = Some(value),
                "NAME" => name = Some(value),
                "PRETTY_NAME" => pretty = Some(value),
                "VERSION_ID" => version = Some(value),
                _ => {}
            }
        }
        Self {
            id: id.unwrap_or_else(|| "linux".to_string()),
            id_like: id_like
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            name: pretty.or(name).unwrap_or_else(|| "Linux".to_string()),
            version,
            image_based: false,
        }
    }

    /// `ID`, then each `ID_LIKE` entry: the order in which a native package manager is looked for.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str()).chain(self.id_like.iter().map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    #[test]
    fn ids_in_lookup_order() {
        let mint = Distro::parse("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n");
        assert_eq!(mint.ids().collect::<Vec<_>>(), ["linuxmint", "ubuntu", "debian"]);
        assert_eq!(Distro::parse("").ids().collect::<Vec<_>>(), ["linux"]);
    }

    #[test]
    fn names_and_versions() {
        let distro =
            Distro::parse("NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nID=ubuntu\n");
        assert_eq!(distro.name, "Ubuntu 24.04.1 LTS");
        assert_eq!(distro.version.as_deref(), Some("24.04"));
        assert_eq!(Distro::parse("NAME=Arch Linux\nID=arch\n").name, "Arch Linux");
        assert_eq!(Distro::parse("").name, "Linux");
    }

    #[test]
    fn detects_image_based_systems_and_falls_back_to_usr_lib() {
        let fixture = Fixture::new();
        fixture
            .file(
                "/usr/lib/os-release",
                "ID=fedora\nVARIANT_ID=silverblue\nPRETTY_NAME=\"Fedora Linux 41 (Silverblue)\"\n",
            )
            .file("/run/ostree-booted", "");
        let distro = Distro::detect(&fixture.host(FakeRunner::new()));
        assert_eq!(distro.id, "fedora");
        assert!(distro.image_based);
        assert_eq!(distro.name, "Fedora Linux 41 (Silverblue)");
    }
}
