//! `housekeeper`: every installed app, however it got here.

mod filter;
mod history;
mod inspect;
mod show;
mod table;
mod who;

use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use housekeeper_core::history::ChangeOutcome;
use housekeeper_core::{
    Availability, Cancel, Details, Event, Host, Housekeeper, Inspection, Leftover, Leftovers, Lookup, MEASURE_BUDGET,
    Package, Recorded, RemovalPlan, Selection, SourceStatus,
};
use serde::Serialize;

use crate::filter::{Filter, View, keep_duplicates};
use crate::table::{Table, human_size};

#[derive(Parser)]
#[command(name = "housekeeper", version, about = "Every installed app, however it got here")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show which sources exist on this machine.
    Sources {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// List installed apps. By default only things someone installed on purpose and launches are shown.
    List {
        /// Also show dependencies, libraries, runtimes, components (fonts, data, plugins…) and system packages.
        #[arg(long)]
        all: bool,
        /// Only read these sources (repeatable), e.g. `--source flatpak --source snap`.
        #[arg(long = "source", value_name = "ID")]
        sources: Vec<String>,
        /// Only show items whose name, title or summary contains this text (case-insensitive).
        #[arg(long)]
        search: Option<String>,
        /// Only show apps installed more than once (from several sources, or twice from one), grouped.
        #[arg(long)]
        duplicates: bool,
        /// Print the scan as JSON (schema version in the `schema` field).
        #[arg(long)]
        json: bool,
    },
    /// Tell which source and package a command or file comes from. Exits 0 when an owner is found, 1 when the file
    /// exists but nothing owns it, 2 when there is no such command or file.
    Who {
        /// A command on the PATH (`git`) or a path to a file (`/usr/bin/git`, `./tool`).
        #[arg(value_name = "COMMAND_OR_PATH")]
        query: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Everything about one installed item: details, launchers, other installs of the same app, files, and what
    /// needs it. Exits 0 when found, 1 when the name matches several items, 2 when nothing matches.
    Show {
        /// `source:id` from `list --json` or a previous `show` (`dpkg:firefox`), or a name or title.
        #[arg(value_name = "ID_OR_NAME")]
        query: String,
        /// Also list every file.
        #[arg(long)]
        files: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// What a file someone wants to install is (`.deb`, `.rpm`, Arch package, `.flatpakref`, Flatpak bundle, `.snap`,
    /// AppImage, archive, script), what is inside, whether it is made for this system, and how it would be installed
    /// (what the package manager would add or remove with it). Reads the file; runs and installs nothing. Exits 0 when
    /// it fits, 1 when it does not or its install is blocked, 2 when there is no such file or its format is unknown.
    Inspect {
        /// The file (`~/Downloads/app.deb`, `./app.AppImage`).
        file: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Install a downloaded file (`.deb`, `.rpm`, Arch package, `.flatpakref`, Flatpak bundle, `.snap`) the way its
    /// format intends, after showing what `inspect` shows and asking. Package files go through the system's package
    /// tool and Housekeeper's root helper (an administrator's password is asked), which installs its own copy of the
    /// file only if it is still the file inspected. Exits 0 when it was installed, 2 when there is no such file or its
    /// format is unknown, 3 when nothing was changed (it does not fit, has no install plan, is blocked, declined,
    /// changed since it was inspected), 4 when a step failed.
    Install {
        /// The file (`~/Downloads/app.deb`, `./app.flatpakref`).
        file: String,
        /// Install without asking first.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Print JSON instead of text: what was done, as the history keeps it.
        #[arg(long)]
        json: bool,
    },
    /// Remove one installed item the way its installer intended, after showing the plan and asking. Steps that need
    /// root go through Housekeeper's root helper (an administrator's password is asked). Exits 0 when it was removed,
    /// 1 when the name matches several items, 2 when nothing matches, 3 when nothing was changed (blocked, declined,
    /// changed since it was planned, cancelled), 4 when a step failed.
    Remove {
        /// `source:id` from `list --json` or `show` (`dpkg:firefox`), or a name or title.
        #[arg(value_name = "ID_OR_NAME")]
        query: String,
        /// Plan only: show the steps, what else would go and the leftovers, and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Remove without asking first.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Which leftover folders go to the Trash with it: those nothing else may use (`caches`, the default), none, or
        /// all of them, your settings and data too.
        #[arg(long, value_enum, default_value_t = LeftoverChoice::Caches)]
        leftovers: LeftoverChoice,
        /// Print JSON instead of text: the plan with `--dry-run`, otherwise what was done (as the history keeps it).
        #[arg(long)]
        json: bool,
    },
    /// What Housekeeper changed on this computer, oldest first, from its history in
    /// `$XDG_STATE_HOME/housekeeper/history.jsonl`.
    History {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let host = Host::local();
    let housekeeper = Housekeeper::new(housekeeper_platform::detect(&host)?, host);
    let mut out = io::stdout().lock();
    let result = match cli.command {
        Command::Sources { json } => sources(&housekeeper, json, &mut out).map(|()| ExitCode::SUCCESS),
        Command::List {
            all,
            sources,
            search,
            duplicates,
            json,
        } => {
            let view = View {
                filter: Filter { all, search },
                duplicates,
            };
            list(&housekeeper, &sources, &view, json, &mut out).map(|()| ExitCode::SUCCESS)
        }
        Command::Who { query, json } => who(&housekeeper, &query, json, &mut out),
        Command::Show { query, files, json } => show(&housekeeper, &query, files, json, &mut out),
        Command::Inspect { file, json } => inspect(&housekeeper, &file, json, &mut out),
        Command::Install { file, yes, json } => install(&housekeeper, &file, yes, json, &mut out),
        Command::Remove {
            query,
            dry_run,
            yes,
            leftovers,
            json,
        } => remove(
            &housekeeper,
            &query,
            &RemoveOptions {
                dry_run,
                yes,
                leftovers,
                json,
            },
            &mut out,
        ),
        Command::History { json } => history(&housekeeper, json, &mut out).map(|()| ExitCode::SUCCESS),
    };
    match result {
        // The reader went away (`housekeeper list | head`): not an error.
        Err(error) if is_broken_pipe(&error) => Ok(ExitCode::SUCCESS),
        other => other,
    }
}

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<io::Error>().map(io::Error::kind).or_else(|| {
            cause
                .downcast_ref::<serde_json::Error>()
                .and_then(|e| e.io_error_kind())
        }) == Some(io::ErrorKind::BrokenPipe)
    })
}

fn history(housekeeper: &Housekeeper, json: bool, out: &mut impl Write) -> Result<()> {
    let entries = housekeeper.history()?;
    if json {
        serde_json::to_writer_pretty(&mut *out, &entries)?;
        writeln!(out)?;
        Ok(())
    } else {
        Ok(history::render(&entries, out)?)
    }
}

fn sources(housekeeper: &Housekeeper, json: bool, out: &mut impl Write) -> Result<()> {
    let probed = housekeeper.probe();
    if json {
        #[derive(Serialize)]
        struct Row<'a> {
            #[serde(flatten)]
            info: &'a housekeeper_core::SourceInfo,
            availability: &'a Availability,
        }
        let rows: Vec<Row> = probed
            .iter()
            .map(|(info, availability)| Row { info, availability })
            .collect();
        serde_json::to_writer_pretty(&mut *out, &rows)?;
        writeln!(out)?;
        return Ok(());
    }
    let system = housekeeper.system();
    writeln!(out, "{} ({}, {})\n", system.name, system.family, system.arch)?;
    let mut table = Table::new(["SOURCE", "ID", "STATUS"]);
    for (info, availability) in &probed {
        let status = match availability {
            Availability::Available { version: Some(v) } => format!("available, {v}"),
            Availability::Available { version: None } => "available".to_string(),
            Availability::Unavailable { reason } => format!("not available: {reason}"),
        };
        table.row([info.label.to_string(), info.id.to_string(), status]);
    }
    table.write(out)?;
    Ok(())
}

fn list(housekeeper: &Housekeeper, only: &[String], view: &View, json: bool, out: &mut impl Write) -> Result<()> {
    let known: Vec<String> = housekeeper.sources().map(|info| info.id.to_string()).collect();
    if let Some(unknown) = only.iter().find(|id| !known.contains(id)) {
        bail!("unknown source `{unknown}`; available: {}", known.join(", "));
    }
    let selection = if only.is_empty() {
        Selection::All
    } else {
        Selection::Only(only)
    };
    let filter = &view.filter;
    let mut scan = housekeeper.scan(&selection);
    let scanned = scan.packages.len();
    if view.duplicates {
        keep_duplicates(&mut scan, filter);
    } else {
        scan.packages.retain(|package| filter.keeps(package));
    }

    if json {
        serde_json::to_writer_pretty(&mut *out, &scan)?;
        writeln!(out)?;
        return Ok(());
    }

    let mut err = io::stderr().lock();
    if view.duplicates {
        let by_key: HashMap<String, &Package> = scan.packages.iter().map(|package| (package.key(), package)).collect();
        // FROM tells the copies apart: the repository or Node/Python version, or else where the install is.
        let mut table = Table::new(["APP", "NAME", "VERSION", "SOURCE", "SIZE", "FROM"]);
        for group in &scan.groups {
            let installs = group.packages.iter().filter_map(|key| by_key.get(key));
            for (n, package) in installs.enumerate() {
                let from = package
                    .origin
                    .clone()
                    .or_else(|| package.location.as_ref().map(|path| path.display().to_string()));
                table.row([
                    if n == 0 { group.name.clone() } else { String::new() },
                    package.display_name().to_string(),
                    package.version.clone().unwrap_or_default(),
                    package.source.to_string(),
                    package.size_bytes.map(human_size).unwrap_or_default(),
                    from.unwrap_or_default(),
                ]);
            }
        }
        table.write(out)?;
        writeln!(
            err,
            "\n{} installed more than once, {} installs.",
            count(scan.groups.len(), "app"),
            scan.packages.len()
        )?;
    } else {
        let mut table = Table::new(["NAME", "VERSION", "SOURCE", "SIZE"]);
        for package in &scan.packages {
            table.row([
                package.display_name().to_string(),
                package.version.clone().unwrap_or_default(),
                package.source.to_string(),
                package.size_bytes.map(human_size).unwrap_or_default(),
            ]);
        }
        table.write(out)?;
        writeln!(
            err,
            "\n{} shown of {} found{}.",
            scan.packages.len(),
            scanned,
            if filter.all {
                ""
            } else {
                " (use --all to include dependencies, libraries, runtimes and components)"
            }
        )?;
        if !scan.groups.is_empty() {
            writeln!(
                err,
                "{} installed more than once; `housekeeper list --duplicates` shows which.",
                count(scan.groups.len(), "app")
            )?;
        }
    }
    for report in &scan.sources {
        match &report.status {
            SourceStatus::Failed { error } => writeln!(err, "warning: could not read {}: {error}", report.info.label)?,
            SourceStatus::Ok { warnings, .. } => {
                for warning in warnings {
                    writeln!(err, "warning: {}: {warning}", report.info.label)?;
                }
            }
            SourceStatus::Unavailable { .. } => {}
        }
    }
    for warning in &scan.warnings {
        writeln!(err, "warning: {warning}")?;
    }
    Ok(())
}

/// `1 app`, `3 apps`.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn who(housekeeper: &Housekeeper, query: &str, json: bool, out: &mut impl Write) -> Result<ExitCode> {
    // Relative paths (`./tool`, `bin/tool`) are relative to where the command runs; core wants absolute ones.
    let query = if query.contains('/') && !query.starts_with('/') {
        std::env::current_dir()?.join(query).to_string_lossy().into_owned()
    } else {
        query.to_string()
    };
    let answer = housekeeper.who(&query);
    if json {
        serde_json::to_writer_pretty(&mut *out, &answer)?;
        writeln!(out)?;
    } else {
        who::render(&answer, out)?;
    }
    let mut err = io::stderr().lock();
    for warning in &answer.warnings {
        writeln!(err, "warning: {warning}")?;
    }
    Ok(match answer.found.first() {
        None => {
            writeln!(err, "housekeeper: no command or file named `{query}`")?;
            ExitCode::from(2)
        }
        Some(found) if found.owner.is_none() => ExitCode::from(1),
        Some(_) => ExitCode::SUCCESS,
    })
}

/// The file named on the command line, inspected, after printing its warnings; `None` after saying why there is no
/// inspection (no such file, an unknown format).
fn inspected(housekeeper: &Housekeeper, file: &str) -> Result<Option<Inspection>> {
    // Core wants absolute paths; a relative one is relative to where the command runs. `components` drops `./`.
    let path: PathBuf = std::env::current_dir()?.join(file).components().collect();
    match housekeeper.inspect(&path) {
        Ok(Some(inspection)) => {
            for warning in &inspection.warnings {
                eprintln!("warning: {warning}");
            }
            Ok(Some(inspection))
        }
        Ok(None) => {
            eprintln!(
                "housekeeper: {} is no installer format Housekeeper knows",
                path.display()
            );
            Ok(None)
        }
        Err(error) => {
            eprintln!("housekeeper: {error}");
            Ok(None)
        }
    }
}

fn inspect(housekeeper: &Housekeeper, file: &str, json: bool, out: &mut impl Write) -> Result<ExitCode> {
    let Some(inspection) = inspected(housekeeper, file)? else {
        return Ok(ExitCode::from(2));
    };
    if json {
        serde_json::to_writer_pretty(&mut *out, &inspection)?;
        writeln!(out)?;
    } else {
        inspect::render(&inspection, housekeeper.system(), out)?;
    }
    let blocked = inspection
        .install
        .as_ref()
        .is_some_and(|plan| !plan.blockers.is_empty());
    Ok(if inspection.fit == housekeeper_core::Fit::Fits && !blocked {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn show(housekeeper: &Housekeeper, query: &str, all_files: bool, json: bool, out: &mut impl Write) -> Result<ExitCode> {
    lookup(housekeeper, query, "show", json, out, |details, out| {
        if json {
            serde_json::to_writer_pretty(&mut *out, details)?;
            writeln!(out)?;
        } else {
            show::render(details, all_files, out)?;
        }
        Ok(ExitCode::SUCCESS)
    })
}

/// What `remove --dry-run --json` prints: the item, its plan and its leftovers.
#[derive(Serialize)]
struct DryRun<'a> {
    package: &'a Package,
    removal: &'a Option<RemovalPlan>,
    leftovers: &'a Leftovers,
    warnings: &'a [String],
}

/// Which leftover folders a removal moves to the Trash (D38).
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum LeftoverChoice {
    /// Those nothing else may use (`Leftover::preselected`).
    Caches,
    None,
    /// Every folder listed, the user's settings and data included.
    All,
}

/// What `remove` was told.
struct RemoveOptions {
    dry_run: bool,
    yes: bool,
    leftovers: LeftoverChoice,
    json: bool,
}

fn remove(housekeeper: &Housekeeper, query: &str, options: &RemoveOptions, out: &mut impl Write) -> Result<ExitCode> {
    let command = if options.dry_run { "remove --dry-run" } else { "remove" };
    lookup(housekeeper, query, command, options.json, out, |details, out| {
        housekeeper.measure_leftovers(&mut details.leftovers, MEASURE_BUDGET);
        if options.json && options.dry_run {
            let dry_run = DryRun {
                package: &details.package,
                removal: &details.removal,
                leftovers: &details.leftovers,
                warnings: &details.warnings,
            };
            serde_json::to_writer_pretty(&mut *out, &dry_run)?;
            writeln!(out)?;
            return Ok(ExitCode::SUCCESS);
        }
        if !options.json {
            show::render_removal(details, options.dry_run, out)?;
        }
        if options.dry_run {
            return Ok(ExitCode::SUCCESS);
        }
        carry_out(housekeeper, details, options, out)
    })
}

/// Asks, checks the plan again (D37), removes, and says how each step went. Exit codes as `remove` documents.
fn carry_out(
    housekeeper: &Housekeeper,
    details: &Details,
    options: &RemoveOptions,
    out: &mut impl Write,
) -> Result<ExitCode> {
    let not_done = ExitCode::from(3);
    let title = details.package.display_name().to_string();
    let plan = details.removal.clone().unwrap_or_else(RemovalPlan::unknown);
    if !plan.blockers.is_empty() || plan.steps.is_empty() {
        let why = if plan.steps.is_empty() {
            "nothing records how it is removed".to_string()
        } else {
            plan.blockers.join("; ")
        };
        eprintln!("housekeeper: {title} is not removed: {why}. Nothing was changed.");
        return Ok(not_done);
    }
    let chosen: Vec<Leftover> = details
        .leftovers
        .folders
        .iter()
        .filter(|folder| match options.leftovers {
            LeftoverChoice::Caches => folder.preselected(),
            LeftoverChoice::None => false,
            LeftoverChoice::All => true,
        })
        .cloned()
        .collect();
    let also = match chosen.len() {
        0 => String::new(),
        n => format!(" and move {} to the Trash", count(n, "leftover folder")),
    };
    if !options.yes && !confirm(&format!("remove {title}"), &format!("Remove {title}{also}?"))? {
        return Ok(not_done);
    }
    if let Err(change) = housekeeper.still_as_reviewed(details) {
        eprintln!("housekeeper: {title} changed since its removal was planned ({change}). Nothing was changed.");
        return Ok(not_done);
    }
    let removal = housekeeper.remove(details, &chosen, &Cancel::default(), &|event| match event {
        Event::Waiting { holder, .. } => eprintln!(
            "Waiting for {} to let go of {}…",
            match (&holder.program, holder.pid) {
                (Some(program), Some(pid)) => format!("{program} (process {pid})"),
                _ => "another program".to_string(),
            },
            holder.lock.display()
        ),
        Event::Started { .. } => eprintln!("Removing {title}…"),
        Event::Finished { .. } | Event::Cancelled { .. } => {}
    });
    if options.json {
        serde_json::to_writer_pretty(&mut *out, &removal.entry)?;
        writeln!(out)?;
    } else {
        show::render_outcome(&removal.entry, out)?;
    }
    if let Some(error) = removal.history_error {
        eprintln!("warning: the history could not be written: {error}");
    }
    Ok(match removal.entry.outcome {
        ChangeOutcome::Done => ExitCode::SUCCESS,
        ChangeOutcome::NotDone => not_done,
        ChangeOutcome::Failed => ExitCode::from(4),
    })
}

/// Finds the one item `query` names and hands it to `found`, whose exit code it returns, or lists the items it matches
/// (exit 1) or says nothing matches (exit 2). `command` is the subcommand to suggest with an item's ID.
fn lookup<W: Write>(
    housekeeper: &Housekeeper,
    query: &str,
    command: &str,
    json: bool,
    out: &mut W,
    found: impl FnOnce(&mut Details, &mut W) -> Result<ExitCode>,
) -> Result<ExitCode> {
    match housekeeper.show(query) {
        Lookup::Found(mut details) => {
            for warning in &details.warnings {
                eprintln!("warning: {warning}");
            }
            found(&mut details, out)
        }
        Lookup::Several(packages) => {
            if json {
                serde_json::to_writer_pretty(&mut *out, &packages)?;
                writeln!(out)?;
            } else {
                show::render_several(&packages, out)?;
            }
            eprintln!(
                "housekeeper: {} installed items match `{query}`; pick one by its ID (`housekeeper {command} {}`).",
                packages.len(),
                packages.first().map(Package::key).unwrap_or_default()
            );
            Ok(ExitCode::from(1))
        }
        Lookup::NotFound => {
            eprintln!("housekeeper: nothing installed matches `{query}`");
            Ok(ExitCode::from(2))
        }
    }
}
