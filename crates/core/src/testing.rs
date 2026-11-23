//! Test doubles for source implementations (feature `test-support`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::command::{Cmd, CommandRunner, Output, RunError};
use crate::host::Host;

/// Home directory of the fixture user.
pub const HOME: &str = "/home/user";

/// Answers commands from a script. Unscripted commands panic, so a test notices any command it did not expect.
#[derive(Default)]
pub struct FakeRunner {
    programs: HashMap<String, PathBuf>,
    responses: HashMap<String, Result<Output, String>>,
    calls: Mutex<Vec<String>>,
}

impl FakeRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a program as installed at `/usr/bin/<name>`.
    pub fn program(mut self, name: &str) -> Self {
        self.programs.insert(name.to_string(), Path::new("/usr/bin").join(name));
        self
    }

    /// `command` is the space-joined command line (`Cmd`'s `Display`). Also declares the program installed.
    pub fn respond(self, command: &str, stdout: &str) -> Self {
        self.respond_with(
            command,
            Output {
                code: Some(0),
                stdout: stdout.to_string(),
                stderr: String::new(),
            },
        )
    }

    pub fn respond_with(mut self, command: &str, output: Output) -> Self {
        let program = command.split(' ').next().unwrap_or_default();
        self = self.program(program);
        self.responses.insert(command.to_string(), Ok(output));
        self
    }

    /// The command fails to run at all (for example it times out).
    pub fn fail(mut self, command: &str, message: &str) -> Self {
        let program = command.split(' ').next().unwrap_or_default();
        self = self.program(program);
        self.responses.insert(command.to_string(), Err(message.to_string()));
        self
    }

    /// Command lines run so far.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, cmd: &Cmd) -> Result<Output, RunError> {
        let line = cmd.to_string();
        self.calls.lock().unwrap().push(line.clone());
        if !self.programs.contains_key(&cmd.program) {
            return Err(RunError::NotFound(cmd.program.clone()));
        }
        match self.responses.get(&line) {
            Some(Ok(output)) => Ok(output.clone()),
            Some(Err(message)) => Err(RunError::Io {
                program: cmd.program.clone(),
                source: std::io::Error::other(message.clone()),
            }),
            None => panic!("FakeRunner: unexpected command `{line}`"),
        }
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.programs.get(name).cloned()
    }
}

/// A throwaway root file system. Paths given to its methods are logical absolute paths.
pub struct Fixture {
    dir: tempfile::TempDir,
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("create fixture directory"),
        }
    }

    fn real(&self, logical: &str) -> PathBuf {
        self.dir.path().join(logical.trim_start_matches('/'))
    }

    pub fn file(&self, path: &str, content: impl AsRef<[u8]>) -> &Self {
        let real = self.real(path);
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(real, content).unwrap();
        self
    }

    pub fn executable(&self, path: &str, content: impl AsRef<[u8]>) -> &Self {
        use std::os::unix::fs::PermissionsExt;
        self.file(path, content);
        fs::set_permissions(self.real(path), fs::Permissions::from_mode(0o755)).unwrap();
        self
    }

    pub fn dir(&self, path: &str) -> &Self {
        fs::create_dir_all(self.real(path)).unwrap();
        self
    }

    /// `target` is written verbatim; absolute targets are logical and resolved under the fixture root.
    pub fn symlink(&self, path: &str, target: &str) -> &Self {
        let real = self.real(path);
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, real).unwrap();
        self
    }

    /// A host rooted here, user home at [`HOME`], no environment variables besides `HOME`.
    pub fn host(&self, runner: FakeRunner) -> Host {
        self.host_with_vars(runner, &[])
    }

    pub fn host_with_vars(&self, runner: FakeRunner, vars: &[(&str, &str)]) -> Host {
        let mut env: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        env.insert("HOME".into(), HOME.into());
        Host::new(self.dir.path(), HOME, env, Arc::new(runner))
    }
}
