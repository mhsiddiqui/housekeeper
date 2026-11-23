//! An operating system: what it is and which sources it has.

use crate::model::SystemInfo;
use crate::source::Source;

/// Implemented once per operating system (`LinuxPlatform`, later macOS and Windows).
pub trait Platform: Send + Sync {
    fn system(&self) -> &SystemInfo;

    /// Every source that applies to this system, in display order. Sources still probe for availability.
    fn sources(&self) -> &[Box<dyn Source>];
}
