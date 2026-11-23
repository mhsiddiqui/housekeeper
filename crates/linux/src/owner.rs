//! Which paths the native package manager installed, asked in one batched call.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use housekeeper_core::{Cmd, Host, Output, SourceError};

/// The distribution's own package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeManager {
    Dpkg,
    Rpm,
    Pacman,
}

impl NativeManager {
    /// The subset of `paths` that belong to an installed package. Only exact path matches count.
    pub fn owned_paths(self, host: &Host, paths: &[PathBuf]) -> Result<HashSet<PathBuf>, SourceError> {
        if paths.is_empty() {
            return Ok(HashSet::new());
        }
        let wanted: HashSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let cmd = self.command(paths);
        let output = host.run(&cmd)?;
        // Unowned paths make all three exit non-zero, so stdout is authoritative. dpkg-query exits 2 on real errors.
        let failed = match self {
            Self::Dpkg => !matches!(output.code, Some(0 | 1)),
            Self::Rpm | Self::Pacman => output.code.is_none(),
        };
        if failed {
            return Err(command_failed(&cmd, &output));
        }
        let owned = match self {
            Self::Dpkg => dpkg_owned(&output.stdout, &wanted),
            Self::Rpm => rpm_owned(&output.stdout, &wanted),
            Self::Pacman => pacman_owned(&output.stdout, &wanted),
        };
        Ok(owned)
    }

    fn command(self, paths: &[PathBuf]) -> Cmd {
        let paths = paths.iter().map(|p| p.to_string_lossy().into_owned());
        match self {
            Self::Dpkg => Cmd::new("dpkg-query")
                .arg("--search")
                .args(paths.map(|p| escape_glob(&p))),
            // The owning packages' file lists: unlike `%{NAME}`, they say which argument was owned even when an
            // argument has several owners or none (missing files print nothing on stdout).
            Self::Rpm => Cmd::new("rpm")
                .args(["-qf", "--queryformat", "[%{FILENAMES}\\n]"])
                .args(paths),
            Self::Pacman => Cmd::new("pacman").arg("-Qo").args(paths),
        }
    }
}

fn command_failed(cmd: &Cmd, output: &Output) -> SourceError {
    SourceError::CommandFailed {
        command: cmd.to_string(),
        code: output.code,
        stderr: output.stderr.trim().to_string(),
    }
}

/// dpkg-query treats its arguments as glob patterns; escape the wildcard characters so paths match literally.
fn escape_glob(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '*' | '?' | '[' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Lines are `pkg[, pkg2]: /path`, plus `diversion by pkg from: /path` and `diversion by pkg to: /path` for diverted
/// files, which say nothing about ownership.
fn dpkg_owned(stdout: &str, wanted: &HashSet<&Path>) -> HashSet<PathBuf> {
    stdout
        .lines()
        .filter(|line| !line.starts_with("diversion by "))
        .filter_map(|line| line.split_once(": ").map(|(_, path)| Path::new(path)))
        .filter(|path| wanted.contains(path))
        .map(Path::to_path_buf)
        .collect()
}

/// One file of an owning package per line; "file … is not owned by any package" lines never equal a path.
fn rpm_owned(stdout: &str, wanted: &HashSet<&Path>) -> HashSet<PathBuf> {
    stdout
        .lines()
        .map(Path::new)
        .filter(|path| wanted.contains(path))
        .map(Path::to_path_buf)
        .collect()
}

/// Lines are `/path is owned by pkg version`; directories get a trailing slash. Errors go to stderr.
fn pacman_owned(stdout: &str, wanted: &HashSet<&Path>) -> HashSet<PathBuf> {
    stdout
        .lines()
        .filter_map(|line| line.rsplit_once(" is owned by "))
        .map(|(path, _)| Path::new(path.trim_end_matches('/')))
        .filter(|path| wanted.contains(path))
        .map(Path::to_path_buf)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn sorted(set: HashSet<PathBuf>) -> Vec<PathBuf> {
        let mut list: Vec<_> = set.into_iter().collect();
        list.sort();
        list
    }

    fn exit(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            code: Some(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn dpkg_partial_matches_are_not_an_error() {
        let runner = FakeRunner::new().respond_with(
            "dpkg-query --search /opt/brave.com /opt/linear /usr/local/bin/ngrok",
            exit(
                1,
                "brave-browser: /opt/brave.com\nngrok: /usr/local/bin/ngrok\n",
                "dpkg-query: no path found matching pattern /opt/linear\n",
            ),
        );
        let host = Fixture::new().host(runner);
        let owned = NativeManager::Dpkg
            .owned_paths(
                &host,
                &paths(&["/opt/brave.com", "/opt/linear", "/usr/local/bin/ngrok"]),
            )
            .unwrap();
        assert_eq!(sorted(owned), paths(&["/opt/brave.com", "/usr/local/bin/ngrok"]));
    }

    #[test]
    fn dpkg_ignores_diversions_and_paths_it_was_not_asked_about() {
        let stdout = "\
diversion by dictionaries-common from: /usr/local/bin/words
diversion by dictionaries-common to: /usr/local/bin/words.pre
t3code, freelens, zoom: /opt
t3code: /opt/T3 Code (Beta)
";
        let wanted_paths = paths(&["/usr/local/bin/words", "/opt/T3 Code (Beta)"]);
        let wanted: HashSet<&Path> = wanted_paths.iter().map(PathBuf::as_path).collect();
        assert_eq!(sorted(dpkg_owned(stdout, &wanted)), paths(&["/opt/T3 Code (Beta)"]));
    }

    #[test]
    fn dpkg_patterns_are_escaped() {
        assert_eq!(escape_glob("/opt/a*b?[c]\\d"), "/opt/a\\*b\\?\\[c]\\\\d");
    }

    #[test]
    fn dpkg_fatal_errors_fail() {
        let runner = FakeRunner::new().respond_with(
            "dpkg-query --search /opt/x",
            exit(2, "", "dpkg-query: error: parsing file '/var/lib/dpkg/status'"),
        );
        let host = Fixture::new().host(runner);
        assert!(matches!(
            NativeManager::Dpkg.owned_paths(&host, &paths(&["/opt/x"])),
            Err(SourceError::CommandFailed { code: Some(2), .. })
        ));
    }

    #[test]
    fn rpm_matches_paths_in_owner_file_lists() {
        let stdout = "\
/opt/google
/opt/google/chrome
/opt/google/chrome/chrome
/usr/bin/google-chrome-stable
file /opt/manual is not owned by any package
";
        let runner = FakeRunner::new().respond_with(
            "rpm -qf --queryformat [%{FILENAMES}\\n] /opt/google /opt/manual /opt/gone",
            exit(2, stdout, "error: file /opt/gone: No such file or directory\n"),
        );
        let host = Fixture::new().host(runner);
        let owned = NativeManager::Rpm
            .owned_paths(&host, &paths(&["/opt/google", "/opt/manual", "/opt/gone"]))
            .unwrap();
        assert_eq!(sorted(owned), paths(&["/opt/google"]));
    }

    #[test]
    fn pacman_reads_owner_lines_and_directory_slashes() {
        let runner = FakeRunner::new().respond_with(
            "pacman -Qo /opt/visual-studio-code /usr/local/bin/tool /opt/manual",
            exit(
                1,
                "/opt/visual-studio-code/ is owned by visual-studio-code-bin 1.95.3-1\n\
                 /usr/local/bin/tool is owned by tool-git r12.abc-1\n",
                "error: No package owns /opt/manual\n",
            ),
        );
        let host = Fixture::new().host(runner);
        let owned = NativeManager::Pacman
            .owned_paths(
                &host,
                &paths(&["/opt/visual-studio-code", "/usr/local/bin/tool", "/opt/manual"]),
            )
            .unwrap();
        assert_eq!(
            sorted(owned),
            paths(&["/opt/visual-studio-code", "/usr/local/bin/tool"])
        );
    }

    #[test]
    fn nothing_to_ask_runs_nothing() {
        let host = Fixture::new().host(FakeRunner::new());
        assert!(NativeManager::Pacman.owned_paths(&host, &[]).unwrap().is_empty());
    }
}
