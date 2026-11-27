//! Snaps, read from snapd's REST API on its unix socket (the same API `snap` and App Center use).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use housekeeper_core::{
    Actor, Availability, Cmd, Dependants, FileInstaller, FilePackage, Host, InstallFile, InstallPlan, InstallReason,
    Package, PackageKind, RemovalPlan, Remover, ScanContext, Scope, Source, SourceError, SourceId, SourceInfo,
    SourceKind, Step, Tier,
};
use serde::Deserialize;

use crate::formats::{field, machine, path_arg};
use crate::provider::{Context, FileFormat, LinuxSource, register};

pub const ID: SourceId = SourceId::new("snap");

const SOCKET: &str = "/run/snapd.socket";
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct SnapSource;

impl LinuxSource for SnapSource {
    const FILES: &'static [FileFormat] = &[FileFormat {
        id: "snap",
        title: "Snap package",
        suffixes: &[".snap"],
        matches: is_squashfs,
        evidence: "a SquashFS image (`hsqs`) named `.snap`",
        read: read_snap_file,
    }];

    fn create(_: &Context<'_>) -> Self {
        Self
    }
}
register!(SnapSource);

/// A `.snap` is a SquashFS image (little-endian magic `hsqs`).
fn is_squashfs(head: &[u8]) -> bool {
    head.starts_with(b"hsqs")
}

/// What a `.snap` holds, from its `meta/snap.yaml` through `unsquashfs` (squashfs-tools, which snapd needs); nothing
/// on systems without it.
fn read_snap_file(host: &Host, path: &Path, _: &[u8]) -> Result<Option<FilePackage>, SourceError> {
    if !host.has_program("unsquashfs") {
        return Ok(None);
    }
    let yaml = host.run_ok(
        &Cmd::new("unsquashfs")
            .arg("-cat")
            .arg(path_arg(host, path))
            .arg("meta/snap.yaml"),
    )?;
    let value = |key: &str| field(&yaml, key, ':').map(str::to_string);
    Ok(Some(FilePackage {
        name: value("name"),
        version: value("version"),
        title: value("title"),
        arch: only_architecture(&yaml).and_then(machine),
        ..FilePackage::default()
    }))
}

/// The one processor `snap.yaml`'s `architectures` names (`[amd64]` or a `- amd64` list); `None` for several, none
/// (snapd then takes the snap for the processor it was built on, which the file does not say) or `all`.
fn only_architecture(yaml: &str) -> Option<&str> {
    let mut lines = yaml.lines();
    let inline = lines.find_map(|line| line.strip_prefix("architectures:"))?.trim();
    let names: Vec<&str> = if inline.is_empty() {
        lines
            .map_while(|line| line.trim_start().strip_prefix('-'))
            .map(str::trim)
            .collect()
    } else {
        inline.trim_matches(['[', ']']).split(',').map(str::trim).collect()
    };
    match names.as_slice() {
        [name] => Some(name.trim_matches(['\'', '"'])),
        _ => None,
    }
}

impl Source for SnapSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("snap", "Snap", SourceKind::Universal).evidence("snapd's API (`/v2/snaps`)")
    }

    fn probe(&self, host: &Host) -> Availability {
        if !host.exists(SOCKET) {
            return Availability::unavailable("snapd is not running");
        }
        let version = get::<SystemInfo>(&host.real_path(SOCKET), "/v2/system-info")
            .ok()
            .map(|info| info.version);
        Availability::Available { version }
    }

    fn install_roots(&self, _host: &Host) -> Vec<PathBuf> {
        vec![PathBuf::from("/snap")]
    }

    fn inventory(&self, host: &Host, _: &ScanContext<'_>) -> Result<Vec<Package>, SourceError> {
        let snaps: Vec<Snap> = get(&host.real_path(SOCKET), "/v2/snaps")?;
        Ok(snaps.into_iter().map(Snap::into_package).collect())
    }

    fn dependant_lookup(&self) -> Option<&dyn Dependants> {
        Some(self)
    }

    fn remover(&self) -> Option<&dyn Remover> {
        Some(self)
    }

    fn file_installer(&self) -> Option<&dyn FileInstaller> {
        Some(self)
    }
}

impl Remover for SnapSource {
    /// `snap remove <name>`, as the user: snapd asks for authorisation itself.
    fn plan(&self, _host: &Host, package: &Package) -> Result<RemovalPlan, SourceError> {
        Ok(RemovalPlan::new(
            Tier::PackageManager,
            vec![Step::run(Actor::User, ["snap", "remove", package.name.as_str()])],
        ))
    }
}

impl FileInstaller for SnapSource {
    /// With the store's signatures for it (the `.assert` file `snap download` saves next to it), they are
    /// acknowledged first and the snap installs as the store's. Without them only `--dangerous` installs it, which
    /// skips the signature check; the plan says so. As the user: snapd asks for authorisation itself.
    fn plan(&self, host: &Host, file: &InstallFile<'_>) -> Result<InstallPlan, SourceError> {
        let snap = path_arg(host, file.path);
        let assertions = file.path.with_extension("assert");
        if host.exists(&assertions) {
            return Ok(InstallPlan::new(vec![
                Step::run(
                    Actor::User,
                    ["snap".to_string(), "ack".into(), path_arg(host, &assertions)],
                ),
                Step::run(Actor::User, ["snap".to_string(), "install".into(), snap]),
            ]));
        }
        Ok(InstallPlan::new(vec![Step::run(
            Actor::User,
            ["snap".to_string(), "install".into(), "--dangerous".into(), snap],
        )])
        .caution(
            "installed with --dangerous: no Snap Store signature (no .assert file next to it), so nobody vouches \
                     for where it came from",
        ))
    }
}

impl Dependants for SnapSource {
    fn dependants(&self, host: &Host, package: &Package) -> Result<Vec<String>, SourceError> {
        let socket = host.real_path(SOCKET);
        let snaps: Vec<Snap> = get(&socket, "/v2/snaps")?;
        let connections: Connections = get(&socket, "/v2/connections")?;
        Ok(dependants_of(&package.name, &snaps, &connections))
    }
}

/// The snaps that run on `target` as their base, or plug into a `content` slot it offers (GNOME/KDE runtimes,
/// themes), sorted and without `target`.
fn dependants_of(target: &str, snaps: &[Snap], connections: &Connections) -> Vec<String> {
    let on_base = snaps.iter().filter(|snap| match snap.base.as_deref() {
        Some(base) => base == target,
        // App snaps that name no base run on `core`.
        None => snap.snap_type == "app" && target == "core",
    });
    let plugged = connections
        .established
        .iter()
        .filter(|connection| connection.interface == "content" && connection.slot.snap == target)
        .map(|connection| connection.plug.snap.clone());
    let mut names: Vec<String> = on_base.map(|snap| snap.name.clone()).chain(plugged).collect();
    names.retain(|name| name != target);
    names.sort();
    names.dedup();
    names
}

#[derive(Deserialize)]
struct SystemInfo {
    version: String,
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(rename = "status-code")]
    status_code: u16,
    result: T,
}

#[derive(Deserialize)]
struct Snap {
    name: String,
    title: Option<String>,
    summary: Option<String>,
    version: Option<String>,
    #[serde(rename = "type")]
    snap_type: String,
    #[serde(rename = "installed-size")]
    installed_size: Option<u64>,
    #[serde(rename = "install-date")]
    install_date: Option<String>,
    #[serde(rename = "tracking-channel")]
    tracking_channel: Option<String>,
    publisher: Option<Publisher>,
    #[serde(default)]
    apps: Vec<App>,
    /// The base snap this snap runs on; absent for bases, snapd and app snaps on the original `core`.
    base: Option<String>,
    /// `strict`, `devmode` or `classic`.
    confinement: Option<String>,
}

#[derive(Deserialize)]
struct Publisher {
    #[serde(rename = "display-name")]
    display_name: String,
    validation: Option<String>,
}

#[derive(Deserialize)]
struct App {
    name: String,
    /// The app's AppStream ID, when the snap declares one.
    #[serde(rename = "common-id")]
    common_id: Option<String>,
}

/// The `result` of `GET /v2/connections`: by default only the established connections are of interest.
#[derive(Deserialize)]
struct Connections {
    #[serde(default)]
    established: Vec<Connection>,
}

#[derive(Deserialize)]
struct Connection {
    interface: String,
    slot: SnapRef,
    plug: SnapRef,
}

/// The snap end of a plug or slot.
#[derive(Deserialize)]
struct SnapRef {
    snap: String,
}

impl Snap {
    fn into_package(self) -> Package {
        let mut package = Package::new(ID, &self.name, &self.name, Scope::System);
        // snapd falls back to the name when a snap has no title.
        package.title = self.title.filter(|title| *title != self.name);
        package.summary = self.summary.filter(|s| !s.is_empty());
        package.version = self.version;
        package.size_bytes = self.installed_size;
        package.installed_at = self.install_date.as_deref().and_then(parse_rfc3339);
        package.origin = Some(origin(self.publisher.as_ref(), self.tracking_channel.as_deref()));
        package.location = Some(PathBuf::from("/snap").join(&self.name).join("current"));
        package.commands = self
            .apps
            .iter()
            .map(|app| {
                if app.name == self.name {
                    app.name.clone()
                } else {
                    format!("{}.{}", self.name, app.name)
                }
            })
            .collect();
        package.app_ids = self.apps.iter().filter_map(|app| app.common_id.clone()).collect();
        package.app_ids.retain(|id| !id.is_empty());
        package.app_ids.sort();
        package.app_ids.dedup();
        // A confined snap (strict, or devmode, which only stops enforcing it) gets its own `$HOME` and XDG folders in
        // `~/snap/<name>`; a classic snap uses the real ones.
        package.sandboxed = matches!(self.confinement.as_deref(), Some("strict" | "devmode"));
        (package.kind, package.reason, package.protected) = match self.snap_type.as_str() {
            "snapd" => (
                PackageKind::Runtime,
                InstallReason::System,
                Some("it runs every snap".to_string()),
            ),
            kind @ ("os" | "kernel" | "gadget") => (
                PackageKind::Runtime,
                InstallReason::System,
                Some(format!("the system runs on its {kind} snap")),
            ),
            "base" => (PackageKind::Runtime, InstallReason::Dependency, None),
            // App snaps without commands are content providers (themes, GNOME/KDE runtimes) other snaps plug into.
            "app" if self.apps.is_empty() => (PackageKind::Runtime, InstallReason::Dependency, None),
            _ => (PackageKind::App, InstallReason::Explicit, None),
        };
        package
    }
}

fn origin(publisher: Option<&Publisher>, channel: Option<&str>) -> String {
    let mut origin = String::from("Snap Store");
    if let Some(publisher) = publisher {
        origin.push_str(", ");
        origin.push_str(&publisher.display_name);
        if matches!(publisher.validation.as_deref(), Some("verified" | "starred")) {
            origin.push_str(" (verified)");
        }
    }
    if let Some(channel) = channel {
        origin.push_str(", ");
        origin.push_str(channel);
    }
    origin
}

/// GET `path` from snapd and return the `result` of its JSON envelope.
fn get<T: for<'de> Deserialize<'de>>(socket: &Path, path: &str) -> Result<T, SourceError> {
    let what = format!("snapd {path}");
    let io = |e| SourceError::io(socket, e);
    let mut stream = UnixStream::connect(socket).map_err(io)?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(io)?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(io)?;
    // HTTP/1.0: snapd answers with Content-Length or closes the connection, never chunked.
    let request = format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(io)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(io)?;

    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| SourceError::parse(&what, "no end of HTTP headers"))?;
    let body = &response[split + 4..];
    let envelope: Envelope<serde_json::Value> =
        serde_json::from_slice(body).map_err(|e| SourceError::parse(&what, e.to_string()))?;
    if envelope.status_code != 200 {
        let message = envelope.result.get("message").and_then(|m| m.as_str()).unwrap_or("");
        return Err(SourceError::parse(
            &what,
            format!("status {}: {message}", envelope.status_code),
        ));
    }
    serde_json::from_value(envelope.result).map_err(|e| SourceError::parse(&what, e.to_string()))
}

/// Seconds since the Unix epoch of an RFC 3339 timestamp such as `2026-11-09T11:47:23.930407791+05:00`.
fn parse_rfc3339(text: &str) -> Option<i64> {
    let (date, time) = text.split_once(['T', 't', ' '])?;
    let mut date_parts = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (
        date_parts.next()?.ok()?,
        date_parts.next()?.ok()?,
        date_parts.next()?.ok()?,
    );

    let (clock, offset_seconds) = if let Some(clock) = time.strip_suffix(['Z', 'z']) {
        (clock, 0)
    } else {
        let sign_at = time.rfind(['+', '-'])?;
        let (clock, offset) = time.split_at(sign_at);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = offset[1..].split_once(':')?;
        (
            clock,
            sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60),
        )
    };
    let clock = clock.split('.').next()?;
    let mut clock_parts = clock.splitn(3, ':').map(str::parse::<i64>);
    let (hour, minute, second) = (
        clock_parts.next()?.ok()?,
        clock_parts.next()?.ok()?,
        clock_parts.next()?.ok()?,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};
    use std::os::unix::net::UnixListener;
    use std::thread;

    const SNAPS: &str = r#"{"type":"sync","status-code":200,"status":"OK","result":[
{"name":"firefox","title":"firefox","summary":"Mozilla Firefox web browser","version":"159.0.1-1","revision":"9171",
 "channel":"latest/stable","tracking-channel":"latest/stable/ubuntu-24.04","installed-size":298881024,
 "install-date":"2026-11-09T11:47:23.930407791+05:00","type":"app","base":"core24",
 "publisher":{"id":"OgeoZuqQpVvSr9eGKJzNCrFGSaKXpkey","username":"mozilla","display-name":"Mozilla","validation":"verified"},
 "apps":[{"snap":"firefox","name":"firefox","common-id":"org.mozilla.firefox"},{"snap":"firefox","name":"geckodriver"}],"confinement":"strict"},
{"name":"core24","title":"Core 24 base snap","summary":"Snap runtime environment","version":"20260824","type":"base",
 "installed-size":70033408,"install-date":"2026-09-18T19:59:40.40037538+05:00","tracking-channel":"latest/stable",
 "publisher":{"display-name":"Canonical","validation":"verified"}},
{"name":"snapd","title":"snapd","version":"2.77.1","type":"snapd","apps":[],
 "publisher":{"display-name":"Canonical","validation":"verified"}},
{"name":"gtk-common-themes","title":"gtk-common-themes","version":"0.1-81-g442e511","type":"app",
 "publisher":{"display-name":"Canonical","validation":"verified"}},
{"name":"hello-world","summary":"","version":"6.4","type":"app","apps":[{"name":"hello-world"},{"name":"evil"}],
 "publisher":{"display-name":"Canonical Ltd.","validation":"unproven"},"tracking-channel":"latest/edge"},
{"name":"sublime-merge","title":"Sublime Merge","version":"2121","type":"app","base":"core22",
 "apps":[{"name":"sublime-merge"}],"publisher":{"display-name":"Snapcrafters","validation":"starred"},
 "confinement":"classic"}
]}"#;

    const CONNECTIONS: &str = r#"{"type":"sync","status-code":200,"status":"OK","result":{"established":[
{"slot":{"snap":"gtk-common-themes","slot":"gtk-3-themes"},"plug":{"snap":"firefox","plug":"gtk-3-themes"},
 "interface":"content","plug-attrs":{"content":"gtk-3-themes","default-provider":"gtk-common-themes",
 "target":"$SNAP/data-dir/themes"},"slot-attrs":{"content":"gtk-3-themes","source":{"read":["$SNAP/share/themes/Yaru"]}}},
{"slot":{"snap":"snapd","slot":"network"},"plug":{"snap":"firefox","plug":"network"},"interface":"network"},
{"slot":{"snap":"gtk-common-themes","slot":"icon-themes"},"plug":{"snap":"gtk-common-themes","plug":"icon-themes"},
 "interface":"content"}
],"plugs":[],"slots":[]}}"#;

    /// Serves one canned HTTP response on the fixture's snapd socket.
    fn serve(fixture: &Fixture, response: String) -> thread::JoinHandle<String> {
        fixture.dir("/run");
        let host = fixture.host(FakeRunner::new());
        let listener = UnixListener::bind(host.real_path(SOCKET)).unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                request.push(byte[0]);
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        })
    }

    fn http(body: &str) -> String {
        format!(
            "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    fn scan() -> Vec<Package> {
        let fixture = Fixture::new();
        let server = serve(&fixture, http(SNAPS));
        let packages = SnapSource
            .inventory(&fixture.host(FakeRunner::new()), &ScanContext::default())
            .unwrap();
        assert!(server.join().unwrap().starts_with("GET /v2/snaps HTTP/1.0\r\n"));
        packages
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn removal_runs_snap_remove_as_the_user() {
        let packages = scan();
        let fixture = Fixture::new();
        let plan = Remover::plan(
            &SnapSource,
            &fixture.host(FakeRunner::new()),
            find(&packages, "firefox"),
        )
        .unwrap();
        assert_eq!(
            plan,
            RemovalPlan::new(
                Tier::PackageManager,
                vec![Step::run(Actor::User, ["snap", "remove", "firefox"])]
            )
        );
    }

    #[test]
    fn app_snaps_with_their_commands() {
        let packages = scan();
        let firefox = find(&packages, "firefox");
        assert_eq!(
            (firefox.kind, firefox.reason),
            (PackageKind::App, InstallReason::Explicit)
        );
        assert_eq!(firefox.title, None, "title equal to the name adds nothing");
        assert_eq!(firefox.version.as_deref(), Some("159.0.1-1"));
        assert_eq!(firefox.size_bytes, Some(298_881_024));
        assert_eq!(firefox.installed_at, Some(1_794_206_843));
        assert_eq!(firefox.commands, ["firefox", "firefox.geckodriver"]);
        assert_eq!(firefox.app_ids, ["org.mozilla.firefox"]);
        assert_eq!(
            firefox.origin.as_deref(),
            Some("Snap Store, Mozilla (verified), latest/stable/ubuntu-24.04")
        );
        assert_eq!(firefox.location.as_deref(), Some(Path::new("/snap/firefox/current")));
    }

    #[test]
    fn confined_snaps_are_sandboxed_and_classic_ones_are_not() {
        let packages = scan();
        assert!(
            find(&packages, "firefox").sandboxed,
            "strict: its settings are under ~/snap/firefox"
        );
        assert!(
            !find(&packages, "sublime-merge").sandboxed,
            "classic: it writes the real ~/.config/sublime-merge"
        );
    }

    #[test]
    fn bases_content_snaps_and_snapd_are_not_user_apps() {
        let packages = scan();
        let core = find(&packages, "core24");
        assert_eq!(
            (core.kind, core.reason),
            (PackageKind::Runtime, InstallReason::Dependency)
        );
        assert_eq!(core.title.as_deref(), Some("Core 24 base snap"));
        let themes = find(&packages, "gtk-common-themes");
        assert_eq!(
            (themes.kind, themes.reason),
            (PackageKind::Runtime, InstallReason::Dependency)
        );
        let snapd = find(&packages, "snapd");
        assert_eq!(snapd.reason, InstallReason::System);
        assert_eq!(snapd.protected.as_deref(), Some("it runs every snap"));
        assert_eq!(core.protected, None);
    }

    #[test]
    fn unverified_publishers_are_not_marked_verified() {
        let packages = scan();
        let hello = find(&packages, "hello-world");
        assert_eq!(hello.origin.as_deref(), Some("Snap Store, Canonical Ltd., latest/edge"));
        assert_eq!(hello.summary, None);
        assert_eq!(hello.installed_at, None);
    }

    #[test]
    fn snapd_errors_are_reported() {
        let fixture = Fixture::new();
        let body = r#"{"type":"error","status-code":401,"status":"Unauthorized","result":{"message":"access denied"}}"#;
        let server = serve(&fixture, http(body).replace("200 OK", "401 Unauthorized"));
        let err = SnapSource
            .inventory(&fixture.host(FakeRunner::new()), &ScanContext::default())
            .unwrap_err();
        server.join().unwrap();
        assert!(err.to_string().contains("access denied"), "{err}");
    }

    #[test]
    fn probe_without_socket_is_unavailable_and_with_socket_reads_the_version() {
        let fixture = Fixture::new();
        assert!(matches!(
            SnapSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Unavailable { .. }
        ));
        let body = r#"{"type":"sync","status-code":200,"result":{"series":"16","version":"2.77.1+ubuntu26.04.2"}}"#;
        let server = serve(&fixture, http(body));
        assert_eq!(
            SnapSource.probe(&fixture.host(FakeRunner::new())),
            Availability::Available {
                version: Some("2.77.1+ubuntu26.04.2".into())
            }
        );
        assert!(server.join().unwrap().starts_with("GET /v2/system-info "));
    }

    #[test]
    fn dependants_run_on_the_base_or_plug_into_its_content() {
        let snaps: Envelope<Vec<Snap>> = serde_json::from_str(SNAPS).unwrap();
        let connections: Envelope<Connections> = serde_json::from_str(CONNECTIONS).unwrap();
        let dependants = |target: &str| dependants_of(target, &snaps.result, &connections.result);
        assert_eq!(dependants("core24"), ["firefox"]);
        assert_eq!(
            dependants("gtk-common-themes"),
            ["firefox"],
            "a snap is not its own dependant"
        );
        assert_eq!(
            dependants("core"),
            ["gtk-common-themes", "hello-world"],
            "no base means core"
        );
        assert!(dependants("snapd").is_empty(), "only content connections count");
        assert!(dependants("firefox").is_empty());
    }

    #[test]
    fn rfc3339_timestamps() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339("2026-11-09T11:47:23.930407791+05:00"),
            Some(1_794_206_843)
        );
        assert_eq!(parse_rfc3339("2026-11-09T06:47:23Z"), Some(1_794_206_843));
        assert_eq!(parse_rfc3339("2026-11-09T01:17:23-05:30"), Some(1_794_206_843));
        assert_eq!(parse_rfc3339("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_rfc3339("0001-01-01T00:00:00Z"), Some(-62_135_596_800));
        assert_eq!(parse_rfc3339("not a date"), None);
        assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn a_snap_file_is_read_from_its_snap_yaml() {
        use crate::formats::test_support::inspect;
        let yaml = "name: hello\nversion: '6.4'\nsummary: says hello\narchitectures:\n  - s390x\napps:\n  hello:\n    \
                    command: bin/hello\n";
        let found = inspect("ID=ubuntu\n", "hello_29.snap", b"hsqs\0\0", |real| {
            FakeRunner::new().respond(&format!("unsquashfs -cat {real} meta/snap.yaml"), yaml)
        })
        .unwrap();
        let package = found.package.unwrap();
        assert_eq!(package.name.as_deref(), Some("hello"));
        assert_eq!(package.version.as_deref(), Some("6.4"));
        assert_eq!(package.arch.as_deref(), Some("s390x"));

        assert_eq!(only_architecture("architectures: [amd64, arm64]\n"), None);
        assert_eq!(only_architecture("architectures: [all]\n"), Some("all"));
        assert_eq!(only_architecture("name: x\n"), None);
    }

    #[test]
    fn a_snap_with_its_store_signatures_is_acknowledged_and_one_without_is_flagged_dangerous() {
        let fixture = Fixture::new();
        fixture.file("/home/user/Downloads/hello_29.snap", "hsqs");
        fixture.file("/home/user/Downloads/hello_29.assert", "type: account-key");
        fixture.file("/home/user/Downloads/local_1.snap", "hsqs");
        let host = fixture.host(FakeRunner::new());
        let plan = |name: &str| {
            let path = Path::new("/home/user/Downloads").join(name);
            let file = InstallFile {
                path: &path,
                format: "snap",
                package: None,
            };
            FileInstaller::plan(&SnapSource, &host, &file).unwrap()
        };
        let real = |name: &str| path_arg(&host, &Path::new("/home/user/Downloads").join(name));
        let signed = plan("hello_29.snap");
        assert_eq!(
            signed.steps,
            [
                Step::run(Actor::User, ["snap".to_string(), "ack".into(), real("hello_29.assert")]),
                Step::run(
                    Actor::User,
                    ["snap".to_string(), "install".into(), real("hello_29.snap")]
                ),
            ]
        );
        assert!(signed.cautions.is_empty());
        let local = plan("local_1.snap");
        assert_eq!(
            local.steps,
            [Step::run(
                Actor::User,
                [
                    "snap".to_string(),
                    "install".into(),
                    "--dangerous".into(),
                    real("local_1.snap")
                ]
            )]
        );
        assert_eq!(local.cautions.len(), 1);
    }
}
