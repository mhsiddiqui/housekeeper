//! Text output of `housekeeper show`.

use std::io::{self, Write};

use housekeeper_core::{Details, FilesFrom, Package, PackageKind, Shared};

use crate::table::{Table, human_size};
use crate::who::circumstances;

/// Width of the labels, so values line up.
const LABEL: usize = 12;

/// How many packages `Needed by` names before it only counts the rest.
const NEEDED_BY_SHOWN: usize = 10;

/// The item, then one labelled line per thing known about it. `all_files` lists every file after the summary.
pub fn render(details: &Details, all_files: bool, out: &mut impl Write) -> io::Result<()> {
    let package = &details.package;
    writeln!(out, "{}", package.display_name())?;
    writeln!(out, "{}", package.key())?;
    writeln!(out)?;
    // A label of "" continues the previous one.
    let mut line = |label: &str, value: &str| {
        let label = if label.is_empty() {
            String::new()
        } else {
            format!("{label}:")
        };
        writeln!(out, "{label:<LABEL$}{value}")
    };

    if let Some(summary) = &package.summary {
        line("Summary", summary)?;
    }
    if let Some(version) = &package.version {
        line("Version", version)?;
    }
    line("Source", &details.source.label)?;
    if let Some(evidence) = &details.source.evidence {
        line("Known from", evidence)?;
    }
    line("Installed", &circumstances(package))?;
    line("Kind", kind(package.kind))?;
    if let Some(size) = package.size_bytes {
        line("Size", &human_size(size))?;
    }
    if let Some(origin) = &package.origin {
        line("Origin", origin)?;
    }
    if let Some(location) = &package.location {
        line("Location", &location.display().to_string())?;
    }
    if package.protected {
        line("Protected", "yes: removing it could break the system")?;
    }
    if !package.commands.is_empty() {
        line("Commands", &package.commands.join(", "))?;
    }
    for (n, launcher) in package.launchers.iter().enumerate() {
        let hidden = if launcher.hidden { ", not shown in menus" } else { "" };
        let text = format!("{} ({}{hidden})", launcher.name, launcher.id);
        line(if n == 0 { "Launchers" } else { "" }, &text)?;
    }
    if !package.app_ids.is_empty() {
        line("AppStream", &package.app_ids.join(", "))?;
    }
    for (n, other) in details.also_installed.iter().enumerate() {
        line(if n == 0 { "Also as" } else { "" }, &entry(other))?;
    }
    if !details.shared.is_empty() {
        line("Same app", &format!("shares {}", shared(&details.shared)))?;
    }
    match &details.files {
        Some(files) => {
            let mut text = format!(
                "{}, {}",
                crate::count(files.count, "file"),
                human_size(files.size_bytes)
            );
            match &files.from {
                FilesFrom::Recorded => text.push_str(", as the source records them"),
                FilesFrom::Location { path } => text.push_str(&format!(", everything in {}", path.display())),
            }
            if files.missing > 0 {
                text.push_str(&format!("; {} listed but missing", files.missing));
            }
            if !all_files {
                text.push_str(" (--files lists them)");
            }
            line("Files", &text)?;
        }
        None => line("Files", "not known")?,
    }
    match &details.needed_by {
        Some(needed) if needed.is_empty() => line("Needed by", "nothing installed")?,
        Some(needed) => {
            for (n, package) in needed.iter().take(NEEDED_BY_SHOWN).enumerate() {
                line(if n == 0 { "Needed by" } else { "" }, &entry(package))?;
            }
            if needed.len() > NEEDED_BY_SHOWN {
                let more = needed.len() - NEEDED_BY_SHOWN;
                line("", &format!("… and {more} more (--json lists every one)"))?;
            }
        }
        None => line("Needed by", "not known")?,
    }

    if all_files {
        if let Some(files) = &details.files {
            writeln!(out)?;
            for path in &files.paths {
                writeln!(out, "{}", path.display())?;
            }
        }
    }
    Ok(())
}

/// The installed items a name matched, to pick one by `source:id`.
pub fn render_several(packages: &[Package], out: &mut impl Write) -> io::Result<()> {
    let mut table = Table::new(["ID", "NAME", "VERSION", "SOURCE"]);
    for package in packages {
        table.row([
            package.key(),
            package.display_name().to_string(),
            package.version.clone().unwrap_or_default(),
            package.source.to_string(),
        ]);
    }
    table.write(out)
}

fn kind(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::App => "app",
        PackageKind::Runtime => "runtime other apps need",
        PackageKind::Library => "library or development files",
        PackageKind::Component => "component (no command and no menu entry)",
        PackageKind::Unknown => "unknown",
    }
}

/// `Sublime Merge 2136 (snap:sublime-merge)`.
fn entry(package: &Package) -> String {
    let version = package.version.as_deref().map(|v| format!(" {v}")).unwrap_or_default();
    format!("{}{version} ({})", package.display_name(), package.key())
}

/// `the name sublimemerge and the command smerge`.
fn shared(shared: &[Shared]) -> String {
    let parts: Vec<String> = shared
        .iter()
        .map(|item| match item {
            Shared::AppId(id) => format!("the AppStream ID {id}"),
            Shared::Name(name) => format!("the name {name}"),
            Shared::Command(command) => format!("the command {command}"),
        })
        .collect();
    match parts.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::{Files, InstallReason, Scope, SourceId, SourceInfo, SourceKind};
    use std::path::PathBuf;

    #[test]
    fn every_known_fact_gets_a_line_and_long_dependant_lists_are_cut() {
        let mut libc = Package::new(SourceId::new("dpkg"), "libc6:amd64", "libc6", Scope::System);
        libc.version = Some("2.42-2ubuntu2".into());
        libc.reason = InstallReason::System;
        libc.kind = PackageKind::Library;
        let needed_by: Vec<Package> = (0..12)
            .map(|i| {
                Package::new(
                    SourceId::new("dpkg"),
                    format!("app{i}"),
                    format!("app{i}"),
                    Scope::System,
                )
            })
            .collect();
        let details = Details {
            package: libc,
            source: SourceInfo::new("dpkg", "Debian packages", SourceKind::System).evidence("dpkg's database"),
            also_installed: Vec::new(),
            shared: Vec::new(),
            files: Some(Files {
                from: FilesFrom::Recorded,
                paths: vec![
                    PathBuf::from("/usr/lib/libc.so.6"),
                    PathBuf::from("/usr/share/doc/libc6"),
                ],
                count: 1,
                missing: 0,
                size_bytes: 2_000_000,
            }),
            needed_by: Some(needed_by),
            warnings: Vec::new(),
        };
        let mut out = Vec::new();
        render(&details, true, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let expected = "\
libc6
dpkg:libc6:amd64

Version:    2.42-2ubuntu2
Source:     Debian packages
Known from: dpkg's database
Installed:  part of the system, for all users
Kind:       library or development files
Files:      1 file, 2.0 MB, as the source records them
Needed by:  app0 (dpkg:app0)
            app1 (dpkg:app1)
            app2 (dpkg:app2)
            app3 (dpkg:app3)
            app4 (dpkg:app4)
            app5 (dpkg:app5)
            app6 (dpkg:app6)
            app7 (dpkg:app7)
            app8 (dpkg:app8)
            app9 (dpkg:app9)
            … and 2 more (--json lists every one)

/usr/lib/libc.so.6
/usr/share/doc/libc6
";
        assert_eq!(text, expected);
    }
}
