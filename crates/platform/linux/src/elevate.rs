//! Reaching Housekeeper's root helper (decisions D4, D40): through `pkexec`, which asks for an administrator's password
//! (in a dialog from the desktop's polkit agent, or on the terminal), or directly when Housekeeper already runs as
//! root. Only the helper the packages install is used: a copy anywhere a user can write would run as root.

use std::time::Duration;

use housekeeper_core::helper::{STEP_TIMEOUT, effective_uid};
use housekeeper_core::{Cmd, HelperReply, HelperRequest, Host, RootHelper};

/// Where the packages install the helper (and where its polkit action points).
pub(crate) const HELPER: &str = "/usr/lib/housekeeper/housekeeper-helper";

/// The helper's own limit, plus time for the password prompt (and for copying a file it installs).
const ASK_TIMEOUT: Duration = Duration::from_secs(STEP_TIMEOUT.as_secs() + 15 * 60);

pub(crate) struct Pkexec;

impl RootHelper for Pkexec {
    fn ask(&self, host: &Host, request: &HelperRequest) -> Result<HelperReply, String> {
        if host.metadata(HELPER).is_none() {
            return Err(format!(
                "Housekeeper's root helper is not installed ({HELPER}); the packages install it"
            ));
        }
        let as_root = host
            .read_to_string("/proc/self/status")
            .is_ok_and(|status| effective_uid(&status) == Some("0"));
        let cmd = if as_root {
            Cmd::new(HELPER)
        } else if host.has_program("pkexec") {
            Cmd::new("pkexec").arg(HELPER)
        } else {
            return Err("pkexec is not installed (it comes with polkit), so nothing can be changed as root".into());
        };
        let input = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        let output = host
            .run(&cmd.stdin(input).timeout(ASK_TIMEOUT))
            .map_err(|e| e.to_string())?;
        match serde_json::from_str::<HelperReply>(output.stdout.trim()) {
            Ok(reply) => Ok(reply),
            // pkexec's own exit codes: the dialog was dismissed, or the user may not do this.
            Err(_) if output.code == Some(126) => Err("the administrator password was not given".into()),
            Err(_) if output.code == Some(127) => Err("not authorized to change the system's software".into()),
            Err(_) => Err(format!(
                "the root helper gave no answer{}",
                output
                    .stderr
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| format!(": {}", line.trim()))
                    .unwrap_or_default()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};
    use housekeeper_core::{HELPER_PROTOCOL, HelperOperation, Output};

    fn request() -> HelperRequest {
        HelperRequest {
            protocol: HELPER_PROTOCOL,
            operation: HelperOperation::RemovePackages {
                tool: "apt".into(),
                packages: vec!["vim".into()],
            },
        }
    }

    const DONE: &str = r#"{"outcome":"done","command":["apt-get","remove","--yes","vim"],"stdout":"","stderr":""}"#;

    fn fixture(uid: &str) -> Fixture {
        let fixture = Fixture::new();
        fixture.executable(HELPER, "").file(
            "/proc/self/status",
            format!("Name:\thousekeeper\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
        );
        fixture
    }

    #[test]
    fn a_user_reaches_the_helper_through_pkexec_and_root_directly() {
        let user = fixture("1000");
        let host = user.host(FakeRunner::new().respond(&format!("pkexec {HELPER}"), DONE));
        assert!(matches!(Pkexec.ask(&host, &request()), Ok(HelperReply::Done { .. })));

        let root = fixture("0");
        let host = root.host(FakeRunner::new().respond(HELPER, DONE));
        assert!(matches!(Pkexec.ask(&host, &request()), Ok(HelperReply::Done { .. })));
    }

    #[test]
    fn a_dismissed_password_prompt_or_a_missing_helper_is_said_plainly() {
        let user = fixture("1000");
        let dismissed = Output {
            code: Some(126),
            stdout: String::new(),
            stderr: "Error executing command as another user: Request dismissed\n".into(),
        };
        let host = user.host(FakeRunner::new().respond_with(&format!("pkexec {HELPER}"), dismissed));
        assert_eq!(
            Pkexec.ask(&host, &request()),
            Err("the administrator password was not given".to_string())
        );

        let bare = Fixture::new();
        bare.file("/proc/self/status", "Uid:\t1000\t1000\t1000\t1000\n");
        let reason = Pkexec.ask(&bare.host(FakeRunner::new()), &request()).unwrap_err();
        assert!(reason.contains("not installed"), "{reason}");
    }
}
