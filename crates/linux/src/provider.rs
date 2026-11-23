//! How Linux sources are discovered and assembled.
//!
//! **To add a provider, create one file in `src/sources/` with a type that implements [`Source`] and
//! [`LinuxSource`].** The build script finds every `impl LinuxSource for <Type>` in that folder and registers it; no
//! other file changes. A distribution's own package manager additionally declares [`LinuxSource::NATIVE`], which is
//! how a new distribution gets supported.

use std::sync::Arc;

use housekeeper_core::Source;

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

/// What a source can use while it is being created.
pub struct Context<'a> {
    pub distro: &'a Distro,
    /// The native package manager chosen for this system, if any. Created before every other source.
    pub native: Option<Arc<dyn Source>>,
}

/// Implemented, next to [`Source`], by every Linux source.
pub trait LinuxSource: Source + Sized + 'static {
    /// `Some` for a distribution's own package manager; `None` (the default) for sources that work on every
    /// distribution, such as Flatpak or npm.
    const NATIVE: Option<Native> = None;

    fn create(context: &Context<'_>) -> Self;
}

/// One registered source type. The build script lists one per `LinuxSource` implementation.
pub struct Provider {
    pub native: Option<Native>,
    create: fn(&Context<'_>) -> Arc<dyn Source>,
}

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
