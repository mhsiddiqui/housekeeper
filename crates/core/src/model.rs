//! What Housekeeper knows about installed software, independent of the operating system.

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Stable identifier of a source, e.g. `dpkg`, `flatpak`, `npm`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceId(Cow<'static, str>);

impl SourceId {
    pub const fn new(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Broad family a source belongs to. Front ends group sources by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// The distribution's own package manager (dpkg, rpm, pacman).
    System,
    /// Cross-distribution package systems with their own repository (Flatpak, Snap, Homebrew).
    Universal,
    /// Programming-language package managers (npm, pipx, cargo).
    Language,
    /// Self-contained files run in place (AppImage).
    Portable,
    /// Installed by hand or by a script.
    Manual,
}

/// Static description of a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub id: SourceId,
    /// Human-readable name, e.g. "Debian packages".
    pub label: Cow<'static, str>,
    pub kind: SourceKind,
}

impl SourceInfo {
    pub const fn new(id: &'static str, label: &'static str, kind: SourceKind) -> Self {
        Self {
            id: SourceId::new(id),
            label: Cow::Borrowed(label),
            kind,
        }
    }
}

/// What an installed item is, as far as its source and its launchers tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageKind {
    /// Something a person launches: a desktop app or a command-line program.
    App,
    /// Shared runtime other apps need (Flatpak runtimes, snap bases).
    Runtime,
    /// Library or development files.
    Library,
    /// Nothing a person launches: data, fonts, themes, firmware, documentation, plugins, metapackages. Told from
    /// packages whose files are known: none of them is a command or a launcher shown in menus.
    Component,
    /// The source cannot tell.
    Unknown,
}

/// Why the item is on the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallReason {
    /// Someone asked for it.
    Explicit,
    /// Pulled in because something else needs it.
    Dependency,
    /// Part of the operating system image.
    System,
    Unknown,
}

/// Who the installation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Installed for every user, usually needs admin rights to change.
    System,
    /// Installed in the current user's home.
    User,
}

/// One installed item as one source sees it.
///
/// The same app installed twice (say, from Flatpak and as a .deb) is two packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub source: SourceId,
    /// Unique within the source; the handle the source acts on
    /// (`libc6:i386`, `system/app/org.godotengine.Godot/x86_64/stable`, `v22.18.0/typescript`).
    pub id: String,
    /// What people call it (`libc6`, `org.godotengine.Godot`, `typescript`).
    pub name: String,
    /// Display name: the source's, or else the name of the package's main launcher (`Godot Engine`).
    pub title: Option<String>,
    pub version: Option<String>,
    pub summary: Option<String>,
    pub kind: PackageKind,
    pub reason: InstallReason,
    pub scope: Scope,
    pub size_bytes: Option<u64>,
    /// Seconds since the Unix epoch.
    pub installed_at: Option<i64>,
    /// Repository, remote, registry, publisher or runtime it came from.
    pub origin: Option<String>,
    /// Main location on disk, when the item lives in one place.
    pub location: Option<PathBuf>,
    /// Commands the item puts on the PATH.
    pub commands: Vec<String>,
    /// Menu entries that start it, sorted by id.
    pub launchers: Vec<Launcher>,
    /// Icon of its main launcher: a name in the icon theme (`firefox`) or an absolute path.
    pub icon: Option<String>,
    /// AppStream component IDs (`org.mozilla.firefox`): the app's name in software catalogs, the same whichever source
    /// installed it.
    pub app_ids: Vec<String>,
    /// Removing it could break the system.
    pub protected: bool,
}

/// An entry in the system's application menu (a desktop entry on Linux).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launcher {
    /// Unique among launchers: the desktop file ID on Linux (`firefox_firefox.desktop`).
    pub id: String,
    /// What the menu shows.
    pub name: String,
    /// A name in the icon theme (`firefox`) or an absolute path.
    pub icon: Option<String>,
    /// The file that defines it (logical path).
    pub path: PathBuf,
    /// Files with the same id that `path` takes precedence over (a user's copy hides the system's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadows: Vec<PathBuf>,
    /// The program it starts, when that could be found.
    pub program: Option<PathBuf>,
    /// Not shown in menus: handlers for links and file types, settings panels, entries a user removed.
    pub hidden: bool,
}

/// An app's catalog entry installed on disk (AppStream metainfo on Linux).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metainfo {
    /// The AppStream component ID (`org.gnome.Papers`).
    pub id: String,
    /// The file (logical path).
    pub path: PathBuf,
    /// Ids of the launchers that start the app (`org.gnome.Papers.desktop`).
    pub launchables: Vec<String>,
}

impl Package {
    pub fn new(source: SourceId, id: impl Into<String>, name: impl Into<String>, scope: Scope) -> Self {
        Self {
            source,
            id: id.into(),
            name: name.into(),
            title: None,
            version: None,
            summary: None,
            kind: PackageKind::Unknown,
            reason: InstallReason::Unknown,
            scope,
            size_bytes: None,
            installed_at: None,
            origin: None,
            location: None,
            commands: Vec::new(),
            launchers: Vec::new(),
            icon: None,
            app_ids: Vec::new(),
            protected: false,
        }
    }

    /// Unique across all sources: `source:id`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.source, self.id)
    }

    /// Title if known, otherwise the name.
    pub fn display_name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// Whether a source can be used on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Availability {
    Available { version: Option<String> },
    Unavailable { reason: String },
}

impl Availability {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable { reason: reason.into() }
    }
}

/// The machine being inspected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemInfo {
    /// `linux`, later `macos`, `windows`.
    pub os: String,
    /// Full name, e.g. "Ubuntu 24.04.1 LTS".
    pub name: String,
    /// Distribution or edition ID, e.g. `ubuntu`, `fedora`.
    pub id: String,
    pub version: Option<String>,
    /// Group of systems that share a native package manager, e.g. `debian`, `fedora`, `suse`, `arch` on Linux.
    pub family: String,
    /// The system is a read-only image that apps are layered on (Fedora Silverblue on Linux, for example).
    pub image_based: bool,
    pub arch: String,
}
