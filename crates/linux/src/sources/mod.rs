//! One module per way software gets installed on Linux.

pub mod dpkg;
pub mod flatpak;
pub mod pacman;
pub mod rpm;
pub mod snap;

/// First non-empty line of a program's output.
pub(crate) fn version_line(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.to_string())
}
