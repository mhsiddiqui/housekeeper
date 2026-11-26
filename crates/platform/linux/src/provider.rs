//! How Linux sources and package tools are discovered and assembled.
//!
//! **To add a provider, create one file in `src/sources/` with a type that implements [`Source`] and
//! [`LinuxSource`], and call `register!(<Type>);` next to the `impl LinuxSource`.** The build script declares every
//! file in that folder as a module and lists its `PROVIDER` constant, which `register!` defines; no other file
//! changes. A file without `register!` fails to compile. A distribution's own package manager additionally declares
//! [`LinuxSource::NATIVE`], which is how a new distribution gets supported.
//!
//! **A package tool (apt, dnf…) is the same: one file in `src/tools/` with a type that implements [`PackageTool`]
//! and [`LinuxTool`], and `register_package_tool!(<Type>);`** (decision D24).
//!
//! Sources never import from each other; code two sources share lives in a module outside `src/sources/`.

use std::sync::Arc;

use housekeeper_core::{PackageRemoval, Remover, Source};

use crate::distro::Distro;

/// A group of distributions served by one native package manager, e.g. `debian` for Debian, Ubuntu, Mint….
pub struct Family {
    /// Reported as `SystemInfo::family`.
    pub name: &'static str,
    /// `ID` values from `os-release`. Distributions whose `ID_LIKE` names one of these match too.
    pub distro_ids: &'static [&'static str],
}

/// Marks a source as a distribution's own package manager (decision D2: exactly one per system).
pub struct Native {
    pub families: &'static [Family],
    /// On a distribution no family claims, the first native source whose program is installed is used.
    pub program: &'static str,
}

/// What a source can use while it is being created. Sources that need other sources reach them while being read,
/// through `housekeeper_core::ScanContext`, not here.
pub struct Context<'a> {
    pub distro: &'a Distro,
    /// The tool that changes the native package database on this system (D24). Only the native source uses it.
    pub tool: Option<&'a Arc<dyn PackageTool>>,
}

/// Implemented, next to [`Source`], by every Linux source.
pub trait LinuxSource: Source + Sized + 'static {
    /// `Some` for a distribution's own package manager; `None` (the default) for sources that work on every
    /// distribution, such as Flatpak or npm.
    const NATIVE: Option<Native> = None;

    fn create(context: &Context<'_>) -> Self;
}

/// One registered source type. Each file in `src/sources/` defines one with `register!`.
pub struct Provider {
    pub native: Option<Native>,
    create: fn(&Context<'_>) -> Arc<dyn Source>,
}

/// Registers a source type: `register!(MySource);` defines the file's `PROVIDER`, which the build script lists.
macro_rules! register {
    ($ty:ty) => {
        pub(crate) const PROVIDER: crate::provider::Provider = crate::provider::Provider::of::<$ty>();
    };
}
pub(crate) use register;

impl Provider {
    pub const fn of<T: LinuxSource>() -> Self {
        Self {
            native: T::NATIVE,
            create: create::<T>,
        }
    }

    pub fn create(&self, context: &Context<'_>) -> Arc<dyn Source> {
        (self.create)(context)
    }
}

fn create<T: LinuxSource>(context: &Context<'_>) -> Arc<dyn Source> {
    Arc::new(T::create(context))
}

/// The program a distribution changes its packages with: apt, dnf, zypper, rpm-ostree, pacman (decision D24). The
/// native source reads the package database; what changes it comes from the tool, offered like a source's optional
/// abilities.
pub trait PackageTool: Send + Sync {
    fn remover(&self) -> Option<&dyn Remover> {
        None
    }

    /// What the root helper removes packages with (D25).
    fn package_removal(&self) -> Option<&dyn PackageRemoval> {
        None
    }
}

/// Which systems a package tool serves.
pub struct Serves {
    /// Names of the families of the native source it changes (`debian`, `fedora`…; see [`Family`]).
    pub families: &'static [&'static str],
    /// Whether it is the tool of the image-based systems of those families (rpm-ostree) or of the others.
    pub image_based: bool,
}

/// Implemented, next to [`PackageTool`], by every package tool.
pub trait LinuxTool: PackageTool + Sized + 'static {
    const SERVES: Serves;

    fn create() -> Self;
}

/// One registered package tool. Each file in `src/tools/` defines one with `register_package_tool!`.
pub struct ToolProvider {
    pub serves: Serves,
    create: fn() -> Arc<dyn PackageTool>,
}

/// Registers a package tool type: `register_package_tool!(MyTool);` defines the file's `TOOL`, which the build script
/// lists. (`register_tool` is taken by a built-in attribute.)
macro_rules! register_package_tool {
    ($ty:ty) => {
        pub(crate) const TOOL: crate::provider::ToolProvider = crate::provider::ToolProvider::of::<$ty>();
    };
}
pub(crate) use register_package_tool;

impl ToolProvider {
    pub const fn of<T: LinuxTool>() -> Self {
        Self {
            serves: T::SERVES,
            create: create_tool::<T>,
        }
    }

    pub fn create(&self) -> Arc<dyn PackageTool> {
        (self.create)()
    }
}

fn create_tool<T: LinuxTool>() -> Arc<dyn PackageTool> {
    Arc::new(T::create())
}
