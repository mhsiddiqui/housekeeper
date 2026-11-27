//! Text output of `housekeeper show` and `housekeeper remove`.

use std::io::{self, Write};

use housekeeper_core::history::{HistoryEntry, StepOutcome};
use housekeeper_core::{
    Action, Actor, Details, FilesFrom, Leftover, LeftoverKind, Leftovers, Package, PackageKind, RemovalPlan, Shared,
    Step, Tier,
};

use crate::table::{Table, human_size};
use crate::who::circumstances;

/// Width of the labels, so values line up.
const LABEL: usize = 12;

/// How many packages `Needed by` names before it only counts the rest.
const NEEDED_BY_SHOWN: usize = 10;

/// How many other packages a simulation names before it only counts them (`--json` lists every one).
pub const SIMULATED_SHOWN: usize = 10;

/// The item, then one labelled line per thing known about it. `all_files` lists every file after the summary.
pub fn render(details: &Details, all_files: bool, out: &mut impl Write) -> io::Result<()> {
    let package = &details.package;
    writeln!(out, "{}", package.display_name())?;
    writeln!(out, "{}", package.key())?;
    writeln!(out)?;
    // A label of "" continues the previous one.
    let mut line = |label: &str, value: &str| labelled(out, label, value);

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
    if let Some(why) = &package.protected {
        line("Protected", &format!("yes: {why}"))?;
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
    match &details.removal {
        Some(plan) => {
            for (n, text) in removal(plan).iter().enumerate() {
                line(if n == 0 { "Removal" } else { "" }, text)?;
            }
        }
        None => line("Removal", "could not be planned")?,
    }
    line("Leftovers", &leftovers_summary(&details.leftovers))?;

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

/// `housekeeper remove`: how the item would be removed and everything it would leave behind, in full; with `dry_run`
/// it ends by saying nothing was changed.
pub fn render_removal(details: &Details, dry_run: bool, out: &mut impl Write) -> io::Result<()> {
    let package = &details.package;
    writeln!(out, "{}", package.display_name())?;
    writeln!(out, "{}", package.key())?;
    writeln!(out)?;
    match &details.removal {
        Some(plan) => {
            for (n, text) in removal(plan).iter().enumerate() {
                labelled(out, if n == 0 { "Removal" } else { "" }, text)?;
            }
        }
        None => labelled(out, "Removal", "could not be planned")?,
    }
    let leftovers = &details.leftovers;
    if leftovers.folders.is_empty() {
        labelled(out, "Leftovers", "no folders named after it")?;
    }
    for (n, folder) in leftovers.folders.iter().enumerate() {
        labelled(out, if n == 0 { "Leftovers" } else { "" }, &leftover(folder))?;
    }
    for (n, shell) in leftovers.shell_lines.iter().enumerate() {
        let text = format!("{} line {}: {}", shell.file.display(), shell.line, shell.text);
        labelled(out, if n == 0 { "Shell" } else { "" }, &text)?;
    }
    if !leftovers.shell_lines.is_empty() {
        labelled(
            out,
            "",
            "(these lines are not edited yet; remove them by hand after removing it)",
        )?;
    }
    if dry_run {
        writeln!(out)?;
        writeln!(out, "Nothing was changed: this is a dry run.")?;
    }
    Ok(())
}

/// How each step of a removal went.
pub fn render_outcome(entry: &HistoryEntry, out: &mut impl Write) -> io::Result<()> {
    writeln!(out)?;
    for record in &entry.steps {
        let what = step_text(&record.step);
        match &record.outcome {
            StepOutcome::Done => writeln!(out, "done:     {what}")?,
            StepOutcome::Failed { reason } => writeln!(out, "failed:   {what}\n          {reason}")?,
            StepOutcome::Skipped { reason } => writeln!(out, "not done: {what}\n          {reason}")?,
        }
    }
    Ok(())
}

/// `as root, by Housekeeper's helper: apt-get remove --yes vim`.
pub fn step_text(step: &Step) -> String {
    let by = match step.by {
        Actor::User => "as you",
        Actor::Helper => "as root, by Housekeeper's helper",
    };
    let what = match &step.action {
        Action::Run { argv } | Action::RemovePackages { argv, .. } | Action::InstallPackageFiles { argv, .. } => {
            command_line(argv)
        }
        Action::Trash { path } => format!("move {} to the Trash", path.display()),
    };
    format!("{by}: {what}")
}

/// `label:` padded so values line up; a label of "" continues the previous one.
fn labelled(out: &mut impl Write, label: &str, value: &str) -> io::Result<()> {
    let label = if label.is_empty() {
        String::new()
    } else {
        format!("{label}:")
    };
    writeln!(out, "{label:<LABEL$}{value}")
}

/// `3 folders and 2 lines in shell startup files (housekeeper remove --dry-run measures and lists them)`. Not measured
/// here: a large folder takes seconds to walk.
fn leftovers_summary(leftovers: &Leftovers) -> String {
    let mut parts = Vec::new();
    if !leftovers.folders.is_empty() {
        parts.push(crate::count(leftovers.folders.len(), "folder"));
    }
    if !leftovers.shell_lines.is_empty() {
        parts.push(format!(
            "{} in shell startup files",
            crate::count(leftovers.shell_lines.len(), "line")
        ));
    }
    if parts.is_empty() {
        return "none found".to_string();
    }
    format!(
        "{} (`housekeeper remove --dry-run` measures and lists them)",
        parts.join(" and ")
    )
}

/// `/home/u/.config/foo (settings, 2.1 MB; yours: kept unless you choose it; snap:foo may still use it)`; a folder
/// measured only in part is "at least" its size. Whether it goes unless the user says otherwise follows D38.
fn leftover(folder: &Leftover) -> String {
    let kind = match folder.kind {
        LeftoverKind::Config => "settings",
        LeftoverKind::Cache => "cache",
        LeftoverKind::Data => "data",
        LeftoverKind::State => "history and state",
    };
    let mut notes = vec![match folder.size_bytes {
        Some(size) if folder.size_complete => format!("{kind}, {}", human_size(size)),
        Some(size) => format!(
            "{kind}, at least {}: too many files to count them all",
            human_size(size)
        ),
        None => kind.to_string(),
    }];
    if folder.preselected() {
        notes.push("goes to the Trash with it".into());
    } else if folder.kind.is_user_data() {
        notes.push("yours: kept unless you choose it".into());
    } else {
        notes.push("kept unless you choose it".into());
    }
    if !folder.also_used_by.is_empty() {
        notes.push(format!("{} may still use it", folder.also_used_by.join(", ")));
    }
    if folder.unseen_copies {
        notes.push("copies in projects or virtual environments Housekeeper does not read may still use it".into());
    }
    format!("{} ({})", folder.path.display(), notes.join("; "))
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

/// The plan's tier, each step, what the package manager says it takes, then what stands in the way. Nothing is
/// removed yet; this only says how.
fn removal(plan: &RemovalPlan) -> Vec<String> {
    let tier = match plan.tier {
        Tier::OwnUninstaller => "with its own uninstaller",
        Tier::PackageManager => "through its package manager",
        Tier::FormatConvention => "as its format's convention says (the file and what was made for it)",
        Tier::Recipe => "with a reviewed recipe",
        Tier::Manifest => "from the record made when it was installed",
        Tier::Unknown => "not known: nothing records how it was installed",
    };
    let mut lines = vec![tier.to_string()];
    lines.extend(plan.steps.iter().map(step_text));
    // A refusal is listed with the blockers; otherwise the item comes first and the rest is what goes with it.
    if let Some(simulation) = plan.simulation.as_ref().filter(|s| s.refused.is_empty()) {
        let others: Vec<&str> = simulation.removes.iter().skip(1).map(String::as_str).collect();
        let text = if others.is_empty() {
            format!("removes nothing else, says {}", simulation.by)
        } else {
            let mut names: Vec<&str> = others.iter().copied().take(SIMULATED_SHOWN).collect();
            if others.len() > SIMULATED_SHOWN {
                names.push("…");
            }
            format!(
                "also removes {}: {}, says {}",
                crate::count(others.len(), "package"),
                names.join(", "),
                simulation.by
            )
        };
        lines.push(text);
    }
    if plan.restart {
        lines.push("takes effect after a restart".into());
    }
    lines.extend(plan.blockers.iter().map(|blocker| format!("blocked: {blocker}")));
    lines
}

/// `argv` as one line, quoting arguments that are empty or hold spaces or quotes so the line reads unambiguously.
fn command_line(argv: &[String]) -> String {
    let quoted: Vec<String> = argv
        .iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"') {
                format!("'{}'", arg.replace('\'', r"'\''"))
            } else {
                arg.clone()
            }
        })
        .collect();
    quoted.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::{Files, InstallReason, Scope, ShellLine, Simulation, SourceId, SourceInfo, SourceKind};
    use std::path::PathBuf;

    fn leftovers() -> Leftovers {
        Leftovers {
            folders: vec![
                Leftover {
                    path: PathBuf::from("/home/u/.cache/tool"),
                    kind: LeftoverKind::Cache,
                    size_bytes: Some(1_500_000),
                    size_complete: false,
                    also_used_by: Vec::new(),
                    unseen_copies: true,
                    identity: None,
                },
                Leftover {
                    path: PathBuf::from("/home/u/.config/tool"),
                    kind: LeftoverKind::Config,
                    size_bytes: Some(500_000),
                    size_complete: true,
                    also_used_by: vec!["snap:tool".into()],
                    unseen_copies: false,
                    identity: None,
                },
                Leftover {
                    path: PathBuf::from("/home/u/.cache/tool-thumbnails"),
                    kind: LeftoverKind::Cache,
                    size_bytes: Some(2_000),
                    size_complete: true,
                    also_used_by: Vec::new(),
                    unseen_copies: false,
                    identity: None,
                },
            ],
            shell_lines: vec![ShellLine {
                file: PathBuf::from("/home/u/.bashrc"),
                line: 12,
                text: "export PATH=\"$HOME/.tool/bin:$PATH\"".into(),
            }],
        }
    }

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
            removal: Some(RemovalPlan {
                simulation: Some(Simulation {
                    by: "`apt-get --simulate remove`".into(),
                    removes: std::iter::once("libc6:amd64".to_string())
                        .chain((0..12).map(|i| format!("app{i}")))
                        .collect(),
                    refused: Vec::new(),
                }),
                ..RemovalPlan::new(
                    Tier::PackageManager,
                    vec![housekeeper_core::Step::remove_packages(
                        "apt",
                        vec!["libc6:amd64".into()],
                        ["apt-get", "remove", "--yes", "libc6:amd64"].map(String::from).to_vec(),
                    )],
                )
            }),
            leftovers: leftovers(),
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
Removal:    through its package manager
            as root, by Housekeeper's helper: apt-get remove --yes libc6:amd64
            also removes 12 packages: app0, app1, app2, app3, app4, app5, app6, app7, app8, app9, …, says `apt-get --simulate remove`
Leftovers:  3 folders and 1 line in shell startup files (`housekeeper remove --dry-run` measures and lists them)

/usr/lib/libc.so.6
/usr/share/doc/libc6
";
        assert_eq!(text, expected);
    }

    #[test]
    fn a_dry_run_lists_every_leftover_and_marks_what_is_the_users() {
        let details = Details {
            package: Package::new(SourceId::new("dpkg"), "tool", "tool", Scope::System),
            source: SourceInfo::new("dpkg", "Debian packages", SourceKind::System),
            also_installed: Vec::new(),
            shared: Vec::new(),
            files: None,
            needed_by: None,
            removal: Some(RemovalPlan::unknown()),
            leftovers: leftovers(),
            warnings: Vec::new(),
        };
        let mut out = Vec::new();
        render_removal(&details, true, &mut out).unwrap();
        let expected = "\
tool
dpkg:tool

Removal:    not known: nothing records how it was installed
Leftovers:  /home/u/.cache/tool (cache, at least 1.5 MB: too many files to count them all; kept unless you choose it; copies in projects or virtual environments Housekeeper does not read may still use it)
            /home/u/.config/tool (settings, 500.0 kB; yours: kept unless you choose it; snap:tool may still use it)
            /home/u/.cache/tool-thumbnails (cache, 2.0 kB; goes to the Trash with it)
Shell:      /home/u/.bashrc line 12: export PATH=\"$HOME/.tool/bin:$PATH\"
            (these lines are not edited yet; remove them by hand after removing it)

Nothing was changed: this is a dry run.
";
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }
}
