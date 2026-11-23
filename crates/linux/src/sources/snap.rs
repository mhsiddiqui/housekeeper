//! Snaps, read from snapd's REST API on its unix socket (the same API `snap` and App Center use).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use housekeeper_core::{
    Availability, Host, InstallReason, Package, PackageKind, Scope, Source, SourceError, SourceId, SourceInfo,
    SourceKind,
};
use serde::Deserialize;

use crate::provider::{Context, LinuxSource};

pub const ID: SourceId = SourceId::new("snap");

const SOCKET: &str = "/run/snapd.socket";
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct SnapSource;

impl LinuxSource for SnapSource {
    fn create(_: &Context<'_>) -> Self {
        Self
    }
}

impl Source for SnapSource {
    fn info(&self) -> SourceInfo {
        SourceInfo::new("snap", "Snap", SourceKind::Universal)
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

    fn inventory(&self, host: &Host) -> Result<Vec<Package>, SourceError> {
        let snaps: Vec<Snap> = get(&host.real_path(SOCKET), "/v2/snaps")?;
        Ok(snaps.into_iter().map(Snap::into_package).collect())
    }
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
        (package.kind, package.reason, package.protected) = match self.snap_type.as_str() {
            "snapd" | "os" | "kernel" | "gadget" => (PackageKind::Runtime, InstallReason::System, true),
            "base" => (PackageKind::Runtime, InstallReason::Dependency, false),
            // App snaps without commands are content providers (themes, GNOME/KDE runtimes) other snaps plug into.
            "app" if self.apps.is_empty() => (PackageKind::Runtime, InstallReason::Dependency, false),
            _ => (PackageKind::App, InstallReason::Explicit, false),
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
 "install-date":"2026-11-09T11:47:23.930407791+05:00","type":"app",
 "publisher":{"id":"OgeoZuqQpVvSr9eGKJzNCrFGSaKXpkey","username":"mozilla","display-name":"Mozilla","validation":"verified"},
 "apps":[{"snap":"firefox","name":"firefox"},{"snap":"firefox","name":"geckodriver"}],"confinement":"strict"},
{"name":"core24","title":"Core 24 base snap","summary":"Snap runtime environment","version":"20260824","type":"base",
 "installed-size":70033408,"install-date":"2026-09-18T19:59:40.40037538+05:00","tracking-channel":"latest/stable",
 "publisher":{"display-name":"Canonical","validation":"verified"}},
{"name":"snapd","title":"snapd","version":"2.77.1","type":"snapd","apps":[],
 "publisher":{"display-name":"Canonical","validation":"verified"}},
{"name":"gtk-common-themes","title":"gtk-common-themes","version":"0.1-81-g442e511","type":"app",
 "publisher":{"display-name":"Canonical","validation":"verified"}},
{"name":"hello-world","summary":"","version":"6.4","type":"app","apps":[{"name":"hello-world"},{"name":"evil"}],
 "publisher":{"display-name":"Canonical Ltd.","validation":"unproven"},"tracking-channel":"latest/edge"}
]}"#;

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
        let packages = SnapSource.inventory(&fixture.host(FakeRunner::new())).unwrap();
        assert!(server.join().unwrap().starts_with("GET /v2/snaps HTTP/1.0\r\n"));
        packages
    }

    fn find<'a>(packages: &'a [Package], id: &str) -> &'a Package {
        packages.iter().find(|p| p.id == id).unwrap()
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
        assert_eq!(
            firefox.origin.as_deref(),
            Some("Snap Store, Mozilla (verified), latest/stable/ubuntu-24.04")
        );
        assert_eq!(firefox.location.as_deref(), Some(Path::new("/snap/firefox/current")));
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
        assert!(snapd.protected);
        assert!(!core.protected);
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
        let err = SnapSource.inventory(&fixture.host(FakeRunner::new())).unwrap_err();
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
}
