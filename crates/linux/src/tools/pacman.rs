//! pacman, which changes its own database on Arch Linux and its derivatives.

use housekeeper_core::{Cmd, Host, Package, RemovalPlan, Remover, Simulation, Simulator, SourceError};

use super::removal;
use crate::provider::{LinuxTool, PackageTool, Serves, register_package_tool};

pub struct Pacman;

impl LinuxTool for Pacman {
    const SERVES: Serves = Serves {
        families: &["arch"],
        image_based: false,
    };

    fn create() -> Self {
        Self
    }
}
register_package_tool!(Pacman);

impl PackageTool for Pacman {
    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }
}

impl Remover for Pacman {
    /// Packages from the AUR are removed the same way: pacman installed them, whichever helper built them.
    fn plan(&self, _: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(removal("pacman", &["pacman", "--remove", "--noconfirm"], &package.id))
    }

    fn simulator(&self) -> Option<&dyn Simulator> {
        Some(self)
    }
}

/// `pacman --remove --print`, which needs no root. pacman removes only what it is given and refuses when another
/// package needs it, naming each one.
impl Simulator for Pacman {
    fn simulate(&self, host: &Host, package: &Package) -> Result<Simulation, SourceError> {
        let cmd = Cmd::new("pacman").args(["--remove", "--print", "--print-format", "%n", package.id.as_str()]);
        let output = host.run(&cmd)?;
        let by = "`pacman --remove --print`".to_string();
        if output.code == Some(0) {
            let removes = output
                .stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect();
            return Ok(Simulation {
                by,
                removes,
                refused: Vec::new(),
            });
        }
        // `:: removing gzip breaks dependency 'gzip' required by base`, one line per package that needs it.
        let refused: Vec<String> = output
            .stdout
            .lines()
            .chain(output.stderr.lines())
            .filter_map(|line| line.strip_prefix(":: "))
            .map(str::to_string)
            .collect();
        if output.code == Some(1) && !refused.is_empty() {
            return Ok(Simulation {
                by,
                removes: Vec::new(),
                refused,
            });
        }
        Err(SourceError::CommandFailed {
            command: cmd.to_string(),
            code: output.code,
            stderr: output.stderr.trim().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};
    use housekeeper_core::{Output, Scope, SourceId};

    fn package(name: &str) -> Package {
        Package::new(SourceId::new("pacman"), name, name, Scope::System)
    }

    #[test]
    fn a_leaf_goes_alone_and_a_needed_package_is_refused_with_who_needs_it() {
        let runner = FakeRunner::new()
            .respond("pacman --remove --print --print-format %n htop", "htop\n")
            .respond_with(
                "pacman --remove --print --print-format %n gzip",
                Output {
                    code: Some(1),
                    stdout: ":: removing gzip breaks dependency 'gzip' required by base\n\
                             :: removing gzip breaks dependency 'gzip' required by kbd\n"
                        .into(),
                    stderr: "error: failed to prepare transaction (could not satisfy dependencies)\n".into(),
                },
            );
        let host = Fixture::new().host(runner);
        let htop = Pacman.simulate(&host, &package("htop")).unwrap();
        assert_eq!((htop.removes, htop.refused.len()), (vec!["htop".to_string()], 0));
        let gzip = Pacman.simulate(&host, &package("gzip")).unwrap();
        assert!(gzip.removes.is_empty());
        assert_eq!(
            gzip.refused,
            [
                "removing gzip breaks dependency 'gzip' required by base",
                "removing gzip breaks dependency 'gzip' required by kbd"
            ]
        );
    }
}
