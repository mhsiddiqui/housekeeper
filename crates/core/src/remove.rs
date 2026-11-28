//! Removal plans: how one installed item would be removed, the way its installer intended (decisions D8, D25).
//!
//! A plan only describes; nothing here changes the system (D4). Carrying plans out comes with the root helper in
//! Phase 4.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::model::Package;
use crate::source::{Source, SourceError};

/// Ability: say how one of the source's packages is removed.
pub trait Remover: Send + Sync {
    /// How `package` (one of this source's) would be removed. Reads only.
    fn plan(&self, host: &Host, package: &Package) -> Result<RemovalPlan, SourceError>;

    /// "What else would go?", for removers whose package manager or database can tell without root (D33).
    fn simulator(&self) -> Option<&dyn Simulator> {
        None
    }
}

/// Ability: tell what removing a package would take, as its package manager sees it, without changing anything.
pub trait Simulator: Send + Sync {
    fn simulate(&self, host: &Host, package: &Package) -> Result<Simulation, SourceError>;
}

/// How `package` (one of `source`'s) would be removed: the source's plan, or `Unknown` when it has no remover. A
/// protected package's plan is blocked whatever the source planned (D5). Any other is simulated when the remover can,
/// and blocked when the package manager refuses or would take a protected package of the scan (`installed`) with it.
/// A simulation that fails leaves the plan without one and says why in `warnings`.
pub(crate) fn plan(
    source: &dyn Source,
    host: &Host,
    package: &Package,
    installed: &[Package],
    warnings: &mut Vec<String>,
) -> Result<RemovalPlan, SourceError> {
    let Some(remover) = source.remover() else {
        return Ok(protect(RemovalPlan::unknown(), package));
    };
    let mut plan = remover.plan(host, package)?;
    if package.protected.is_some() {
        return Ok(protect(plan, package));
    }
    let Some(simulator) = remover.simulator() else {
        return Ok(plan);
    };
    match simulator.simulate(host, package) {
        Ok(simulation) => {
            for refusal in &simulation.refused {
                plan.blockers.push(format!("the package manager refuses: {refusal}"));
            }
            for other in installed
                .iter()
                .filter(|p| p.source == package.source && p.id != package.id && simulation.removes.contains(&p.id))
            {
                if let Some(why) = &other.protected {
                    plan.blockers.push(format!(
                        "it would also remove {}, which is protected: {why}",
                        other.display_name()
                    ));
                }
            }
            plan.simulation = Some(simulation);
        }
        Err(error) => warnings.push(format!("could not simulate the removal of {}: {error}", package.name)),
    }
    Ok(plan)
}

fn protect(plan: RemovalPlan, package: &Package) -> RemovalPlan {
    match &package.protected {
        Some(why) => plan.blocked(format!("protected: {why}")),
        None => plan,
    }
}

/// How an installed item would be removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalPlan {
    pub tier: Tier,
    /// What would be done, in order. Empty when nothing records how to remove it.
    pub steps: Vec<Step>,
    /// Why the plan must not be carried out (installed for every user by a tool that would need root, part of the
    /// system image, protected). A blocked plan is never carried out; its steps, if any, show what it would take.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
    /// The change takes effect only after the computer restarts (image-based systems).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub restart: bool,
    /// What the package manager says the removal takes; `None` when it was not asked or could not tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation: Option<Simulation>,
}

/// What removing a package would take, as its package manager or database tells (D33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Simulation {
    /// Who answered, for people: "`apt-get --simulate remove`", "the RPM database".
    pub by: String,
    /// The `Package::id`s of the source's packages that would be removed, the item itself first.
    pub removes: Vec<String>,
    /// Why the package manager would refuse, in its words; empty when it would go ahead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<String>,
}

impl RemovalPlan {
    pub fn new(tier: Tier, steps: Vec<Step>) -> Self {
        Self {
            tier,
            steps,
            blockers: Vec::new(),
            restart: false,
            simulation: None,
        }
    }

    /// Nothing records how the item was installed, so there is nothing to follow.
    pub fn unknown() -> Self {
        Self::new(Tier::Unknown, Vec::new())
    }

    pub fn blocked(mut self, reason: impl Into<String>) -> Self {
        self.blockers.push(reason.into());
        self
    }

    pub fn needs_restart(mut self) -> Self {
        self.restart = true;
        self
    }
}

/// Where a plan comes from, most trusted first (D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// 1: the tool's own uninstaller (`rustup self uninstall`).
    OwnUninstaller,
    /// 2: the package manager that installed it (apt, Flatpak, npm…).
    PackageManager,
    /// 3: the convention of its format (an AppImage is its file and the launchers made for it).
    FormatConvention,
    /// 4: a reviewed recipe for a script-installed tool (Phase 3.4).
    Recipe,
    /// 5: the manifest recorded when it was installed in the tracked terminal (Phase 11).
    Manifest,
    /// 6: nothing records how it was installed.
    Unknown,
}

/// One thing a removal does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Who carries it out (D25).
    pub by: Actor,
    pub action: Action,
}

impl Step {
    /// Runs `argv` (a program and its arguments, never a shell).
    pub fn run<I, S>(by: Actor, argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            by,
            action: Action::Run {
                argv: argv.into_iter().map(Into::into).collect(),
            },
        }
    }

    /// Moves `path` (logical, absolute) to the Trash.
    pub fn trash(by: Actor, path: impl Into<PathBuf>) -> Self {
        Self {
            by,
            action: Action::Trash { path: path.into() },
        }
    }

    /// Removes `packages` through the system's package tool `tool`, which `argv` runs. Always the root helper.
    pub fn remove_packages(tool: &str, packages: Vec<String>, argv: Vec<String>) -> Self {
        Self {
            by: Actor::Helper,
            action: Action::RemovePackages {
                tool: tool.to_string(),
                packages,
                argv,
            },
        }
    }
}

/// Who carries out a step (D25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Actor {
    /// Housekeeper, as the user. Tools that authorise system changes themselves (snapd, Flatpak) ask on their own.
    User,
    /// Housekeeper's root helper, for what needs root and has no such tool.
    Helper,
}

/// What a step does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    /// Runs a program: `argv[0]` with the rest as its arguments.
    Run { argv: Vec<String> },
    /// Moves a file or folder to the Trash (D7).
    Trash { path: PathBuf },
    /// Removes packages through the system's package tool (D24). The root helper is given only `tool` and
    /// `packages`, checks each name against the package database and builds the command itself; `argv` is that
    /// command, for review.
    RemovePackages {
        tool: String,
        packages: Vec<String>,
        argv: Vec<String>,
    },
    /// Installs a package file through the system's package tool (D24, D46). The root helper is given `tool`, `file`
    /// and the file's checksum when it was inspected, installs its own copy and builds the command itself (D47);
    /// `argv` is that command with the file's path, for review.
    InstallPackageFile {
        tool: String,
        file: PathBuf,
        argv: Vec<String>,
    },
}
