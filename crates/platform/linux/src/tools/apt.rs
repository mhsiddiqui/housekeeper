//! apt, which changes the Debian package database (Debian, Ubuntu, Mint…).

use housekeeper_core::{
    Cmd, Host, LockHolder, Package, PackageLock, PackageRemoval, RemovalPlan, Remover, Simulation, Simulator,
    SourceError,
};

use super::{command_for, removal};
use crate::lock::{self, LockFile};
use crate::provider::{LinuxTool, PackageTool, Serves, register_package_tool};

pub struct Apt;

impl LinuxTool for Apt {
    const SERVES: Serves = Serves {
        families: &["debian"],
        image_based: false,
    };

    fn create() -> Self {
        Self
    }
}
register_package_tool!(Apt);

impl PackageTool for Apt {
    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }

    fn package_removal(&self) -> Option<&dyn PackageRemoval> {
        Some(self)
    }

    fn lock(&self) -> Option<&dyn PackageLock> {
        Some(self)
    }
}

/// apt's front-end lock, then dpkg's own (which `dpkg` run by hand takes alone).
impl PackageLock for Apt {
    fn holder(&self, host: &Host) -> Option<LockHolder> {
        lock::holder(
            host,
            &[
                LockFile::Locked("/var/lib/dpkg/lock-frontend"),
                LockFile::Locked("/var/lib/dpkg/lock"),
            ],
        )
    }
}

impl PackageRemoval for Apt {
    fn tool(&self) -> &'static str {
        "apt"
    }

    /// dpkg lists the package, by its exact id, as installed. dpkg-query reads names as patterns, which the helper's
    /// name check rules out.
    fn is_installed(&self, host: &Host, package: &str) -> Result<bool, SourceError> {
        let cmd = Cmd::new("dpkg-query").args([
            "--show",
            "--showformat=${binary:Package}\\t${db:Status-Status}\\n",
            package,
        ]);
        // Exits 1 when dpkg has never heard of it.
        let listing = host.run(&cmd)?.stdout;
        Ok(listing
            .lines()
            .any(|line| line.split_once('\t') == Some((package, "installed"))))
    }

    fn command(&self, packages: &[String]) -> Vec<String> {
        command_for(&["apt-get", "remove", "--yes"], packages)
    }
}

impl Remover for Apt {
    /// By its dpkg id, which carries the architecture when several are installed (`libc6:i386`).
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal(self, &package.id))
    }

    fn simulator(&self) -> Option<&dyn Simulator> {
        Some(self)
    }
}

/// `apt-get --simulate remove`, which needs no root: apt's own answer, packages that need this one included.
impl Simulator for Apt {
    fn simulate(&self, host: &Host, package: &Package) -> Result<Simulation, SourceError> {
        let cmd = Cmd::new("apt-get").args(["--simulate", "remove", package.id.as_str()]);
        let output = host.run(&cmd)?;
        let by = "`apt-get --simulate remove`".to_string();
        // apt exits 100 when it will not do it, saying why in `E:` messages.
        if output.code == Some(100) {
            let refused = errors(&output.stderr);
            if !refused.is_empty() {
                return Ok(Simulation {
                    by,
                    removes: Vec::new(),
                    refused,
                });
            }
        }
        if output.code != Some(0) {
            return Err(SourceError::CommandFailed {
                command: cmd.to_string(),
                code: output.code,
                stderr: output.stderr.trim().to_string(),
            });
        }
        let names = removed(&output.stdout);
        Ok(Simulation {
            by,
            removes: ids(host, package, &names)?,
            refused: Vec::new(),
        })
    }
}

/// apt's `E:` messages, each with the indented lines that explain it (apt 3's solver says which package forced the
/// refusal) joined into one line.
fn errors(stderr: &str) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();
    for line in stderr.lines() {
        if let Some(message) = line.strip_prefix("E: ") {
            errors.push(message.trim().to_string());
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            if let Some(last) = errors.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    errors
}

/// The packages in `Remv <name>[:<arch>] [<version>]` lines, in apt's order.
fn removed(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("Remv "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect()
}

/// dpkg ids for the names apt printed, the item first. apt leaves out the architecture of the system's own packages,
/// while dpkg's id carries it for packages installed for several architectures (`libgcc-s1:amd64`); dpkg resolves
/// those.
fn ids(host: &Host, package: &Package, names: &[&str]) -> Result<Vec<String>, SourceError> {
    let bare: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| !name.contains(':') && *name != package.id)
        .collect();
    let listing = if bare.is_empty() {
        String::new()
    } else {
        // Exits 1 when a name is not installed (it was just listed, so only in a race); the rest still prints.
        let cmd = Cmd::new("dpkg-query")
            .args(["--show", "--showformat=${binary:Package}\\t${Architecture}\\n"])
            .args(bare.iter().copied());
        host.run(&cmd)?.stdout
    };
    let installed: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(id, _)| id)
        .collect();
    let native = if installed.iter().any(|id| id.contains(':')) {
        Some(
            host.run_ok(&Cmd::new("dpkg").arg("--print-architecture"))?
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    let mut ids = vec![package.id.clone()];
    for name in names {
        let id = if name.contains(':') {
            name.to_string()
        } else {
            let own_arch = native.as_deref().map(|arch| format!("{name}:{arch}"));
            installed
                .iter()
                .find(|id| *id == name)
                .or_else(|| installed.iter().find(|id| Some(**id) == own_arch.as_deref()))
                .map_or_else(|| name.to_string(), |id| id.to_string())
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};
    use housekeeper_core::{Output, Scope, SourceId};

    fn dpkg(id: &str, name: &str) -> Package {
        Package::new(SourceId::new("dpkg"), id, name, Scope::System)
    }

    #[test]
    fn simulation_lists_dpkg_ids_of_everything_apt_would_remove() {
        let stdout = "NOTE: This is only a simulation!\n\
                      The following packages will be REMOVED:\n  libgcc-s1 libstdc++6 libsemanage2 passwd\n\
                      Remv passwd [1:4.17.4-2]\nRemv libstdc++6:i386 [14.2.0-19]\nRemv libsemanage2 [3.8.1-1]\n\
                      Remv libgcc-s1 [14.2.0-19]\n";
        let runner = FakeRunner::new()
            .respond("apt-get --simulate remove libgcc-s1:amd64", stdout)
            .respond(
                "dpkg-query --show --showformat=${binary:Package}\\t${Architecture}\\n passwd libsemanage2 libgcc-s1",
                "passwd\tamd64\nlibsemanage2:amd64\tamd64\nlibsemanage2:i386\ti386\nlibgcc-s1:amd64\tamd64\n",
            )
            .respond("dpkg --print-architecture", "amd64\n");
        let host = Fixture::new().host(runner);
        let simulation = Apt.simulate(&host, &dpkg("libgcc-s1:amd64", "libgcc-s1")).unwrap();
        assert_eq!(
            simulation.removes,
            ["libgcc-s1:amd64", "passwd", "libstdc++6:i386", "libsemanage2:amd64"]
        );
        assert!(simulation.refused.is_empty());
    }

    #[test]
    fn a_refusal_is_reported_in_apts_words_with_its_explanation() {
        let runner = FakeRunner::new().respond_with(
            "apt-get --simulate remove python3",
            Output {
                code: Some(100),
                stdout: "Reading package lists...\n".into(),
                stderr: "E: Unable to satisfy dependencies. Reached two conflicting assignments:\n   \
                         1. shim-signed:amd64 is selected for install\n   \
                         2. shim-signed:amd64 Depends grub2-common (>= 2.04-1ubuntu24)\n\n\
                         E: Error, pkgProblemResolver::Resolve generated breaks.\n"
                    .into(),
            },
        );
        let host = Fixture::new().host(runner);
        let simulation = Apt.simulate(&host, &dpkg("python3", "python3")).unwrap();
        assert!(simulation.removes.is_empty());
        assert_eq!(
            simulation.refused,
            [
                "Unable to satisfy dependencies. Reached two conflicting assignments: \
                 1. shim-signed:amd64 is selected for install \
                 2. shim-signed:amd64 Depends grub2-common (>= 2.04-1ubuntu24)",
                "Error, pkgProblemResolver::Resolve generated breaks."
            ]
        );
    }
}
