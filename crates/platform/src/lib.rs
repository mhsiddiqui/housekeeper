//! The one place that knows which operating-system crate Housekeeper uses.
//!
//! Front ends call [`detect`] and talk only to `housekeeper-core` from then on. Supporting another operating system
//! means one more `detect` here, behind its `cfg`, and that crate as a target-specific dependency.

use housekeeper_core::{Host, Platform};

/// Housekeeper was built for an operating system it has no platform for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Housekeeper supports Linux only for now")]
pub struct Unsupported;

/// Describes the running system and assembles its sources.
#[cfg(target_os = "linux")]
pub fn detect(host: &Host) -> Result<Box<dyn Platform>, Unsupported> {
    Ok(Box::new(housekeeper_linux::LinuxPlatform::detect(host)))
}

/// Describes the running system and assembles its sources.
#[cfg(not(target_os = "linux"))]
pub fn detect(_host: &Host) -> Result<Box<dyn Platform>, Unsupported> {
    Err(Unsupported)
}
