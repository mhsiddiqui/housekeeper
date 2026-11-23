//! One module per way software gets installed on Linux.

pub mod cargo;
pub mod dpkg;
pub mod flatpak;
pub mod npm;
pub mod pacman;
pub mod pip;
pub mod pipx;
pub mod rpm;
pub mod snap;
pub mod uv;

/// First non-empty line of a program's output.
pub(crate) fn version_line(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.to_string())
}
