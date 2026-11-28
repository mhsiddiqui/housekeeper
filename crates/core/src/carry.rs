//! Carrying out a reviewed change (decisions D37, D41, D44, D47): a removal (plan 4.5) or an install from a file (5.5).
//! The plan's steps run in order as one job of the operation queue, as the user or through the root helper; a removal
//! then moves the leftovers the user chose to the Trash; one history entry records how it went.

use crate::command::Cmd;
use crate::helper::{HELPER_PROTOCOL, HelperOperation, HelperReply, HelperRequest, STEP_TIMEOUT};
use crate::history::{Change, ChangeOutcome, HistoryEntry, StepOutcome, StepRecord, record, unix_now};
use crate::inspect::Inspection;
use crate::inventory::Housekeeper;
use crate::leftovers::Leftover;
use crate::queue::{Cancel, Event, Job, Outcome, Queue};
use crate::remove::{Action, Actor, RemovalPlan, Step};
use crate::show::{Details, Lookup};
use crate::trash::{TrashError, trash};

/// What came of a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// As written to the history.
    pub entry: HistoryEntry,
    /// Why the entry could not be written to the history, if it could not.
    pub history_error: Option<String>,
}

impl Housekeeper {
    /// Whether `reviewed` still describes the system (D37): the item is still installed in the same version, still not
    /// protected, and planning its removal again (simulation included) gives the same plan. `Err` says what changed.
    pub fn still_as_reviewed(&self, reviewed: &Details) -> Result<(), String> {
        let Lookup::Found(now) = self.show(&reviewed.package.key()) else {
            return Err("it is no longer installed".into());
        };
        if now.package.version != reviewed.package.version {
            return Err(format!(
                "its version changed from {} to {}",
                reviewed.package.version.as_deref().unwrap_or("unknown"),
                now.package.version.as_deref().unwrap_or("unknown")
            ));
        }
        if let Some(reason) = &now.package.protected {
            return Err(format!("it is protected now: {reason}"));
        }
        if now.removal != reviewed.removal {
            return Err("planning its removal again gives a different plan (what it would take changed)".into());
        }
        Ok(())
    }

    /// Whether `reviewed` still describes the file and the system (D37, D47): the file has the same contents (its
    /// checksum), still fits, and planning its install again (simulation included) gives the same plan. `Err` says
    /// what changed.
    pub fn still_as_inspected(&self, reviewed: &Inspection) -> Result<(), String> {
        let now = match self.inspect(&reviewed.path) {
            Ok(Some(now)) => now,
            Ok(None) => return Err("it is no longer a file Housekeeper knows".into()),
            Err(e) => return Err(e.to_string()),
        };
        if now.sha256 != reviewed.sha256 {
            return Err("the file changed (its SHA-256 checksum is not the same)".into());
        }
        if now.fit != reviewed.fit {
            return Err("whether it fits this system changed".into());
        }
        if now.install != reviewed.install {
            return Err("planning its install again gives a different plan (what it would take changed)".into());
        }
        Ok(())
    }

    /// Carries out `reviewed`'s removal plan, then moves `leftovers` (the folders the user chose) to the Trash, and
    /// records the outcome in the history. Steps run in order and stop at the first that fails; leftovers go only when
    /// every step was done. The change waits in the operation queue while its backend's lock is held; `cancel` stops it
    /// before it starts. A blocked or empty plan is not carried out.
    pub fn remove(
        &self,
        reviewed: &Details,
        leftovers: &[Leftover],
        cancel: &Cancel,
        events: &(dyn Fn(Event) + Sync),
    ) -> Recorded {
        let plan = reviewed.removal.clone().unwrap_or_else(RemovalPlan::unknown);
        let package = &reviewed.package;
        let refusal = if !plan.blockers.is_empty() {
            Some(format!("the plan is blocked: {}", plan.blockers.join("; ")))
        } else if plan.steps.is_empty() {
            Some("nothing records how it is removed".to_string())
        } else {
            None
        };
        let job = ChangeJob {
            housekeeper: self,
            backend: backend(&plan.steps).unwrap_or(package.source.as_str()).to_string(),
            steps: &plan.steps,
            sha256: None,
            leftovers,
        };
        let entry = HistoryEntry {
            at: 0,
            change: Change::Remove,
            item: package.key(),
            title: package.display_name().to_string(),
            file: None,
            steps: Vec::new(),
            outcome: ChangeOutcome::NotDone,
        };
        self.carry(entry, job, refusal, cancel, events)
    }

    /// Carries out `reviewed`'s install plan and records the outcome in the history (D47). Steps run in order and stop
    /// at the first that fails; a step for the root helper has it install its own copy of the file, and only if the
    /// copy has the checksum the inspection read. The change waits in the operation queue while its backend's lock is
    /// held; `cancel` stops it before it starts. A file that does not fit, or whose plan is missing, blocked or empty,
    /// is not installed.
    pub fn install(&self, reviewed: &Inspection, cancel: &Cancel, events: &(dyn Fn(Event) + Sync)) -> Recorded {
        let steps = reviewed.install.as_ref().map_or(&[][..], |plan| plan.steps.as_slice());
        let source = reviewed.source.as_ref().map_or("", |source| source.id.as_str());
        let job = ChangeJob {
            housekeeper: self,
            backend: backend(steps).unwrap_or(source).to_string(),
            steps,
            sha256: Some(&reviewed.sha256),
            leftovers: &[],
        };
        let entry = HistoryEntry {
            at: 0,
            change: Change::Install,
            item: format!("{source}:{}", reviewed.package_name()),
            title: reviewed.display_name(),
            file: Some(reviewed.path.clone()),
            steps: Vec::new(),
            outcome: ChangeOutcome::NotDone,
        };
        self.carry(entry, job, reviewed.refusal(), cancel, events)
    }

    /// Runs `job` through the operation queue, unless `refusal` says why not, and records how it went in `entry`,
    /// which is then added to the history.
    fn carry(
        &self,
        mut entry: HistoryEntry,
        job: ChangeJob<'_>,
        refusal: Option<String>,
        cancel: &Cancel,
        events: &(dyn Fn(Event) + Sync),
    ) -> Recorded {
        let (steps, leftovers) = (job.steps, job.leftovers);
        let records = match refusal {
            Some(reason) => skipped(steps, leftovers, &reason),
            None => match Queue::new(vec![job])
                .run(self.platform.as_ref(), &self.host, cancel, events)
                .pop()
            {
                Some(Outcome::Ran(records)) => records,
                Some(Outcome::LockTimeout(holder)) => {
                    let who = holder.program.unwrap_or_else(|| "another program".into());
                    let reason = format!("{who} held {} for longer than Housekeeper waits", holder.lock.display());
                    skipped(steps, leftovers, &reason)
                }
                Some(Outcome::Cancelled) | None => skipped(steps, leftovers, "cancelled before it started"),
            },
        };
        // Done when every step of the plan was; a leftover that changed since it was found and stayed does not undo it.
        entry.outcome = if records.iter().any(|r| matches!(r.outcome, StepOutcome::Failed { .. })) {
            ChangeOutcome::Failed
        } else if !steps.is_empty() && records[..steps.len()].iter().all(|r| r.outcome == StepOutcome::Done) {
            ChangeOutcome::Done
        } else {
            ChangeOutcome::NotDone
        };
        entry.steps = records;
        entry.at = unix_now();
        let history_error = record(&self.host, &entry).err().map(|e| e.to_string());
        Recorded { entry, history_error }
    }

    /// Carries out one step. A package file is installed only with `sha256`, its checksum when it was inspected.
    fn run_step(&self, step: &Step, sha256: Option<&str>) -> StepOutcome {
        match (&step.by, &step.action) {
            (Actor::User, Action::Run { argv }) => {
                let Some((program, args)) = argv.split_first() else {
                    return StepOutcome::Failed {
                        reason: "the step has no command".into(),
                    };
                };
                match self.host.run(&Cmd::new(program).args(args).timeout(STEP_TIMEOUT)) {
                    Ok(output) if output.success() => StepOutcome::Done,
                    Ok(output) => StepOutcome::Failed {
                        reason: failure(output.code, &output.stderr),
                    },
                    Err(e) => StepOutcome::Failed { reason: e.to_string() },
                }
            }
            (Actor::User, Action::Trash { path }) => match trash(&self.host, path, unix_now()) {
                Ok(_) => StepOutcome::Done,
                Err(e) => StepOutcome::Failed { reason: e.to_string() },
            },
            (Actor::Helper, Action::Trash { path }) => self.ask_helper(HelperOperation::Trash { path: path.clone() }),
            (_, Action::RemovePackages { tool, packages, .. }) => self.ask_helper(HelperOperation::RemovePackages {
                tool: tool.clone(),
                packages: packages.clone(),
            }),
            (_, Action::InstallPackageFile { tool, file, .. }) => match sha256 {
                Some(sha256) => self.ask_helper(HelperOperation::InstallPackageFile {
                    tool: tool.clone(),
                    file: file.clone(),
                    sha256: sha256.to_string(),
                }),
                None => StepOutcome::Failed {
                    reason: "a package file is installed only after it is inspected".into(),
                },
            },
            (Actor::Helper, Action::Run { .. }) => StepOutcome::Failed {
                reason: "the root helper does not run commands it is given".into(),
            },
        }
    }

    fn ask_helper(&self, operation: HelperOperation) -> StepOutcome {
        let Some(helper) = self.platform.root_helper() else {
            return StepOutcome::Failed {
                reason: "this system has no way to reach Housekeeper's root helper".into(),
            };
        };
        let request = HelperRequest {
            protocol: HELPER_PROTOCOL,
            operation,
        };
        match helper.ask(&self.host, &request) {
            Ok(HelperReply::Done { .. }) => StepOutcome::Done,
            Ok(HelperReply::Failed { code, stderr, .. }) => StepOutcome::Failed {
                reason: failure(code, &stderr),
            },
            Ok(HelperReply::Refused { reason }) => StepOutcome::Failed {
                reason: format!("the root helper refused: {reason}"),
            },
            Err(reason) => StepOutcome::Failed { reason },
        }
    }
}

/// One change for the queue: a removal's or an install's steps, then the leftovers a removal takes.
struct ChangeJob<'a> {
    housekeeper: &'a Housekeeper,
    backend: String,
    steps: &'a [Step],
    /// The checksum an install's file had when it was inspected.
    sha256: Option<&'a str>,
    leftovers: &'a [Leftover],
}

impl Job for ChangeJob<'_> {
    type Output = Vec<StepRecord>;

    fn backend(&self) -> &str {
        &self.backend
    }

    fn run(self) -> Vec<StepRecord> {
        let mut records = Vec::new();
        let mut failed = false;
        for step in self.steps {
            let outcome = if failed {
                StepOutcome::Skipped {
                    reason: "an earlier step failed".into(),
                }
            } else {
                self.housekeeper.run_step(step, self.sha256)
            };
            failed |= matches!(outcome, StepOutcome::Failed { .. });
            records.push(StepRecord {
                step: step.clone(),
                outcome,
            });
        }
        for leftover in self.leftovers {
            let outcome = if failed {
                StepOutcome::Skipped {
                    reason: "the removal did not finish, so what it leaves behind stays".into(),
                }
            } else {
                match self.housekeeper.trash_leftover(leftover) {
                    Ok(_) => StepOutcome::Done,
                    Err(e @ (TrashError::Changed { .. } | TrashError::Missing(_))) => {
                        StepOutcome::Skipped { reason: e.to_string() }
                    }
                    Err(e) => StepOutcome::Failed { reason: e.to_string() },
                }
            };
            records.push(StepRecord {
                step: Step::trash(Actor::User, &leftover.path),
                outcome,
            });
        }
        records
    }
}

/// The package tool the plan changes packages with, which is the backend whose lock it waits for.
fn backend(steps: &[Step]) -> Option<&str> {
    steps.iter().find_map(|step| match &step.action {
        Action::RemovePackages { tool, .. } | Action::InstallPackageFile { tool, .. } => Some(tool.as_str()),
        _ => None,
    })
}

/// Every step and leftover, not tried for `reason`.
fn skipped(steps: &[Step], leftovers: &[Leftover], reason: &str) -> Vec<StepRecord> {
    steps
        .iter()
        .cloned()
        .chain(
            leftovers
                .iter()
                .map(|leftover| Step::trash(Actor::User, &leftover.path)),
        )
        .map(|step| StepRecord {
            step,
            outcome: StepOutcome::Skipped { reason: reason.into() },
        })
        .collect()
}

/// A failed program in one line: the last thing it said on standard error, or its exit code.
fn failure(code: Option<i32>, stderr: &str) -> String {
    let said = stderr.lines().rev().map(str::trim).find(|line| !line.is_empty());
    match (said, code) {
        (Some(line), _) => line.chars().take(300).collect(),
        (None, Some(code)) => format!("it exited with code {code}"),
        (None, None) => "it was stopped".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::history::history;
    use crate::install::InstallPlan;
    use crate::remove::Tier;
    use crate::stub::{Stub, housekeeper_removing, package};
    use crate::testing::{FakeRunner, Fixture, HOME};

    fn home(path: &str) -> String {
        format!("{HOME}/{path}")
    }

    /// `stub:tool`, removed by uninstalling it as the user and then its system package through the helper.
    fn source() -> Stub {
        Stub {
            id: "stub",
            packages: vec![package("stub", "tool", None, &[])],
            plan: Some(RemovalPlan::new(
                Tier::PackageManager,
                vec![
                    Step::run(Actor::User, ["tool", "--uninstall"]),
                    Step::remove_packages("apt", vec!["tool".into()], vec!["apt-get".into(), "remove".into()]),
                ],
            )),
            ..Stub::default()
        }
    }

    fn done() -> HelperReply {
        HelperReply::Done {
            command: vec!["apt-get".into()],
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn reviewed(hk: &Housekeeper) -> Details {
        match hk.show("stub:tool") {
            Lookup::Found(details) => *details,
            other => panic!("{other:?}"),
        }
    }

    fn outcomes(recorded: &Recorded) -> Vec<&StepOutcome> {
        recorded.entry.steps.iter().map(|record| &record.outcome).collect()
    }

    #[test]
    fn a_reviewed_removal_runs_its_steps_then_trashes_the_chosen_leftovers_and_is_recorded() {
        let fixture = Fixture::new();
        fixture.file(&home(".cache/tool/blob"), "x");
        let runner = FakeRunner::new().respond("tool --uninstall", "");
        let (hk, helper) = housekeeper_removing(&fixture, vec![source()], runner, done());
        let details = reviewed(&hk);
        assert_eq!(hk.still_as_reviewed(&details), Ok(()));
        let chosen: Vec<Leftover> = details
            .leftovers
            .folders
            .iter()
            .filter(|f| f.preselected())
            .cloned()
            .collect();

        let removal = hk.remove(&details, &chosen, &Cancel::default(), &|_| {});
        assert_eq!(removal.entry.outcome, ChangeOutcome::Done);
        assert_eq!(outcomes(&removal), [&StepOutcome::Done; 3]);
        assert_eq!(
            helper.asked.lock().unwrap()[0].operation,
            HelperOperation::RemovePackages {
                tool: "apt".into(),
                packages: vec!["tool".into()],
            },
            "the helper is given the tool and the names, not the command"
        );
        let fixture_host = fixture.host(FakeRunner::new());
        assert!(fixture_host.exists(home(".local/share/Trash/files/tool/blob")));
        assert_eq!(history(&fixture_host).unwrap(), [removal.entry]);
    }

    #[test]
    fn a_failed_step_stops_the_rest_and_keeps_the_leftovers() {
        let fixture = Fixture::new();
        fixture.file(&home(".cache/tool/blob"), "x");
        let runner = FakeRunner::new().respond("tool --uninstall", "");
        let failed = HelperReply::Failed {
            command: vec!["apt-get".into()],
            code: Some(100),
            stdout: String::new(),
            stderr: "Reading package lists...\nE: Could not get lock /var/lib/dpkg/lock-frontend\n".into(),
        };
        let (hk, _) = housekeeper_removing(&fixture, vec![source()], runner, failed);
        let details = reviewed(&hk);
        let removal = hk.remove(&details, &details.leftovers.folders, &Cancel::default(), &|_| {});
        assert_eq!(removal.entry.outcome, ChangeOutcome::Failed);
        assert_eq!(
            outcomes(&removal)[1..],
            [
                &StepOutcome::Failed {
                    reason: "E: Could not get lock /var/lib/dpkg/lock-frontend".into()
                },
                &StepOutcome::Skipped {
                    reason: "the removal did not finish, so what it leaves behind stays".into()
                },
            ]
        );
        assert!(fixture.host(FakeRunner::new()).exists(home(".cache/tool/blob")));
    }

    #[test]
    fn a_blocked_plan_changes_nothing() {
        let fixture = Fixture::new();
        let mut blocked = source();
        blocked.plan = blocked.plan.map(|plan| plan.blocked("part of the system image"));
        // Any command or helper request would be a change: the runner panics on commands it was not given.
        let (hk, helper) = housekeeper_removing(&fixture, vec![blocked], FakeRunner::new(), done());
        let removal = hk.remove(&reviewed(&hk), &[], &Cancel::default(), &|_| {});
        assert_eq!(removal.entry.outcome, ChangeOutcome::NotDone);
        assert!(helper.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn a_new_version_since_the_review_is_a_change() {
        let fixture = Fixture::new();
        let (hk, _) = housekeeper_removing(&fixture, vec![source()], FakeRunner::new(), done());
        let mut details = reviewed(&hk);
        details.package.version = Some("1.0".into());
        let change = hk.still_as_reviewed(&details).unwrap_err();
        assert!(change.contains("version changed from 1.0"), "{change}");
    }

    /// `stub`, which installs `.pkg` files through the system's package tool, run by the root helper.
    fn installer() -> Stub {
        Stub {
            id: "stub",
            install: Some(InstallPlan::new(vec![Step::install_package_file(
                "apt",
                home("Downloads/app.pkg"),
                vec!["apt-get".into(), "install".into()],
            )])),
            ..Stub::default()
        }
    }

    fn inspected(hk: &Housekeeper) -> Inspection {
        hk.inspect(Path::new(&home("Downloads/app.pkg"))).unwrap().unwrap()
    }

    #[test]
    fn an_inspected_file_is_installed_by_the_helper_only_with_the_checksum_it_had_and_recorded() {
        let fixture = Fixture::new();
        fixture.file(&home("Downloads/app.pkg"), "PKG app 1.0");
        let (hk, helper) = housekeeper_removing(&fixture, vec![installer()], FakeRunner::new(), done());
        let inspection = inspected(&hk);
        assert_eq!(hk.still_as_inspected(&inspection), Ok(()));

        let installed = hk.install(&inspection, &Cancel::default(), &|_| {});
        assert_eq!(installed.entry.outcome, ChangeOutcome::Done);
        assert_eq!(
            helper.asked.lock().unwrap()[0].operation,
            HelperOperation::InstallPackageFile {
                tool: "apt".into(),
                file: home("Downloads/app.pkg").into(),
                sha256: "e9dd8097fe79de71a19aa8c28bce427c6173ec5f8d9b0d863471bd155fdd15e1".into(),
            },
            "the helper is given the tool, the file and its checksum when inspected, not the command"
        );
        assert_eq!(
            (installed.entry.item.as_str(), installed.entry.change),
            ("stub:app", Change::Install)
        );
        assert_eq!(history(&fixture.host(FakeRunner::new())).unwrap(), [installed.entry]);
    }

    #[test]
    fn a_file_changed_since_it_was_inspected_is_not_installed() {
        let fixture = Fixture::new();
        fixture.file(&home("Downloads/app.pkg"), "PKG app 1.0");
        let (hk, helper) = housekeeper_removing(&fixture, vec![installer()], FakeRunner::new(), done());
        let inspection = inspected(&hk);
        fixture.file(&home("Downloads/app.pkg"), "PKG app 1.0, and something else");
        let change = hk.still_as_inspected(&inspection).unwrap_err();
        assert!(change.contains("SHA-256 checksum"), "{change}");

        let mut blocked = inspection;
        if let Some(plan) = &mut blocked.install {
            plan.blockers.push("the package manager refuses: a conflict".into());
        }
        let installed = hk.install(&blocked, &Cancel::default(), &|_| {});
        assert_eq!(installed.entry.outcome, ChangeOutcome::NotDone);
        assert!(
            helper.asked.lock().unwrap().is_empty(),
            "a blocked plan asks nothing of the helper"
        );
    }
}
