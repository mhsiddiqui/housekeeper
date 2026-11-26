//! One file per package tool: the program a distribution changes its packages with (decision D24).
//!
//! Every file here is a tool and is registered automatically by `build.rs`, which declares it as a module and lists
//! its `TOOL`. Each file must call `register_package_tool!(<Type>);` for its `impl LinuxTool for <Type>` (see
//! [`crate::provider`]). Tools never import from each other; what they share is below.

use housekeeper_core::{Host, Package, RemovalPlan, Simulation, SourceError, Step, Tier};

use crate::rpmdb::removal_closure;

include!(concat!(env!("OUT_DIR"), "/tools.rs"));

/// Removing `package` with `command` followed by its name: one step for the root helper (D25).
pub(crate) fn removal(tool: &str, command: &[&str], package: &str) -> RemovalPlan {
    let argv = command.iter().copied().chain([package]).map(str::to_string).collect();
    RemovalPlan::new(
        Tier::PackageManager,
        vec![Step::remove_packages(tool, vec![package.to_string()], argv)],
    )
}

/// What dnf and zypper would remove with `package`: it and every installed package that needs it, directly or through
/// others, from the RPM database. Their own simulations need root on most releases (D33).
pub(crate) fn rpm_simulation(host: &Host, package: &Package) -> Result<Simulation, SourceError> {
    Ok(Simulation {
        by: "the RPM database (what needs it, and what needs those)".to_string(),
        removes: removal_closure(host, &package.id)?,
        refused: Vec::new(),
    })
}
