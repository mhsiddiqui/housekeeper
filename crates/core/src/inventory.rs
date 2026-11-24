//! Scanning every source of a platform.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::identity;
use crate::model::{Availability, Package, SourceInfo, SystemInfo};
use crate::platform::Platform;
use crate::source::{ScanContext, Source};

/// Version of the JSON document produced from a `Scan`. Bumped on incompatible changes.
pub const SCAN_SCHEMA: u32 = 1;

/// Outcome of one source in a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SourceStatus {
    Ok {
        version: Option<String>,
        packages: usize,
        /// Problems that did not stop the source; its list may be incomplete.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    Unavailable {
        reason: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReport {
    #[serde(flatten)]
    pub info: SourceInfo,
    pub status: SourceStatus,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scan {
    pub schema: u32,
    pub system: SystemInfo,
    pub sources: Vec<SourceReport>,
    /// Sorted by display name, then source.
    pub packages: Vec<Package>,
    /// Problems outside any one source (an unreadable launcher folder); the scan is still complete otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Which sources to scan.
pub enum Selection<'a> {
    All,
    Only(&'a [String]),
}

impl Selection<'_> {
    fn includes(&self, info: &SourceInfo) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.iter().any(|id| id == info.id.as_str()),
        }
    }
}

/// Composition root: a platform and the host it inspects. Services hang off this.
pub struct Housekeeper {
    pub(crate) platform: Box<dyn Platform>,
    pub(crate) host: Host,
}

impl Housekeeper {
    pub fn new(platform: Box<dyn Platform>, host: Host) -> Self {
        Self { platform, host }
    }

    pub fn system(&self) -> &SystemInfo {
        self.platform.system()
    }

    pub fn sources(&self) -> impl Iterator<Item = SourceInfo> + '_ {
        self.platform.sources().iter().map(|source| source.info())
    }

    /// Only checks which sources are available; reads no package databases.
    pub fn probe(&self) -> Vec<(SourceInfo, Availability)> {
        self.platform
            .sources()
            .iter()
            .map(|source| (source.info(), source.probe(&self.host)))
            .collect()
    }

    /// Reads every selected source in parallel, then attaches each launcher of the application menu and each command
    /// a package database lists to its package, and derives kinds, titles and icons from them. A source that fails
    /// or panics is reported, not fatal.
    pub fn scan(&self, selection: &Selection<'_>) -> Scan {
        self.scan_with(selection, |_, _| {})
    }

    /// Like [`scan`](Self::scan), but hands each source's report and packages to `on_report` as soon as that source
    /// finishes, so a caller can show progress per source. The packages are as the source reported them; launchers,
    /// commands found in package databases, and the kinds, titles and icons that follow from them are only in the
    /// returned `Scan`, which is the same as `scan` returns.
    ///
    /// `on_report` runs once per selected source, on that source's scanning thread, so calls come concurrently from
    /// several threads in the order the sources finish. It sees every outcome: available, unavailable, failed, and a
    /// panicking source (reported as failed). If `on_report` itself panics, that source is treated like a panicking
    /// source: the scan reports it as failed with no packages, and the other sources are unaffected.
    pub fn scan_with(&self, selection: &Selection<'_>, on_report: impl Fn(&SourceReport, &[Package]) + Sync) -> Scan {
        self.read(selection, true, on_report)
    }

    /// The scan, with launchers and commands attached when `identify` is set. Reading them, and asking the package
    /// databases about their files, happens while the sources are being read.
    pub(crate) fn read(
        &self,
        selection: &Selection<'_>,
        identify: bool,
        on_report: impl Fn(&SourceReport, &[Package]) + Sync,
    ) -> Scan {
        let selected: Vec<&dyn Source> = self
            .platform
            .sources()
            .iter()
            .map(|source| source.as_ref())
            .filter(|source| selection.includes(&source.info()))
            .collect();

        let on_report = &on_report;
        let (results, gathered) = thread::scope(|scope| {
            let all = self.platform.sources();
            let selected = &selected;
            let gathering =
                identify.then(|| scope.spawn(move || identity::gather(&self.host, self.platform.as_ref(), selected)));
            let handles: Vec<_> = selected
                .iter()
                .map(|source| {
                    scope.spawn(move || {
                        let (report, packages) = scan_one(*source, &self.host, all);
                        on_report(&report, &packages);
                        (report, packages)
                    })
                })
                .collect();
            let results: Vec<(SourceReport, Vec<Package>)> = handles
                .into_iter()
                .zip(selected)
                .map(|(handle, source)| {
                    handle.join().unwrap_or_else(|_| {
                        let report = SourceReport {
                            info: source.info(),
                            status: SourceStatus::Failed {
                                error: "internal error while reading this source".into(),
                            },
                            elapsed_ms: 0,
                        };
                        (report, Vec::new())
                    })
                })
                .collect();
            (results, gathering.map(|handle| handle.join()))
        });

        let mut sources = Vec::with_capacity(results.len());
        let mut packages = Vec::new();
        for (report, found) in results {
            sources.push(report);
            packages.extend(found);
        }
        let mut warnings = Vec::new();
        match gathered {
            None => {}
            Some(Ok(gathered)) => gathered.apply(&mut packages, &mut sources, &mut warnings),
            Some(Err(_)) => warnings.push("internal error while reading launchers and commands".into()),
        }
        packages.sort_by(|a, b| {
            a.display_name()
                .to_lowercase()
                .cmp(&b.display_name().to_lowercase())
                .then_with(|| a.source.cmp(&b.source))
                .then_with(|| a.id.cmp(&b.id))
        });
        Scan {
            schema: SCAN_SCHEMA,
            system: self.platform.system().clone(),
            sources,
            packages,
            warnings,
        }
    }
}

/// `all` is every source of the platform, which the source may consult through its [`ScanContext`].
fn scan_one(source: &dyn Source, host: &Host, all: &[Arc<dyn Source>]) -> (SourceReport, Vec<Package>) {
    let started = Instant::now();
    let info = source.info();
    let (status, packages) = match catch_unwind(AssertUnwindSafe(|| source.probe(host))) {
        Err(_) => (failed("internal error while probing this source"), Vec::new()),
        Ok(Availability::Unavailable { reason }) => (SourceStatus::Unavailable { reason }, Vec::new()),
        Ok(Availability::Available { version }) => {
            let context = ScanContext::new(all);
            match catch_unwind(AssertUnwindSafe(|| source.inventory(host, &context))) {
                Err(_) => (failed("internal error while reading this source"), Vec::new()),
                Ok(Err(error)) => (failed(error.to_string()), Vec::new()),
                Ok(Ok(packages)) => (
                    SourceStatus::Ok {
                        version,
                        packages: packages.len(),
                        warnings: context.into_warnings(),
                    },
                    packages,
                ),
            }
        }
    };
    let report = SourceReport {
        info,
        status,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    (report, packages)
}

fn failed(error: impl Into<String>) -> SourceStatus {
    SourceStatus::Failed { error: error.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PackageKind, Scope, SourceId, SourceKind};
    use crate::source::SourceError;
    use crate::testing::{FakeRunner, Fixture};
    use std::sync::{Arc, Mutex};

    struct Stub {
        info: SourceInfo,
        behaviour: fn() -> Result<Vec<Package>, SourceError>,
        available: bool,
    }

    impl Source for Stub {
        fn info(&self) -> SourceInfo {
            self.info.clone()
        }
        fn probe(&self, _: &Host) -> Availability {
            if self.available {
                Availability::Available { version: None }
            } else {
                Availability::unavailable("not installed")
            }
        }
        fn inventory(&self, _: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
            (self.behaviour)()
        }
    }

    struct TestPlatform {
        system: SystemInfo,
        sources: Vec<Arc<dyn Source>>,
    }

    impl Platform for TestPlatform {
        fn system(&self) -> &SystemInfo {
            &self.system
        }
        fn sources(&self) -> &[Arc<dyn Source>] {
            &self.sources
        }
    }

    fn stub(
        id: &'static str,
        available: bool,
        behaviour: fn() -> Result<Vec<Package>, SourceError>,
    ) -> Arc<dyn Source> {
        Arc::new(Stub {
            info: SourceInfo::new(id, id, SourceKind::System),
            behaviour,
            available,
        })
    }

    fn app(source: &'static str, name: &str) -> Package {
        let mut package = Package::new(SourceId::new(source), name, name, Scope::System);
        package.kind = PackageKind::App;
        package
    }

    fn housekeeper(sources: Vec<Arc<dyn Source>>) -> Housekeeper {
        let system = SystemInfo {
            os: "linux".into(),
            name: "Test".into(),
            id: "test".into(),
            version: None,
            family: "test".into(),
            image_based: false,
            arch: "x86_64".into(),
        };
        let host = Fixture::new().host(FakeRunner::new());
        Housekeeper::new(Box::new(TestPlatform { system, sources }), host)
    }

    #[test]
    fn one_failing_or_panicking_source_does_not_hide_the_others() {
        let hk = housekeeper(vec![
            stub("good", true, || Ok(vec![app("good", "zed"), app("good", "Alacritty")])),
            stub("broken", true, || Err(SourceError::parse("db", "garbage"))),
            stub("panics", true, || panic!("bug")),
            stub("absent", false, || unreachable!("unavailable sources are not read")),
        ]);
        let scan = hk.scan(&Selection::All);

        let names: Vec<_> = scan.packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Alacritty", "zed"], "sorted case-insensitively");
        let status = |id: &str| &scan.sources.iter().find(|r| r.info.id.as_str() == id).unwrap().status;
        assert_eq!(
            status("good"),
            &SourceStatus::Ok {
                version: None,
                packages: 2,
                warnings: Vec::new(),
            }
        );
        assert!(matches!(status("broken"), SourceStatus::Failed { error } if error.contains("garbage")));
        assert!(matches!(status("panics"), SourceStatus::Failed { .. }));
        assert!(matches!(status("absent"), SourceStatus::Unavailable { .. }));
    }

    #[test]
    fn selection_limits_which_sources_are_read() {
        let hk = housekeeper(vec![
            stub("a", true, || Ok(vec![app("a", "one")])),
            stub("b", true, || Ok(vec![app("b", "two")])),
        ]);
        let only = ["b".to_string()];
        let scan = hk.scan(&Selection::Only(&only));
        assert_eq!(scan.sources.len(), 1);
        assert_eq!(scan.packages.len(), 1);
        assert_eq!(scan.packages[0].name, "two");
    }

    #[test]
    fn progress_reports_each_selected_source_once_with_its_own_packages() {
        let hk = housekeeper(vec![
            stub("good", true, || Ok(vec![app("good", "zed"), app("good", "Alacritty")])),
            stub("other", true, || Ok(vec![app("other", "vim")])),
            stub("broken", true, || Err(SourceError::parse("db", "garbage"))),
            stub("panics", true, || panic!("bug")),
            stub("absent", false, || unreachable!("unavailable sources are not read")),
            stub("skipped", true, || Ok(vec![app("skipped", "nano")])),
        ]);
        let only = ["good", "other", "broken", "panics", "absent"].map(String::from);
        let seen = Mutex::new(Vec::new());
        let scan = hk.scan_with(&Selection::Only(&only), |report, packages| {
            seen.lock().unwrap().push((report.clone(), packages.to_vec()));
        });

        let mut seen = seen.into_inner().unwrap();
        seen.sort_by(|a, b| a.0.info.id.cmp(&b.0.info.id));
        let ids: Vec<_> = seen.iter().map(|(report, _)| report.info.id.as_str()).collect();
        assert_eq!(ids, ["absent", "broken", "good", "other", "panics"]);
        for (report, packages) in &seen {
            assert!(scan.sources.contains(report), "report of {}", report.info.id);
            let in_scan: Vec<_> = scan.packages.iter().filter(|p| p.source == report.info.id).collect();
            let mut streamed: Vec<_> = packages.iter().collect();
            streamed.sort_by_key(|p| p.display_name().to_lowercase());
            assert_eq!(streamed, in_scan, "packages of {}", report.info.id);
        }
        assert!(matches!(seen[0].0.status, SourceStatus::Unavailable { .. }));
        assert!(matches!(seen[1].0.status, SourceStatus::Failed { .. }));
        assert!(matches!(seen[4].0.status, SourceStatus::Failed { .. }));
        assert_eq!(seen[2].1.len(), 2);
    }
}
