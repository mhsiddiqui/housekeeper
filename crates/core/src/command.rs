//! Running external programs: always an argv vector, never a shell, always with a timeout.

use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Default limit for read-only queries. Package managers waiting on a lock must not hang a scan.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// A program invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Display for Cmd {
    /// Space-joined command line, for messages and test matching. Not shell-quoted.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// Result of a finished program.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    /// Exit code; `None` when killed by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("{0} is not installed")]
    NotFound(String),
    #[error("{program} did not finish within {} s", .after.as_secs())]
    Timeout { program: String, after: Duration },
    #[error("could not run {program}: {source}")]
    Io {
        program: String,
        #[source]
        source: io::Error,
    },
}

/// Executes programs. Swapped for a fake in tests and, later, for an SSH runner.
pub trait CommandRunner: Send + Sync {
    fn run(&self, cmd: &Cmd) -> Result<Output, RunError>;

    /// Full path of a program, if it is installed.
    fn find_program(&self, name: &str) -> Option<PathBuf>;
}

/// Runs programs on the local machine.
#[derive(Debug, Clone)]
pub struct SystemRunner {
    search_path: Vec<PathBuf>,
}

impl SystemRunner {
    /// Searches `path` (a `PATH`-style list) and then the standard system directories, which a desktop session's
    /// `PATH` sometimes lacks.
    pub fn new(path: Option<&str>) -> Self {
        let mut search_path: Vec<PathBuf> = path
            .unwrap_or_default()
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .collect();
        for dir in [
            "/usr/local/sbin",
            "/usr/local/bin",
            "/usr/sbin",
            "/usr/bin",
            "/sbin",
            "/bin",
        ] {
            let dir = PathBuf::from(dir);
            if !search_path.contains(&dir) {
                search_path.push(dir);
            }
        }
        Self { search_path }
    }
}

impl CommandRunner for SystemRunner {
    fn run(&self, cmd: &Cmd) -> Result<Output, RunError> {
        let program = self
            .find_program(&cmd.program)
            .ok_or_else(|| RunError::NotFound(cmd.program.clone()))?;
        let io_err = |source| RunError::Io {
            program: cmd.program.clone(),
            source,
        };
        let mut child = Command::new(program)
            .args(&cmd.args)
            // Untranslated, stable output for parsing.
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(io_err)?;

        // Drain both pipes concurrently so a chatty program cannot block on a full pipe.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let status = match wait_with_deadline(&mut child, cmd.timeout).map_err(io_err)? {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Timeout {
                    program: cmd.program.clone(),
                    after: cmd.timeout,
                });
            }
        };
        Ok(Output {
            code: status.code(),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        if name.contains('/') {
            let path = Path::new(name);
            return is_executable(path).then(|| path.to_path_buf());
        }
        self.search_path
            .iter()
            .map(|dir| dir.join(name))
            .find(|candidate| is_executable(candidate))
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn wait_with_deadline(child: &mut Child, timeout: Duration) -> io::Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_exit_code() {
        let runner = SystemRunner::new(None);
        let out = runner
            .run(&Cmd::new("sh").args(["-c", "echo out; echo err >&2; exit 3"]))
            .unwrap();
        assert_eq!(out.code, Some(3));
        assert_eq!(out.stdout, "out\n");
        assert_eq!(out.stderr, "err\n");
    }

    #[test]
    fn kills_programs_that_overrun_the_timeout() {
        let runner = SystemRunner::new(None);
        let started = Instant::now();
        let err = runner
            .run(&Cmd::new("sleep").arg("5").timeout(Duration::from_millis(100)))
            .unwrap_err();
        assert!(matches!(err, RunError::Timeout { .. }));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn missing_program_is_reported_as_not_found() {
        let runner = SystemRunner::new(Some("/nonexistent"));
        let err = runner.run(&Cmd::new("housekeeper-no-such-program")).unwrap_err();
        assert!(matches!(err, RunError::NotFound(name) if name == "housekeeper-no-such-program"));
    }
}
