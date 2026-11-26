//! One file per package tool: the program a distribution changes its packages with (decision D24).
//!
//! Every file here is a tool and is registered automatically by `build.rs`, which declares it as a module and lists
//! its `TOOL`. Each file must call `register_package_tool!(<Type>);` for its `impl LinuxTool for <Type>` (see
//! [`crate::provider`]). Tools never import from each other; what they share is below.

use housekeeper_core::{Cmd, Host, Package, PackageRemoval, RemovalPlan, Simulation, SourceError, Step, Tier};

use crate::rpmdb::removal_closure;

include!(concat!(env!("OUT_DIR"), "/tools.rs"));

/// Removing `package` with `tool`'s command: one step for the root helper (D25), which builds the same command itself.
pub(crate) fn removal(tool: &dyn PackageRemoval, package: &str) -> RemovalPlan {
    let packages = vec![package.to_string()];
    let argv = tool.command(&packages);
    RemovalPlan::new(
        Tier::PackageManager,
        vec![Step::remove_packages(tool.tool(), packages, argv)],
    )
}

/// `command` followed by `packages`.
pub(crate) fn command_for(command: &[&str], packages: &[String]) -> Vec<String> {
    command
        .iter()
        .map(|arg| arg.to_string())
        .chain(packages.iter().cloned())
        .collect()
}

/// Whether the RPM database lists `package` (a name or a full NEVRA): `rpm -q` exits 0 only then.
pub(crate) fn rpm_installed(host: &Host, package: &str) -> Result<bool, SourceError> {
    Ok(host.run(&Cmd::new("rpm").args(["-q", "--quiet", package]))?.success())
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
