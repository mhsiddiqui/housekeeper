//! Install plans: how a downloaded file that fits this system would be installed, the way its format intends
//! (decision D46).
//!
//! A plan only describes; nothing here installs anything. Carrying plans out is plan chunk 5.5.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::inspect::FilePackage;
use crate::model::Package;
use crate::remove::{Action, Actor, Step};
use crate::source::SourceError;

/// Ability: say how a file of one of the source's formats would be installed.
pub trait FileInstaller: Send + Sync {
    /// How `file` would be installed. Reads only.
    fn plan(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallPlan, SourceError>;

    /// "What else would come with it?", for installers whose package manager can tell without root.
    fn simulator(&self) -> Option<&dyn InstallSimulator> {
        None
    }
}

/// Ability: tell what installing a file would take, as its package manager sees it, without changing anything.
pub trait InstallSimulator: Send + Sync {
    fn simulate(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallSimulation, SourceError>;
}

/// The file to install, as inspected.
#[derive(Debug, Clone, Copy)]
pub struct InstallFile<'a> {
    /// Logical, absolute.
    pub path: &'a Path,
    /// `FormatInfo::id`, `deb`.
    pub format: &'a str,
    /// What is inside, when it could be read.
    pub package: Option<&'a FilePackage>,
}

/// How a file would be installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    /// What would be done, in order.
    pub steps: Vec<Step>,
    /// Why the plan must not be carried out (the package manager refuses, it would remove a protected package).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
    /// What to know before going ahead, without stopping it (a snap that is not signed by the store).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cautions: Vec<String>,
    /// The change takes effect only after the computer restarts (image-based systems).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub restart: bool,
    /// The version of the same package installed now, which this would replace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    /// What the package manager says the install takes; `None` when it was not asked or could not tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation: Option<InstallSimulation>,
}

/// What installing a file would take, as its package manager or database tells.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSimulation {
    /// Who answered, for people: "`apt-get --simulate install`".
    pub by: String,
    /// Packages that would be installed or upgraded (`name version`), the file's own among them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installs: Vec<String>,
    /// What the file requires that nothing installed provides, which the package manager would fetch from its
    /// repositories (the RPM database names capabilities, not the packages that provide them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    /// Packages the install would remove (they conflict with it), as the source's `Package::id`s.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removes: Vec<String>,
    /// Why the package manager would refuse, in its words; empty when it would go ahead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<String>,
}

impl InstallPlan {
    pub fn new(steps: Vec<Step>) -> Self {
        Self {
            steps,
            blockers: Vec::new(),
            cautions: Vec::new(),
            restart: false,
            replaces: None,
            simulation: None,
        }
    }

    pub fn caution(mut self, text: impl Into<String>) -> Self {
        self.cautions.push(text.into());
        self
    }

    pub fn needs_restart(mut self) -> Self {
        self.restart = true;
        self
    }
}

impl Step {
    /// Installs package `files` through the system's package tool `tool`, which `argv` runs. Always the root helper.
    pub fn install_package_files(tool: &str, files: Vec<PathBuf>, argv: Vec<String>) -> Self {
        Self {
            by: Actor::Helper,
            action: Action::InstallPackageFiles {
                tool: tool.to_string(),
                files,
                argv,
            },
        }
    }
}

/// `file`'s plan from `installer`, simulated when it can be. A refusal blocks it, and so does a protected package of
/// the source (`installed`) that it would remove. The installed copy of the same package, if any, is what it replaces;
/// the same version installed already is a caution. A simulation that fails leaves the plan without one and says why
/// in `warnings`.
pub(crate) fn plan(
    installer: &dyn FileInstaller,
    host: &Host,
    file: &InstallFile<'_>,
    installed: &[Package],
    warnings: &mut Vec<String>,
) -> Result<InstallPlan, SourceError> {
    let mut plan = installer.plan(host, file)?;
    plan.replaces = file
        .package
        .and_then(|package| package.name.as_deref())
        .and_then(|name| {
            installed
                .iter()
                .find(|p| p.name == name || p.app_ids.iter().any(|id| id == name))
        })
        .map(|p| p.version.clone().unwrap_or_else(|| "an unknown version".into()));
    let same = file.package.and_then(|package| package.version.as_ref());
    if same.is_some() && plan.replaces.as_ref() == same {
        plan.cautions
            .push("this version is installed already, so installing it changes nothing".into());
    }
    let Some(simulator) = installer.simulator() else {
        return Ok(plan);
    };
    match simulator.simulate(host, file) {
        Ok(simulation) => {
            for refusal in &simulation.refused {
                plan.blockers.push(format!("the package manager refuses: {refusal}"));
            }
            for other in installed.iter().filter(|p| simulation.removes.contains(&p.id)) {
                if let Some(why) = &other.protected {
                    plan.blockers.push(format!(
                        "it would remove {}, which is protected: {why}",
                        other.display_name()
                    ));
                }
            }
            plan.simulation = Some(simulation);
        }
        Err(error) => warnings.push(format!(
            "could not simulate installing {}: {error}",
            file.path.display()
        )),
    }
    Ok(plan)
}
