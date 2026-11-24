//! One file per way software gets installed on Linux.
//!
//! Every file here is a source and is registered automatically by `build.rs`, which declares it as a module and lists
//! its `PROVIDER`. Each file must call `register!(<Type>);` for its `impl LinuxSource for <Type>` (see
//! [`crate::provider`]). Sources never import from each other: shared code lives outside this folder, except
//! `version_line` below.

include!(concat!(env!("OUT_DIR"), "/sources.rs"));

/// First non-empty line of a program's output.
pub(crate) fn version_line(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.to_string())
}
