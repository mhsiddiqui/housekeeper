//! One file per package tool: the program a distribution changes its packages with (decision D24).
//!
//! Every file here is a tool and is registered automatically by `build.rs`, which declares it as a module and lists
//! its `TOOL`. Each file must call `register_package_tool!(<Type>);` for its `impl LinuxTool for <Type>` (see
//! [`crate::provider`]). Tools never import from each other; what they share is below.

use housekeeper_core::{
    Cmd, Host, InstallFile, InstallPlan, InstallSimulation, Package, PackageRemoval, RemovalPlan, Simulation,
    SourceError, Step, Tier,
};

use crate::formats::path_arg;
use crate::lock::LockFile;
use crate::rpmdb::removal_closure;

/// The RPM database's own lock, which every transaction holds (dnf, zypper and rpm itself): in `/usr/lib/sysimage/rpm`
/// on current Fedora and openSUSE, `/var/lib/rpm` elsewhere (often a link to the same file).
pub(crate) const RPM_LOCKS: [LockFile; 2] = [
    LockFile::Locked("/usr/lib/sysimage/rpm/.rpm.lock"),
    LockFile::Locked("/var/lib/rpm/.rpm.lock"),
];

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

/// Installing `file` with `command` followed by its path: one step for the root helper (D46), named by `tool`'s name.
pub(crate) fn installation(
    tool: &dyn PackageRemoval,
    host: &Host,
    file: &InstallFile<'_>,
    command: &[&str],
) -> InstallPlan {
    let argv = command_for(command, &[path_arg(host, file.path)]);
    InstallPlan::new(vec![Step::install_package_files(
        tool.tool(),
        vec![file.path.to_path_buf()],
        argv,
    )])
}

/// What installing an `.rpm` would take, from the RPM database (`rpm -U --test`, which needs no root): the
/// capabilities nothing installed provides, which dnf, zypper or rpm-ostree would fetch from their repositories, and
/// rpm's other objections (conflicts, an equal or newer version installed). Signatures are left to the installing
/// tool, whose policy for local files differs from rpm's.
pub(crate) fn rpm_install_simulation(host: &Host, file: &InstallFile<'_>) -> Result<InstallSimulation, SourceError> {
    let cmd = Cmd::new("rpm")
        .args(["-U", "--test", "--nosignature", "--nodigest"])
        .arg(path_arg(host, file.path));
    let output = host.run(&cmd)?;
    let mut simulation = InstallSimulation {
        by: "the RPM database (`rpm -U --test`)".to_string(),
        installs: file
            .package
            .and_then(|package| Some(format!("{} {}", package.name.as_deref()?, package.version.as_deref()?)))
            .into_iter()
            .collect(),
        ..InstallSimulation::default()
    };
    // Each problem is a tab-indented line under rpm's `error:` heading. `X is needed by (installed) Y` means the new
    // version would break an installed package, which no repository fixes.
    for line in output.stderr.lines().filter_map(|line| line.strip_prefix('\t')) {
        match line.split_once(" is needed by ") {
            Some((capability, by)) if !by.starts_with("(installed)") => {
                simulation.needs.push(capability.trim().to_string())
            }
            _ => simulation.refused.push(line.trim().to_string()),
        }
    }
    if !output.success() && simulation.needs.is_empty() && simulation.refused.is_empty() {
        return Err(SourceError::CommandFailed {
            command: cmd.to_string(),
            code: output.code,
            stderr: output.stderr.trim().to_string(),
        });
    }
    Ok(simulation)
}
