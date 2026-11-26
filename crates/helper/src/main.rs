//! `housekeeper-helper`, Housekeeper's root helper (decisions D4, D25).
//!
//! Started as root through `pkexec`, it reads one [`HelperRequest`] as JSON on standard input, checks it and carries it
//! out with [`carry_out`], and prints one [`HelperReply`] as JSON on standard output. Exit code: 0 done, 1 the package
//! tool failed, 2 refused (nothing was changed). It never scans, never reads a home folder, and runs programs only from
//! the system's own folders.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::sync::Arc;

use housekeeper_core::{HelperReply, HelperRequest, Host, SystemRunner, carry_out};

/// Where the programs the helper runs come from, whatever `PATH` it was started with.
const PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

/// A request is a few hundred bytes; anything larger is not one.
const MAX_REQUEST: u64 = 64 * 1024;

fn main() -> ExitCode {
    let reply = reply(io::stdin().lock());
    let code = reply.exit_code();
    let mut out = io::stdout().lock();
    // With nobody reading the reply there is no one to tell, and the exit code still says what happened.
    let _ = serde_json::to_writer(&mut out, &reply)
        .map_err(io::Error::from)
        .and_then(|()| writeln!(out));
    ExitCode::from(code)
}

fn reply(input: impl Read) -> HelperReply {
    let vars = HashMap::from([("PATH".to_string(), PATH.to_string())]);
    let host = Host::new("/", "/root", vars, Arc::new(SystemRunner::new(Some(PATH))));
    if !runs_as_root(&host) {
        return HelperReply::refused("housekeeper-helper only runs as root (Housekeeper starts it through pkexec)");
    }
    let request = match read_request(input) {
        Ok(request) => request,
        Err(reason) => return HelperReply::refused(reason),
    };
    match housekeeper_platform::detect(&host) {
        Ok(platform) => carry_out(platform.as_ref(), &host, &request),
        Err(e) => HelperReply::refused(e.to_string()),
    }
}

/// The request on `input`: one JSON document of at most [`MAX_REQUEST`] bytes, with no field the format lacks.
fn read_request(input: impl Read) -> Result<HelperRequest, String> {
    let mut bytes = Vec::new();
    input
        .take(MAX_REQUEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("could not read the request: {e}"))?;
    if bytes.len() as u64 > MAX_REQUEST {
        return Err(format!("the request is larger than {MAX_REQUEST} bytes"));
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("not a valid request: {e}"))
}

/// The process's effective user is root, from the kernel's own account of it (`/proc/self/status`).
fn runs_as_root(host: &Host) -> bool {
    host.read_to_string("/proc/self/status")
        .is_ok_and(|status| effective_uid(&status) == Some("0"))
}

/// The effective user ID in a `/proc/<pid>/status`: the second number of its `Uid:` line (real, effective, saved, file
/// system).
fn effective_uid(status: &str) -> Option<&str> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().nth(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::HelperOperation;

    #[test]
    fn a_request_is_one_known_operation_with_only_its_own_fields() {
        let request = read_request(
            br#"{"protocol":1,"operation":{"kind":"remove-packages","tool":"apt","packages":["vim"]}}"#.as_slice(),
        )
        .unwrap();
        assert_eq!(
            request.operation,
            HelperOperation::RemovePackages {
                tool: "apt".into(),
                packages: vec!["vim".into()],
            }
        );
        for bad in [
            // A command line smuggled in next to the names the helper checks.
            r#"{"protocol":1,"operation":{"kind":"remove-packages","tool":"apt","packages":["vim"],"argv":["rm"]}}"#,
            r#"{"protocol":1,"operation":{"kind":"run","argv":["rm","-rf","/"]}}"#,
            r#"{"protocol":1,"operation":{"kind":"remove-packages","tool":"apt","packages":["vim"]},"as":"root"}"#,
            "",
        ] {
            assert!(read_request(bad.as_bytes()).is_err(), "{bad}");
        }
        let huge = format!("{{\"protocol\":1,\"pad\":\"{}\"}}", "x".repeat(MAX_REQUEST as usize));
        assert!(read_request(huge.as_bytes()).unwrap_err().contains("larger than"));
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
