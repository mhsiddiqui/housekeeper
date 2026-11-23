//! One file per way software gets installed on Linux.
//!
//! Every file here is a source and is registered automatically by `build.rs`: it must contain exactly one
//! `impl LinuxSource for <Type>` (see [`crate::provider`]). Shared helpers that are not sources live outside this folder.

include!(concat!(env!("OUT_DIR"), "/sources.rs"));

/// First non-empty line of a program's output.
pub(crate) fn version_line(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.to_string())
}
