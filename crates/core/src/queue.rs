//! The operation queue (plan 4.2, decision D41): changes to one backend run one after another, in the order they were
//! queued; changes to different backends run at the same time. Before a change starts, the queue waits while another
//! program holds its backend's lock (apt running in the background) and says who holds it (D6). Cancelling skips every
//! change that has not started, including one still waiting for a lock; a change that has started runs to its end,
//! because stopping a package manager halfway can leave its database broken.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::platform::Platform;

/// How long a change waits for another program to let go of its backend's lock before giving up.
pub const LOCK_WAIT: Duration = Duration::from_secs(30 * 60);

/// How often a held lock is looked at again.
pub const LOCK_POLL: Duration = Duration::from_secs(1);

/// One change for the queue.
pub trait Job: Send {
    type Output: Send;

    /// What the job changes (`apt`, `flatpak`, `npm`…). Jobs with the same backend never run at the same time.
    fn backend(&self) -> &str;

    fn run(self) -> Self::Output;
}

/// Who holds a backend's lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockHolder {
    /// The lock file.
    pub lock: PathBuf,
    /// The process holding it, when the lock tells.
    pub pid: Option<u32>,
    /// Its program name (`unattended-upgr`), when known.
    pub program: Option<String>,
}

/// Ability of a backend: tell whether another program holds its lock, without taking it (scans and checks never take
/// locks, D6).
pub trait PackageLock: Send + Sync {
    fn holder(&self, host: &Host) -> Option<LockHolder>;
}

/// Stops changes that have not started. Clones share one switch.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// What the queue is doing, for a front end to show. `job` is the job's index in the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The job waits because another program holds its backend's lock (sent again when the holder changes).
    Waiting {
        job: usize,
        holder: LockHolder,
    },
    Started {
        job: usize,
    },
    Finished {
        job: usize,
    },
    /// The job was cancelled before it started.
    Cancelled {
        job: usize,
    },
}

/// What came of one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    /// It ran; `T` says how that went.
    Ran(T),
    /// It was cancelled before it started.
    Cancelled,
    /// Another program held the lock for longer than the queue waits.
    LockTimeout(LockHolder),
}

/// Changes waiting to run.
pub struct Queue<J> {
    jobs: Vec<J>,
    lock_wait: Duration,
    lock_poll: Duration,
}

impl<J: Job> Queue<J> {
    pub fn new(jobs: Vec<J>) -> Self {
        Self {
            jobs,
            lock_wait: LOCK_WAIT,
            lock_poll: LOCK_POLL,
        }
    }

    /// Waits at most `wait` for a held lock, looking every `poll`.
    pub fn lock_timing(mut self, wait: Duration, poll: Duration) -> Self {
        self.lock_wait = wait;
        self.lock_poll = poll;
        self
    }

    /// Runs every job, one backend's jobs in order and different backends' at once, and returns their outcomes in
    /// queue order. `platform` tells which backends have a lock to wait for; `events` hears about every step and may be
    /// called from several threads.
    pub fn run(
        self,
        platform: &dyn Platform,
        host: &Host,
        cancel: &Cancel,
        events: &(dyn Fn(Event) + Sync),
    ) -> Vec<Outcome<J::Output>> {
        let count = self.jobs.len();
        let mut lanes: Vec<(String, Vec<(usize, J)>)> = Vec::new();
        let mut lane_of: HashMap<String, usize> = HashMap::new();
        for (index, job) in self.jobs.into_iter().enumerate() {
            let backend = job.backend().to_string();
            let lane = *lane_of.entry(backend.clone()).or_insert_with(|| {
                lanes.push((backend, Vec::new()));
                lanes.len() - 1
            });
            lanes[lane].1.push((index, job));
        }
        let finished: Vec<(usize, Outcome<J::Output>)> = thread::scope(|scope| {
            let workers: Vec<_> = lanes
                .into_iter()
                .map(|(backend, jobs)| {
                    let lane = Lane {
                        lock: platform.package_lock(&backend),
                        host,
                        cancel,
                        events,
                        wait: self.lock_wait,
                        poll: self.lock_poll,
                    };
                    scope.spawn(move || {
                        jobs.into_iter()
                            .map(|(index, job)| (index, lane.run(index, job)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
                .collect()
        });
        let mut outcomes: Vec<Option<Outcome<J::Output>>> = (0..count).map(|_| None).collect();
        for (index, outcome) in finished {
            outcomes[index] = Some(outcome);
        }
        outcomes.into_iter().flatten().collect()
    }
}

/// What one backend's worker needs to run its jobs.
struct Lane<'a> {
    lock: Option<&'a dyn PackageLock>,
    host: &'a Host,
    cancel: &'a Cancel,
    events: &'a (dyn Fn(Event) + Sync),
    wait: Duration,
    poll: Duration,
}

impl Lane<'_> {
    fn run<J: Job>(&self, index: usize, job: J) -> Outcome<J::Output> {
        let started_waiting = Instant::now();
        let mut told: Option<LockHolder> = None;
        loop {
            if self.cancel.is_cancelled() {
                (self.events)(Event::Cancelled { job: index });
                return Outcome::Cancelled;
            }
            let Some(holder) = self.lock.and_then(|lock| lock.holder(self.host)) else {
                break;
            };
            if started_waiting.elapsed() >= self.wait {
                return Outcome::LockTimeout(holder);
            }
            if told.as_ref() != Some(&holder) {
                (self.events)(Event::Waiting {
                    job: index,
                    holder: holder.clone(),
                });
                told = Some(holder);
            }
            thread::sleep(self.poll);
        }
        (self.events)(Event::Started { job: index });
        let output = job.run();
        (self.events)(Event::Finished { job: index });
        Outcome::Ran(output)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::model::SystemInfo;
    use crate::source::Source;
    use crate::testing::{FakeRunner, Fixture};

    /// Records when it ran, relative to the other jobs.
    struct Step {
        backend: &'static str,
        name: &'static str,
        log: Arc<Mutex<Vec<String>>>,
        pause: Duration,
    }

    impl Job for Step {
        type Output = &'static str;
        fn backend(&self) -> &str {
            self.backend
        }
        fn run(self) -> &'static str {
            self.log.lock().unwrap().push(format!("start {}", self.name));
            thread::sleep(self.pause);
            self.log.lock().unwrap().push(format!("end {}", self.name));
            self.name
        }
    }

    /// Held for the first `held_for` looks, then free.
    struct CountdownLock {
        held_for: AtomicUsize,
    }

    impl PackageLock for CountdownLock {
        fn holder(&self, _: &Host) -> Option<LockHolder> {
            let left = self.held_for.load(Ordering::SeqCst);
            (left > 0).then(|| {
                self.held_for.store(left - 1, Ordering::SeqCst);
                LockHolder {
                    lock: PathBuf::from("/var/lib/dpkg/lock-frontend"),
                    pid: Some(812),
                    program: Some("unattended-upgr".into()),
                }
            })
        }
    }

    struct System {
        system: SystemInfo,
        apt_lock: CountdownLock,
    }

    impl Platform for System {
        fn system(&self) -> &SystemInfo {
            &self.system
        }
        fn sources(&self) -> &[Arc<dyn Source>] {
            &[]
        }
        fn package_lock(&self, backend: &str) -> Option<&dyn PackageLock> {
            (backend == "apt").then_some(&self.apt_lock as &dyn PackageLock)
        }
    }

    fn system(apt_held_for: usize) -> System {
        System {
            system: SystemInfo {
                os: "linux".into(),
                name: "Test".into(),
                id: "test".into(),
                version: None,
                family: "test".into(),
                image_based: false,
                arch: "x86_64".into(),
            },
            apt_lock: CountdownLock {
                held_for: AtomicUsize::new(apt_held_for),
            },
        }
    }

    fn step(log: &Arc<Mutex<Vec<String>>>, backend: &'static str, name: &'static str, pause_ms: u64) -> Step {
        Step {
            backend,
            name,
            log: Arc::clone(log),
            pause: Duration::from_millis(pause_ms),
        }
    }

    fn run(
        platform: &System,
        jobs: Vec<Step>,
        cancel: &Cancel,
        on_event: impl Fn(&Event) + Sync,
    ) -> Vec<Outcome<&'static str>> {
        let fixture = Fixture::new();
        let host = fixture.host(FakeRunner::new());
        Queue::new(jobs)
            .lock_timing(Duration::from_millis(500), Duration::from_millis(5))
            .run(platform, &host, cancel, &|event| on_event(&event))
    }

    #[test]
    fn one_backends_jobs_run_in_order_and_never_overlap() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let jobs = vec![
            step(&log, "apt", "vim", 30),
            step(&log, "flatpak", "godot", 30),
            step(&log, "apt", "emacs", 0),
        ];
        let outcomes = run(&system(0), jobs, &Cancel::default(), |_| {});
        assert_eq!(
            outcomes,
            [Outcome::Ran("vim"), Outcome::Ran("godot"), Outcome::Ran("emacs")],
            "outcomes come back in queue order"
        );
        let log = log.lock().unwrap();
        let at = |entry: &str| log.iter().position(|e| e == entry).unwrap();
        assert!(
            at("end vim") < at("start emacs"),
            "apt's second job waited for its first: {log:?}"
        );
        assert!(
            at("start godot") < at("end vim"),
            "flatpak's job ran while apt's did: {log:?}"
        );
    }

    #[test]
    fn a_held_lock_is_waited_for_and_its_holder_named_once() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let events = Mutex::new(Vec::new());
        let outcomes = run(
            &system(3),
            vec![step(&log, "apt", "vim", 0)],
            &Cancel::default(),
            |event| events.lock().unwrap().push(event.clone()),
        );
        assert_eq!(outcomes, [Outcome::Ran("vim")]);
        let events = events.into_inner().unwrap();
        assert!(
            matches!(&events[..], [Event::Waiting { job: 0, holder }, Event::Started { job: 0 }, Event::Finished { job: 0 }]
                if holder.program.as_deref() == Some("unattended-upgr")),
            "{events:?}"
        );
    }

    #[test]
    fn a_lock_held_too_long_gives_up_without_running() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let outcomes = run(
            &system(usize::MAX),
            vec![step(&log, "apt", "vim", 0)],
            &Cancel::default(),
            |_| {},
        );
        assert!(matches!(&outcomes[..], [Outcome::LockTimeout(holder)] if holder.pid == Some(812)));
        assert!(log.lock().unwrap().is_empty(), "nothing ran");
    }

    #[test]
    fn cancelling_skips_what_has_not_started_but_lets_a_running_job_finish() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let cancel = Cancel::default();
        let jobs = vec![step(&log, "apt", "vim", 50), step(&log, "apt", "emacs", 0)];
        let outcomes = run(&system(0), jobs, &cancel, |event| {
            if *event == (Event::Started { job: 0 }) {
                cancel.cancel();
            }
        });
        assert_eq!(outcomes, [Outcome::Ran("vim"), Outcome::Cancelled]);
        assert_eq!(*log.lock().unwrap(), ["start vim", "end vim"]);
    }

    #[test]
    fn cancelling_stops_a_wait_for_a_lock() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let cancel = Cancel::default();
        let outcomes = run(
            &system(usize::MAX),
            vec![step(&log, "apt", "vim", 0)],
            &cancel,
            |event| {
                if matches!(event, Event::Waiting { .. }) {
                    cancel.cancel();
                }
            },
        );
        assert_eq!(outcomes, [Outcome::Cancelled]);
        assert!(log.lock().unwrap().is_empty());
    }
}
