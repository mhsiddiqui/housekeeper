//! One file per package tool: the program a distribution changes its packages with (decision D24).
//!
//! Every file here is a tool and is registered automatically by `build.rs`, which declares it as a module and lists
//! its `TOOL`. Each file must call `register_package_tool!(<Type>);` for its `impl LinuxTool for <Type>` (see
//! [`crate::provider`]). Tools never import from each other; what they share is below.

use housekeeper_core::{RemovalPlan, Step, Tier};

include!(concat!(env!("OUT_DIR"), "/tools.rs"));

/// Removing `package` with `command` followed by its name: one step for the root helper (D25).
pub(crate) fn removal(tool: &str, command: &[&str], package: &str) -> RemovalPlan {
    let argv = command.iter().copied().chain([package]).map(str::to_string).collect();
    RemovalPlan::new(
        Tier::PackageManager,
        vec![Step::remove_packages(tool, vec![package.to_string()], argv)],
    )
}
