//! Which Linux distribution this is, from `os-release`.

use housekeeper_core::Host;
use housekeeper_core::text::key_values;

/// Distribution family. Decides the native package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Debian,
    /// Fedora, RHEL and rebuilds.
    Fedora,
    Suse,
    Arch,
    Unknown,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debian => "debian",
            Self::Fedora => "fedora",
            Self::Suse => "suse",
            Self::Arch => "arch",
            Self::Unknown => "unknown",
        }
    }

    fn from_id(id: &str) -> Self {
        match id {
            "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" | "zorin" | "kali" | "raspbian" | "neon"
            | "devuan" | "mx" | "deepin" | "pureos" => Self::Debian,
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "nobara" | "ultramarine" | "amzn"
            | "bazzite" => Self::Fedora,
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "opensuse-microos" | "sles" | "suse" | "sled" => {
                Self::Suse
            }
            "arch" | "manjaro" | "endeavouros" | "garuda" | "cachyos" | "artix" | "arcolinux" | "steamos" => Self::Arch,
            _ if id.starts_with("opensuse") => Self::Suse,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distro {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub family: Family,
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
        let id = id.unwrap_or_else(|| "linux".to_string());
        let family = std::iter::once(id.as_str())
            .chain(id_like.as_deref().unwrap_or_default().split_whitespace())
            .map(Family::from_id)
            .find(|family| *family != Family::Unknown)
            .unwrap_or(Family::Unknown);
        Self {
            name: pretty.or(name).unwrap_or_else(|| "Linux".to_string()),
            id,
            version,
            family,
            image_based: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    #[test]
    fn families_from_id_and_id_like() {
        let cases = [
            ("ID=ubuntu\nID_LIKE=debian\n", Family::Debian),
            ("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n", Family::Debian),
            ("ID=fedora\n", Family::Fedora),
            ("ID=rocky\nID_LIKE=\"rhel centos fedora\"\n", Family::Fedora),
            ("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n", Family::Suse),
            ("ID=endeavouros\nID_LIKE=arch\n", Family::Arch),
            ("ID=somethingnew\nID_LIKE=arch\n", Family::Arch),
            ("ID=nixos\n", Family::Unknown),
            ("", Family::Unknown),
        ];
        for (text, family) in cases {
            assert_eq!(Distro::parse(text).family, family, "{text:?}");
        }
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
        assert_eq!(distro.family, Family::Fedora);
        assert!(distro.image_based);
        assert_eq!(distro.name, "Fedora Linux 41 (Silverblue)");
    }
}
