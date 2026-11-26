//! Telling whether another program holds a package tool's lock, without taking it (decision D6): the queue waits
//! while one does. Package tools name their lock files; this reads them the way each tool uses them.

use std::path::Path;

use housekeeper_core::{Host, LockHolder};

/// One lock file and how its tool holds it.
pub(crate) enum LockFile {
    /// Held while a process has a `fcntl`/`flock` lock on the file, as the kernel lists in `/proc/locks` (dpkg and
    /// apt, the RPM database).
    Locked(&'static str),
    /// Held while the file exists (pacman's `db.lck`; a crashed pacman leaves it behind until someone removes it).
    Exists(&'static str),
    /// Held while the process whose ID the file holds is running (zypper's `zypp.pid`).
    Pid(&'static str),
}

/// The first of `locks` someone holds, and who.
pub(crate) fn holder(host: &Host, locks: &[LockFile]) -> Option<LockHolder> {
    locks.iter().find_map(|lock| match *lock {
        LockFile::Locked(path) => locked_by(host, path),
        LockFile::Exists(path) => host.metadata(path).map(|_| LockHolder {
            lock: path.into(),
            pid: None,
            program: None,
        }),
        LockFile::Pid(path) => {
            let pid: u32 = host.read_to_string(path).ok()?.trim().parse().ok()?;
            running(host, pid).then(|| holder_process(host, path, pid))
        }
    })
}

/// The process that holds a lock on `path`, from `/proc/locks`. Matched by inode only: on btrfs and overlay file
/// systems the device `stat` reports is not the one `/proc/locks` prints.
fn locked_by(host: &Host, path: &str) -> Option<LockHolder> {
    let inode = inode(host, Path::new(path))?;
    let locks = host.read_to_string("/proc/locks").ok()?;
    let pid = locks.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // `1: POSIX  ADVISORY  WRITE 9580 103:08:14312025 0 EOF`; waiters (`1: -> POSIX …`) hold nothing.
        if fields.get(1) == Some(&"->") {
            return None;
        }
        let file_inode = fields.get(5)?.rsplit(':').next()?.parse::<u64>().ok()?;
        (file_inode == inode).then(|| fields[4].parse::<i64>().ok())
    })?;
    // Open-file-description locks belong to no process (`-1`).
    match pid.and_then(|pid| u32::try_from(pid).ok()) {
        Some(pid) => Some(holder_process(host, path, pid)),
        None => Some(LockHolder {
            lock: path.into(),
            pid: None,
            program: None,
        }),
    }
}

#[cfg(unix)]
fn inode(host: &Host, path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    host.metadata(path).map(|meta| meta.ino())
}

#[cfg(not(unix))]
fn inode(_: &Host, _: &Path) -> Option<u64> {
    None
}

fn running(host: &Host, pid: u32) -> bool {
    host.metadata(format!("/proc/{pid}")).is_some()
}

fn holder_process(host: &Host, path: &str, pid: u32) -> LockHolder {
    LockHolder {
        lock: path.into(),
        pid: Some(pid),
        program: host
            .read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture};

    const DPKG: &str = "/var/lib/dpkg/lock-frontend";

    /// `/proc/locks` with a lock by `pid` on the fixture file at `path` (its real inode), after unrelated ones.
    fn proc_locks(fixture: &Fixture, host: &Host, path: &str, pid: &str) {
        let inode = inode(host, Path::new(path)).unwrap();
        fixture.file(
            "/proc/locks",
            format!(
                "1: POSIX  ADVISORY  WRITE 4242 103:08:1 0 EOF\n\
                 2: -> POSIX  ADVISORY  WRITE 777 103:08:{inode} 0 EOF\n\
                 3: POSIX  ADVISORY  WRITE {pid} 00:2f:{inode} 0 EOF\n"
            ),
        );
    }

    #[test]
    fn a_lock_the_kernel_lists_is_held_by_its_process() {
        let fixture = Fixture::new();
        fixture.file(DPKG, "").file("/proc/812/comm", "unattended-upgr\n");
        let host = fixture.host(FakeRunner::new());
        assert_eq!(holder(&host, &[LockFile::Locked(DPKG)]), None, "no /proc/locks entry");
        proc_locks(&fixture, &host, DPKG, "812");
        assert_eq!(
            holder(&host, &[LockFile::Locked(DPKG)]),
            Some(LockHolder {
                lock: DPKG.into(),
                pid: Some(812),
                program: Some("unattended-upgr".into()),
            }),
            "matched by inode, whatever the device; the waiter (777) is not the holder"
        );
    }

    #[test]
    fn a_lock_file_alone_is_not_a_held_lock_unless_its_tool_says_so() {
        let fixture = Fixture::new();
        fixture.file(DPKG, "").file("/var/lib/pacman/db.lck", "");
        fixture.file("/proc/locks", "");
        let host = fixture.host(FakeRunner::new());
        assert_eq!(
            holder(&host, &[LockFile::Locked(DPKG)]),
            None,
            "dpkg leaves its lock file in place"
        );
        assert!(holder(&host, &[LockFile::Exists("/var/lib/pacman/db.lck")]).is_some());
        assert_eq!(holder(&host, &[LockFile::Exists("/var/lib/pacman/missing.lck")]), None);
    }

    #[test]
    fn a_pid_file_is_held_only_while_its_process_runs() {
        let fixture = Fixture::new();
        fixture.file("/run/zypp.pid", "4321\n");
        let host = fixture.host(FakeRunner::new());
        assert_eq!(
            holder(&host, &[LockFile::Pid("/run/zypp.pid")]),
            None,
            "a stale pid file"
        );
        fixture.file("/proc/4321/comm", "zypper\n");
        assert_eq!(
            holder(&host, &[LockFile::Pid("/run/zypp.pid")]).and_then(|h| h.program),
            Some("zypper".to_string())
        );
    }
}
