//! What Housekeeper changed, and what it has seen installed (plan 4.3, decision D39). Both live in the user's state
//! folder (`$XDG_STATE_HOME/housekeeper`), never leave the machine (D15), and are JSON Lines: one record per line, so a
//! line that cannot be read (a crash mid-write, an older or newer format) is skipped without losing the others.
//!
//! - `history.jsonl` grows by one line per change Housekeeper carried out or tried to.
//! - `seen.jsonl` holds every item a scan found, with when it was first and last seen. A scan rewrites it: what it found
//!   is refreshed, and items it no longer finds stay with their old `last_seen`, which is how leftovers of an app
//!   removed outside Housekeeper can still be named later (Phase 10).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::inventory::{Housekeeper, Scan, SourceStatus};
use crate::model::SourceId;
use crate::remove::Step;
use crate::source::SourceError;

/// Items not seen for this long are dropped from the record, so it does not grow without end.
pub const FORGET_AFTER_SECS: i64 = 365 * 24 * 60 * 60;

/// Now, in seconds since the Unix epoch, as the history and the record of items seen count time.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
}

/// One change, as it ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// When it ended, in seconds since the Unix epoch.
    pub at: i64,
    pub change: Change,
    /// The item's `source:id`; for an install, the source and the package's name (`dpkg:hello`).
    pub item: String,
    /// The item's display name when it was changed.
    pub title: String,
    /// The file it was installed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// Each step of the plan and how it went, in order.
    pub steps: Vec<StepRecord>,
    pub outcome: ChangeOutcome,
}

/// What kind of change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Change {
    Remove,
    Install,
}

/// How a whole change ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeOutcome {
    /// Every step was carried out.
    Done,
    /// A step failed; the steps after it were not tried.
    Failed,
    /// Nothing was carried out: a check refused it, the user cancelled, or a lock stayed held.
    NotDone,
}

/// One step and how it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    pub step: Step,
    pub outcome: StepOutcome,
}

/// How one step went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum StepOutcome {
    Done,
    Failed {
        reason: String,
    },
    /// Not tried, because of an earlier step or a check (`reason` says which).
    Skipped {
        reason: String,
    },
}

/// One item a scan found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenItem {
    /// `source:id`.
    pub key: String,
    pub source: SourceId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_ids: Vec<String>,
    /// Its launchers' IDs (`firefox.desktop`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launchers: Vec<String>,
    /// Seconds since the Unix epoch.
    pub first_seen: i64,
    pub last_seen: i64,
}

fn folder(host: &Host) -> PathBuf {
    host.state_home().join("housekeeper")
}

fn history_file(host: &Host) -> PathBuf {
    folder(host).join("history.jsonl")
}

fn seen_file(host: &Host) -> PathBuf {
    folder(host).join("seen.jsonl")
}

/// Adds `entry` to the end of the history.
pub fn record(host: &Host, entry: &HistoryEntry) -> Result<(), SourceError> {
    host.append(history_file(host), &line(entry))
}

/// Every change in the history, oldest first. A missing history is empty.
pub fn history(host: &Host) -> Result<Vec<HistoryEntry>, SourceError> {
    read_lines(host, &history_file(host))
}

/// Every item scans have seen, by key, including ones they no longer find.
pub fn seen(host: &Host) -> Result<Vec<SeenItem>, SourceError> {
    read_lines(host, &seen_file(host))
}

/// Updates the record of items seen with `scan`, taken at `now`. Only sources the scan read completely count: an item
/// of a source that failed or was not read is not taken for removed.
pub fn remember(host: &Host, scan: &Scan, now: i64) -> Result<(), SourceError> {
    let mut items: BTreeMap<String, SeenItem> = seen(host)?
        .into_iter()
        .filter(|item| now - item.last_seen <= FORGET_AFTER_SECS)
        .map(|item| (item.key.clone(), item))
        .collect();
    let read: Vec<&SourceId> = scan
        .sources
        .iter()
        .filter(|report| matches!(&report.status, SourceStatus::Ok { warnings, .. } if warnings.is_empty()))
        .map(|report| &report.info.id)
        .collect();
    for package in scan.packages.iter().filter(|package| read.contains(&&package.source)) {
        let key = package.key();
        let first_seen = items.get(&key).map_or(now, |item| item.first_seen);
        let item = SeenItem {
            key: key.clone(),
            source: package.source.clone(),
            name: package.name.clone(),
            title: package.title.clone(),
            app_ids: package.app_ids.clone(),
            launchers: package.launchers.iter().map(|launcher| launcher.id.clone()).collect(),
            first_seen,
            last_seen: now,
        };
        items.insert(key, item);
    }
    let text: Vec<u8> = items.values().flat_map(line).collect();
    host.write_atomic(seen_file(host), &text)
}

impl Housekeeper {
    /// Every change Housekeeper carried out or tried, oldest first.
    pub fn history(&self) -> Result<Vec<HistoryEntry>, SourceError> {
        history(&self.host)
    }
}

fn line(value: &impl Serialize) -> Vec<u8> {
    // Serializing these plain records cannot fail; an empty line would be skipped on reading anyway.
    let mut bytes = serde_json::to_vec(value).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn read_lines<T: DeserializeOwned>(host: &Host, path: &std::path::Path) -> Result<Vec<T>, SourceError> {
    Ok(host
        .read_optional(path)?
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::SourceReport;
    use crate::model::{Launcher, Package, Scope, SourceInfo, SourceKind, SystemInfo};
    use crate::remove::Actor;
    use crate::testing::{FakeRunner, Fixture, HOME};

    const DAY: i64 = 24 * 60 * 60;

    fn package(source: &'static str, name: &str) -> Package {
        Package::new(SourceId::new(source), name, name, Scope::System)
    }

    fn report(source: &'static str, status: SourceStatus) -> SourceReport {
        SourceReport {
            info: SourceInfo::new(source, source, SourceKind::System),
            status,
            elapsed_ms: 0,
        }
    }

    fn ok() -> SourceStatus {
        SourceStatus::Ok {
            version: None,
            packages: 0,
            warnings: Vec::new(),
        }
    }

    fn scan(sources: Vec<SourceReport>, packages: Vec<Package>) -> Scan {
        Scan {
            schema: 2,
            system: SystemInfo {
                os: "linux".into(),
                name: "Test".into(),
                id: "test".into(),
                version: None,
                family: "test".into(),
                image_based: false,
                arch: "x86_64".into(),
            },
            sources,
            packages,
            groups: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn last_seen(host: &Host) -> Vec<(String, i64, i64)> {
        seen(host)
            .unwrap()
            .into_iter()
            .map(|item| (item.key, item.first_seen, item.last_seen))
            .collect()
    }

    #[test]
    fn items_no_longer_found_are_kept_with_when_they_were_last_seen() {
        let fixture = Fixture::new();
        let host = fixture.host(FakeRunner::new());
        let mut firefox = package("flatpak", "org.mozilla.firefox");
        firefox.app_ids = vec!["org.mozilla.firefox".into()];
        firefox.launchers = vec![Launcher {
            id: "org.mozilla.firefox.desktop".into(),
            name: "Firefox".into(),
            icon: None,
            path: PathBuf::from("/var/lib/flatpak/exports/share/applications/org.mozilla.firefox.desktop"),
            shadows: Vec::new(),
            program: None,
            hidden: false,
        }];
        let both = vec![report("flatpak", ok()), report("dpkg", ok())];
        remember(&host, &scan(both.clone(), vec![firefox, package("dpkg", "vim")]), DAY).unwrap();
        // Firefox was uninstalled outside Housekeeper; vim is still there.
        remember(&host, &scan(both, vec![package("dpkg", "vim")]), 2 * DAY).unwrap();

        assert_eq!(
            last_seen(&host),
            [
                ("dpkg:vim".to_string(), DAY, 2 * DAY),
                ("flatpak:org.mozilla.firefox".to_string(), DAY, DAY),
            ]
        );
        let firefox = &seen(&host).unwrap()[1];
        assert_eq!(
            (firefox.app_ids.as_slice(), firefox.launchers.as_slice()),
            (
                ["org.mozilla.firefox".to_string()].as_slice(),
                ["org.mozilla.firefox.desktop".to_string()].as_slice()
            ),
            "enough to name its leftovers later"
        );
        assert!(
            host.exists(format!("{HOME}/.local/state/housekeeper/seen.jsonl")),
            "in the XDG state folder"
        );
    }

    #[test]
    fn a_source_that_failed_or_was_incomplete_does_not_make_its_items_look_removed() {
        let fixture = Fixture::new();
        let host = fixture.host(FakeRunner::new());
        remember(
            &host,
            &scan(vec![report("snap", ok())], vec![package("snap", "lxd")]),
            DAY,
        )
        .unwrap();
        let failed = report(
            "snap",
            SourceStatus::Failed {
                error: "snapd is down".into(),
            },
        );
        remember(&host, &scan(vec![failed], Vec::new()), 2 * DAY).unwrap();
        let partial = report(
            "snap",
            SourceStatus::Ok {
                version: None,
                packages: 0,
                warnings: vec!["could not read one snap".into()],
            },
        );
        remember(&host, &scan(vec![partial], Vec::new()), 3 * DAY).unwrap();
        assert_eq!(last_seen(&host), [("snap:lxd".to_string(), DAY, DAY)]);
    }

    #[test]
    fn items_unseen_for_a_year_are_forgotten() {
        let fixture = Fixture::new();
        let host = fixture.host(FakeRunner::new());
        let dpkg = vec![report("dpkg", ok())];
        remember(&host, &scan(dpkg.clone(), vec![package("dpkg", "old")]), DAY).unwrap();
        remember(&host, &scan(dpkg, Vec::new()), DAY + FORGET_AFTER_SECS + 1).unwrap();
        assert!(last_seen(&host).is_empty());
    }

    #[test]
    fn the_history_keeps_every_change_and_skips_lines_it_cannot_read() {
        let fixture = Fixture::new();
        let host = fixture.host(FakeRunner::new());
        assert!(history(&host).unwrap().is_empty(), "no history yet");
        let entry = |at, outcome| HistoryEntry {
            at,
            change: Change::Remove,
            item: "dpkg:vim".into(),
            title: "Vim".into(),
            file: None,
            steps: vec![StepRecord {
                step: Step::remove_packages("apt", vec!["vim".into()], vec!["apt-get".into(), "remove".into()]),
                outcome: StepOutcome::Done,
            }],
            outcome,
        };
        record(&host, &entry(1, ChangeOutcome::Done)).unwrap();
        // A crash halfway through a write, or a line from a newer Housekeeper.
        host.append(history_file(&host), b"{\"at\": 2, \"chan\n").unwrap();
        record(&host, &entry(3, ChangeOutcome::Failed)).unwrap();
        let read = history(&host).unwrap();
        assert_eq!(read, [entry(1, ChangeOutcome::Done), entry(3, ChangeOutcome::Failed)]);
        assert_eq!(read[0].steps[0].step.by, Actor::Helper);
    }
}
