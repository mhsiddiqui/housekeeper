# 0046: Add the operation queue, which waits for package-manager locks

- **Date:** 2026-11-26
- **Branch / PR:** `feat/operation-queue`, #36
- **Plan chunks:** 4.2, done
- **Commits:** 05c62d5, eea29be

## What changed
- **New core module `queue.rs`** (decision D41).
  - Types: `Job` (a change, with its `backend` and `run`), `Queue` (`new`, `lock_timing`, `run`), `Cancel`, `Event`
    (`Waiting`, `Started`, `Finished`, `Cancelled`), `Outcome` (`Ran`, `Cancelled`, `LockTimeout`), `LockHolder` and the
    `PackageLock` ability.
  - `Queue::run` gives each backend one thread. That thread runs the backend's jobs in queue order; different backends
    run at once. Outcomes come back in queue order.
  - Before each job, the queue waits while the backend's lock is held, sending `Waiting` once per holder. It looks
    every second (`LOCK_POLL`) for at most 30 minutes (`LOCK_WAIT`).
  - `Cancel` skips jobs not yet started, including one waiting for a lock. A running job finishes.
- **`Platform::package_lock(backend)`**, empty by default.
  - `LinuxPlatform` returns the package tool's lock when `backend` is the tool's name in plans (`apt`, `dnf`…).
  - The Linux `PackageTool` trait gains `lock()`.
- **New Linux module `lock.rs`.** Locks are read, never taken, in three ways:
  - **`Locked`:** a `fcntl`/`flock` lock listed in `/proc/locks`, matched by inode. Waiter lines are skipped.
  - **`Exists`:** the lock file exists.
  - **`Pid`:** the pid in the file belongs to a running process.

  The holder's program comes from `/proc/<pid>/comm`. Each tool names its own locks:

  | Tool | Locks |
  |---|---|
  | apt | `/var/lib/dpkg/lock-frontend`, `/var/lib/dpkg/lock` |
  | dnf | the RPM database's `.rpm.lock` (`/usr/lib/sysimage/rpm`, `/var/lib/rpm`) |
  | zypper | `/run/zypp.pid`, then the RPM database's |
  | pacman | `/var/lib/pacman/db.lck` |
  | rpm-ostree | none: its daemon queues transactions itself |

- **Docs:** architecture (`PackageLock` and `Job` rows, new decision D41), D1–D41 in AGENTS.md, docs/README.md and the
  guide, README roadmap, and the plan (4.2 ✅). There is no CHANGELOG entry: nothing a user runs uses the queue until
  4.5.

## Why
Plan chunk 4.2 and decision D6: changes must not collide with each other or with a package manager already running
(unattended upgrades, a software centre). Without the queue, a change would fail on the lock instead of waiting and
naming who holds it.

Cancelling never stops a job that has started: killing apt or dnf mid-transaction can leave the package database
inconsistent.

## How it was verified
- **New tests:**
  - core `queue.rs`:
    - two apt jobs never overlap and run in order, while a flatpak job runs alongside the first; outcomes come back in
      queue order;
    - a lock held for three looks gives one `Waiting` naming its holder, then `Started` and `Finished`;
    - a lock that stays held ends in `LockTimeout` without running the job;
    - cancelling while the first job runs lets it finish and skips the second;
    - cancelling while waiting for a lock skips the job.
  - Linux `lock.rs`:
    - a `/proc/locks` entry with the lock file's real inode, on another device number, is held by its process (pid and
      `comm`), while a waiter on the same inode is not the holder;
    - a dpkg lock file with no `/proc/locks` entry is free;
    - pacman's lock counts while the file exists;
    - a pid file counts only while `/proc/<pid>` exists.
- **Local checks, all passed:**
  - `cargo fmt --all --check`
  - `rustfmt --check` on `sources/*.rs` and `tools/*.rs`
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  - `cargo test --workspace --all-features --locked` (8 + 46 + 2 + 151)
  - `cargo doc` with `-D warnings`
  - `cargo +1.85 check`
  - the Windows check of core and platform
  - `cargo deny` was not run (not installed); no dependency was added.
- **Real locks in containers, with a throwaway static program** (deleted afterwards) that queued one job for the
  system's tool:

  | Container | Lock held by | Holder reported | Result |
  |---|---|---|---|
  | Debian stable | a Perl `fcntl` lock on `/var/lib/dpkg/lock-frontend` (process renamed `unattended-upgr`) | `pid: 7, program: "unattended-upgr"` | waited, then ran when the lock was let go |
  | Fedora 45 (overlay file system) | `flock` on `/usr/lib/sysimage/rpm/.rpm.lock` | `pid: 10, program: "flock"` | waited, then ran |
  | Arch | a `db.lck` that existed for 1.5 s | `/var/lib/pacman/db.lck` | waited, then ran |
  | openSUSE Leap 16.1 | `zypper refresh` running | `/run/zypp.pid`, `pid: 7, program: "Zypp-main"` | `LockTimeout` after the 20 s limit the probe set |

  - On this Ubuntu machine, with nothing holding dpkg's lock, the job ran at once.

## Known limits and follow-ups
- **Inode-only matching.** An unrelated lock on a file with the same inode number on another file system would count as
  held, so the job waits until that lock goes or the wait runs out.
- **A stale `db.lck`** after a pacman crash is waited on until the limit. The front end (4.5) should say how to clear
  it.
- **dnf's own lock is not read.** dnf holds its own lock while it downloads metadata, before it takes the RPM lock.
  dnf removals run without repositories (D40), so they reach the RPM lock at once.
- **Nothing uses the queue yet;** `housekeeper remove` does in 4.5.
