//! Carrying out a reviewed removal (plan 4.5, decisions D37, D41, D44): the plan's steps in order through the operation
//! queue, as the user or through the root helper, then the leftovers the user chose, then one history entry.

use crate::command::Cmd;
use crate::helper::{HELPER_PROTOCOL, HelperOperation, HelperReply, HelperRequest, REMOVAL_TIMEOUT};
use crate::history::{Change, ChangeOutcome, HistoryEntry, StepOutcome, StepRecord, record, unix_now};
use crate::inventory::Housekeeper;
use crate::leftovers::Leftover;
use crate::queue::{Cancel, Event, Job, Outcome, Queue};
use crate::remove::{Action, Actor, RemovalPlan, Step};
use crate::show::{Details, Lookup};
use crate::trash::{TrashError, trash};

/// What came of a removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
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
    ) -> Removal {
        let plan = reviewed.removal.clone().unwrap_or_else(RemovalPlan::unknown);
        let package = &reviewed.package;
        let refusal = if !plan.blockers.is_empty() {
            Some(format!("the plan is blocked: {}", plan.blockers.join("; ")))
        } else if plan.steps.is_empty() {
            Some("nothing records how it is removed".to_string())
        } else {
            None
        };
        let records = match refusal {
            Some(reason) => skipped(&plan.steps, leftovers, &reason),
            None => {
                let job = RemovalJob {
                    housekeeper: self,
                    backend: backend(&plan.steps).unwrap_or(package.source.as_str()).to_string(),
                    steps: &plan.steps,
                    leftovers,
                };
                match Queue::new(vec![job])
                    .run(self.platform.as_ref(), &self.host, cancel, events)
                    .pop()
                {
                    Some(Outcome::Ran(records)) => records,
                    Some(Outcome::LockTimeout(holder)) => {
                        let who = holder.program.unwrap_or_else(|| "another program".into());
                        let reason = format!("{who} held {} for longer than Housekeeper waits", holder.lock.display());
                        skipped(&plan.steps, leftovers, &reason)
                    }
                    Some(Outcome::Cancelled) | None => skipped(&plan.steps, leftovers, "cancelled before it started"),
                }
            }
        };
        // Done when every step of the plan was; a leftover that changed since it was found and stayed does not undo it.
        let outcome = if records.iter().any(|r| matches!(r.outcome, StepOutcome::Failed { .. })) {
            ChangeOutcome::Failed
        } else if records[..plan.steps.len()]
            .iter()
            .all(|r| r.outcome == StepOutcome::Done)
            && !plan.steps.is_empty()
        {
            ChangeOutcome::Done
        } else {
            ChangeOutcome::NotDone
        };
        let entry = HistoryEntry {
            at: unix_now(),
            change: Change::Remove,
            item: package.key(),
            title: package.display_name().to_string(),
            steps: records,
            outcome,
        };
        let history_error = record(&self.host, &entry).err().map(|e| e.to_string());
        Removal { entry, history_error }
    }

    fn run_step(&self, step: &Step) -> StepOutcome {
        match (&step.by, &step.action) {
            (Actor::User, Action::Run { argv }) => {
                let Some((program, args)) = argv.split_first() else {
                    return StepOutcome::Failed {
                        reason: "the step has no command".into(),
                    };
                };
                match self.host.run(&Cmd::new(program).args(args).timeout(REMOVAL_TIMEOUT)) {
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

/// One item's removal for the queue.
struct RemovalJob<'a> {
    housekeeper: &'a Housekeeper,
    backend: String,
    steps: &'a [Step],
    leftovers: &'a [Leftover],
}

impl Job for RemovalJob<'_> {
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
                self.housekeeper.run_step(step)
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
        Action::RemovePackages { tool, .. } => Some(tool.as_str()),
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
    use super::*;
    use crate::history::history;
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

    fn outcomes(removal: &Removal) -> Vec<&StepOutcome> {
        removal.entry.steps.iter().map(|record| &record.outcome).collect()
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
}
