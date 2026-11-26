//! What Housekeeper's root helper does (decisions D4, D25, D31, D40): one typed operation in, one reply out.
//!
//! The helper binary reads a [`HelperRequest`] as JSON on its standard input, hands it to [`carry_out`] and prints the
//! [`HelperReply`]. Everything it is given is checked here: the package tool must be this system's, every package name
//! must be a plain name the package database lists as installed, and the helper builds the command itself; a path to
//! trash must be a plain one, no package's, and not part of the system. Nothing from the caller ever becomes an option
//! or a shell string.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::command::Cmd;
use crate::history::unix_now;
use crate::host::Host;
use crate::platform::Platform;
use crate::source::SourceError;
use crate::trash::trash;

/// Version of the request and reply format. A helper refuses requests of another version, so a front end and a helper
/// from different releases never misread each other.
pub const HELPER_PROTOCOL: u32 = 1;

/// Most packages one request may remove.
pub const MAX_PACKAGES: usize = 100;

/// How long a package tool may take to remove packages before it is stopped.
pub const REMOVAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// What a front end asks the helper to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperRequest {
    /// [`HELPER_PROTOCOL`] of the front end that wrote it.
    pub protocol: u32,
    pub operation: HelperOperation,
}

/// One change that needs root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HelperOperation {
    /// Removes installed packages through the system's package tool: the `tool` and `packages` of a plan's
    /// [`Action::RemovePackages`](crate::remove::Action::RemovePackages) step, and nothing else.
    RemovePackages { tool: String, packages: Vec<String> },
    /// Moves a file or folder outside every home folder to root's Trash: a plan's
    /// [`Action::Trash`](crate::remove::Action::Trash) step for the helper (an AppImage in `/opt`, a recipe's folder).
    Trash { path: PathBuf },
}

/// How a front end reaches the root helper: through `pkexec`, which asks for an administrator's password, or directly
/// when it already runs as root.
pub trait RootHelper: Send + Sync {
    /// The helper's reply to `request`, or why it could not be asked (not installed, authentication refused).
    fn ask(&self, host: &Host, request: &HelperRequest) -> Result<HelperReply, String>;
}

/// The effective user ID in a Linux `/proc/<pid>/status`: the second number of its `Uid:` line (real, effective, saved,
/// file system).
pub fn effective_uid(status: &str) -> Option<&str> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().nth(1))
}

/// What came of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum HelperReply {
    /// The command ran and succeeded.
    Done {
        command: Vec<String>,
        stdout: String,
        stderr: String,
    },
    /// The command ran and failed, or could not be run (`code` is `None`; `stderr` says why).
    Failed {
        command: Vec<String>,
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    /// Nothing was run: the request was not valid, or not true of this system.
    Refused { reason: String },
}

impl HelperReply {
    pub fn refused(reason: impl Into<String>) -> Self {
        Self::Refused { reason: reason.into() }
    }

    /// The helper's exit code: 0 done, 1 failed, 2 refused.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Done { .. } => 0,
            Self::Failed { .. } => 1,
            Self::Refused { .. } => 2,
        }
    }
}

/// What the root helper needs from the system's package tool (D24, D25): whether a package is installed, asked of the
/// package database, and the command that removes packages. The same command is shown in removal plans.
pub trait PackageRemoval: Send + Sync {
    /// The tool's name in plans (`apt`), as in [`Action::RemovePackages`](crate::remove::Action::RemovePackages).
    fn tool(&self) -> &'static str;

    /// Whether the package database lists `package` (a name as the plan gives it) as installed.
    fn is_installed(&self, host: &Host, package: &str) -> Result<bool, SourceError>;

    /// The command that removes `packages`: the tool's fixed arguments followed by the names.
    fn command(&self, packages: &[String]) -> Vec<String>;
}

/// Checks `request` against `platform` and carries it out on `host`.
pub fn carry_out(platform: &dyn Platform, host: &Host, request: &HelperRequest) -> HelperReply {
    if request.protocol != HELPER_PROTOCOL {
        return HelperReply::refused(format!(
            "the request is in version {} of the helper's format; this helper reads version {HELPER_PROTOCOL}",
            request.protocol
        ));
    }
    match &request.operation {
        HelperOperation::RemovePackages { tool, packages } => remove_packages(platform, host, tool, packages),
        HelperOperation::Trash { path } => trash_system_path(platform, host, path),
    }
}

/// Trashes `path` when it is a plain absolute path at least two folders deep, holds no symbolic link, is not (and
/// holds no) folder the system is made of, and no package owns it (a package's files go with the package).
fn trash_system_path(platform: &dyn Platform, host: &Host, path: &Path) -> HelperReply {
    let shown = path.display();
    let plain = path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    if !plain {
        return HelperReply::refused(format!("{shown} is not a plain absolute path"));
    }
    if path.components().count() < 3 {
        return HelperReply::refused(format!("{shown} is too close to the root folder"));
    }
    if platform
        .system_paths()
        .iter()
        .any(|system| Path::new(system).starts_with(path))
    {
        return HelperReply::refused(format!("{shown} is, or holds, a folder the system is made of"));
    }
    if platform.home_paths().iter().any(|home| path.starts_with(home)) {
        return HelperReply::refused(format!("{shown} is in a home folder; its user removes it, not root"));
    }
    if host.canonical(path) != path {
        return HelperReply::refused(format!("{shown} has a symbolic link in its path"));
    }
    if host.symlink_metadata(path).is_none() {
        return HelperReply::refused(format!("{shown} is not there"));
    }
    for lookup in platform.sources().iter().filter_map(|source| source.owner_lookup()) {
        match lookup.owners(host, &[path.to_path_buf()]) {
            Ok(owners) => {
                if let Some(owner) = owners.values().flatten().next() {
                    return HelperReply::refused(format!("{shown} belongs to the package {owner}, which removes it"));
                }
            }
            Err(e) => return HelperReply::refused(format!("could not check which package owns {shown}: {e}")),
        }
    }
    let command = vec!["trash".to_string(), shown.to_string()];
    match trash(host, path, unix_now()) {
        Ok(trashed) => HelperReply::Done {
            command,
            stdout: format!("moved to {}\n", trashed.to.display()),
            stderr: String::new(),
        },
        Err(e) => HelperReply::Failed {
            command,
            code: None,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

fn remove_packages(platform: &dyn Platform, host: &Host, tool: &str, packages: &[String]) -> HelperReply {
    let Some(removal) = platform.package_removal() else {
        return HelperReply::refused("this system has no package tool Housekeeper can remove packages with");
    };
    if tool != removal.tool() {
        return HelperReply::refused(format!(
            "this system's packages are removed with {}, not {tool}",
            removal.tool()
        ));
    }
    if packages.is_empty() || packages.len() > MAX_PACKAGES {
        return HelperReply::refused(format!(
            "a request removes 1 to {MAX_PACKAGES} packages, not {}",
            packages.len()
        ));
    }
    if let Some(name) = packages.iter().find(|name| !is_package_name(name)) {
        return HelperReply::refused(format!("{name:?} is not a package name"));
    }
    for name in packages {
        match removal.is_installed(host, name) {
            Ok(true) => {}
            Ok(false) => return HelperReply::refused(format!("{name} is not installed")),
            Err(e) => return HelperReply::refused(format!("could not check whether {name} is installed: {e}")),
        }
    }
    let command = removal.command(packages);
    let Some((program, args)) = command.split_first() else {
        return HelperReply::refused(format!("{tool} gave no command"));
    };
    let cmd = Cmd::new(program).args(args).timeout(REMOVAL_TIMEOUT);
    match host.run(&cmd) {
        Ok(output) if output.success() => HelperReply::Done {
            command,
            stdout: output.stdout,
            stderr: output.stderr,
        },
        Ok(output) => HelperReply::Failed {
            command,
            code: output.code,
            stdout: output.stdout,
            stderr: output.stderr,
        },
        Err(e) => HelperReply::Failed {
            command,
            code: None,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

/// A package name as package databases write them (`libc6:i386`, `bash-5.3.20-11.1.x86_64`, `gtk+3`, `kernel-1:6.1`):
/// letters, digits and `+ - . _ : ~ ^ @`, starting with a letter or digit, so it can never be read as an option, a
/// pattern (`*`, `?`, `[`) or a path.
pub fn is_package_name(name: &str) -> bool {
    name.len() <= 256
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.' | '_' | ':' | '~' | '^' | '@'))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::command::Output;
    use crate::model::SystemInfo;
    use crate::source::Source;
    use crate::testing::{FakeRunner, Fixture};

    /// `tool` removes with `tool remove --yes <names>` and lists `installed` as installed.
    struct Tool {
        installed: &'static [&'static str],
    }

    impl PackageRemoval for Tool {
        fn tool(&self) -> &'static str {
            "tool"
        }
        fn is_installed(&self, host: &Host, package: &str) -> Result<bool, SourceError> {
            // Asks the database like a real tool, so a test sees the check run.
            host.run_ok(&Cmd::new("db").arg(package))?;
            Ok(self.installed.contains(&package))
        }
        fn command(&self, packages: &[String]) -> Vec<String> {
            ["tool", "remove", "--yes"]
                .into_iter()
                .map(str::to_string)
                .chain(packages.iter().cloned())
                .collect()
        }
    }

    struct System {
        system: SystemInfo,
        tool: Option<Tool>,
    }

    impl Platform for System {
        fn system(&self) -> &SystemInfo {
            &self.system
        }
        fn sources(&self) -> &[Arc<dyn Source>] {
            &[]
        }
        fn package_removal(&self) -> Option<&dyn PackageRemoval> {
            self.tool.as_ref().map(|tool| tool as &dyn PackageRemoval)
        }
        fn system_paths(&self) -> &'static [&'static str] {
            &["/opt", "/usr", "/usr/share"]
        }
        fn home_paths(&self) -> &'static [&'static str] {
            &["/home", "/root"]
        }
    }

    fn system(tool: Option<Tool>) -> System {
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
            tool,
        }
    }

    fn request(tool: &str, packages: &[&str]) -> HelperRequest {
        HelperRequest {
            protocol: HELPER_PROTOCOL,
            operation: HelperOperation::RemovePackages {
                tool: tool.into(),
                packages: packages.iter().map(|p| p.to_string()).collect(),
            },
        }
    }

    /// Carries out `request` where `runner` answers; an unexpected command panics, so a refusal that ran something
    /// fails the test.
    fn reply(tool: Option<Tool>, runner: FakeRunner, request: &HelperRequest) -> HelperReply {
        let fixture = Fixture::new();
        carry_out(&system(tool), &fixture.host(runner), request)
    }

    const VIM: Tool = Tool { installed: &["vim"] };

    fn refusal(reply: HelperReply) -> String {
        match reply {
            HelperReply::Refused { reason } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn installed_packages_are_removed_with_the_tools_own_command() {
        let runner = FakeRunner::new()
            .respond("db vim", "")
            .respond("tool remove --yes vim", "Removing vim\n");
        let reply = reply(Some(VIM), runner, &request("tool", &["vim"]));
        assert_eq!(
            reply,
            HelperReply::Done {
                command: vec!["tool".into(), "remove".into(), "--yes".into(), "vim".into()],
                stdout: "Removing vim\n".into(),
                stderr: String::new(),
            }
        );
        assert_eq!(reply.exit_code(), 0);
    }

    #[test]
    fn a_failing_tool_is_reported_with_its_output() {
        let runner = FakeRunner::new().respond("db vim", "").respond_with(
            "tool remove --yes vim",
            Output {
                code: Some(100),
                stdout: String::new(),
                stderr: "E: lock held\n".into(),
            },
        );
        let reply = reply(Some(VIM), runner, &request("tool", &["vim"]));
        assert!(matches!(&reply, HelperReply::Failed { code: Some(100), stderr, .. } if stderr == "E: lock held\n"));
        assert_eq!(reply.exit_code(), 1);
    }

    #[test]
    fn names_that_could_be_options_patterns_or_paths_are_refused_before_anything_runs() {
        for name in [
            "-y",
            "--purge",
            "vim*",
            "vi?",
            "[v]im",
            "../etc",
            "/usr/bin/vim",
            "vim vim",
            "",
            "vim\n",
        ] {
            let reason = refusal(reply(Some(VIM), FakeRunner::new(), &request("tool", &["vim", name])));
            assert!(reason.contains("is not a package name"), "{name:?}: {reason}");
        }
        for name in [
            "libc6:i386",
            "bash-5.3.20-11.1.x86_64",
            "gtk+3",
            "kernel-1:6.1~rc1^git",
            "python@3",
        ] {
            assert!(is_package_name(name), "{name}");
        }
    }

    #[test]
    fn a_package_the_database_does_not_list_is_refused() {
        let runner = FakeRunner::new().respond("db vim", "").respond("db emacs", "");
        let reason = refusal(reply(Some(VIM), runner, &request("tool", &["vim", "emacs"])));
        assert_eq!(reason, "emacs is not installed");
    }

    #[test]
    fn another_systems_tool_or_no_tool_is_refused() {
        let reason = refusal(reply(Some(VIM), FakeRunner::new(), &request("dnf", &["vim"])));
        assert_eq!(reason, "this system's packages are removed with tool, not dnf");
        let reason = refusal(reply(None, FakeRunner::new(), &request("tool", &["vim"])));
        assert!(reason.contains("no package tool"), "{reason}");
    }

    #[test]
    fn empty_oversized_and_other_version_requests_are_refused() {
        assert!(refusal(reply(Some(VIM), FakeRunner::new(), &request("tool", &[]))).contains("1 to 100"));
        let many = vec!["vim"; MAX_PACKAGES + 1];
        assert!(refusal(reply(Some(VIM), FakeRunner::new(), &request("tool", &many))).contains("not 101"));
        let mut old = request("tool", &["vim"]);
        old.protocol = HELPER_PROTOCOL + 1;
        assert!(refusal(reply(Some(VIM), FakeRunner::new(), &old)).contains("version 2"));
    }

    fn trash_request(path: &str) -> HelperRequest {
        HelperRequest {
            protocol: HELPER_PROTOCOL,
            operation: HelperOperation::Trash { path: path.into() },
        }
    }

    #[test]
    fn a_system_file_goes_to_roots_trash() {
        let fixture = Fixture::new();
        fixture.file("/opt/Tool.AppImage", "ELF");
        let host = fixture.host(FakeRunner::new());
        let reply = carry_out(&system(None), &host, &trash_request("/opt/Tool.AppImage"));
        assert!(matches!(reply, HelperReply::Done { .. }), "{reply:?}");
        assert!(!host.exists("/opt/Tool.AppImage"));
    }

    #[test]
    fn the_helper_trashes_nothing_the_system_or_a_user_owns() {
        let fixture = Fixture::new();
        fixture
            .file("/usr/share/applications/tool.desktop", "")
            .file("/home/user/precious", "")
            .file("/opt/real/Tool.AppImage", "")
            .symlink("/opt/link", "/opt/real");
        let host = fixture.host(FakeRunner::new());
        let refused = |path: &str| refusal(carry_out(&system(None), &host, &trash_request(path)));
        assert!(refused("opt/Tool.AppImage").contains("not a plain absolute path"));
        assert!(refused("/opt/../etc/passwd").contains("not a plain absolute path"));
        assert!(refused("/opt").contains("too close to the root folder"));
        assert!(refused("/usr/share").contains("a folder the system is made of"));
        assert!(refused("/home/user/precious").contains("in a home folder"));
        assert!(refused("/opt/link/Tool.AppImage").contains("symbolic link"));
        assert!(refused("/opt/gone").contains("not there"));
        assert!(host.exists("/home/user/precious") && host.exists("/opt/real/Tool.AppImage"));
    }

    #[test]
    fn only_an_effective_root_user_counts_as_root() {
        let status = |uids: &str| format!("Name:\thousekeeper-help\nUmask:\t0022\nUid:\t{uids}\nGid:\t0\t0\t0\t0\n");
        assert_eq!(effective_uid(&status("0\t0\t0\t0")), Some("0"));
        assert_eq!(
            effective_uid(&status("0\t1000\t0\t1000")),
            Some("1000"),
            "real root, effective user"
        );
        assert_eq!(effective_uid("Name:\tx\n"), None);
    }
}
