//! `housekeeper`: every installed app, however it got here.

mod filter;
mod show;
mod table;
mod who;

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use housekeeper_core::{
    Availability, Details, Host, Housekeeper, Leftovers, Lookup, MEASURE_BUDGET, Package, RemovalPlan, Selection,
    SourceStatus,
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
    /// How one installed item would be removed and what it would leave behind. Only `--dry-run` exists for now:
    /// nothing is removed until Housekeeper's root helper does it. Exits 0 when found, 1 when the name matches several
    /// items, 2 when nothing matches, 3 without `--dry-run` (removing is not available yet).
    Remove {
        /// `source:id` from `list --json` or `show` (`dpkg:firefox`), or a name or title.
        #[arg(value_name = "ID_OR_NAME")]
        query: String,
        /// Plan only: show the steps, what else would go and the leftovers, and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Print JSON instead of text.
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
        Command::Remove { query, dry_run, json } => remove(&housekeeper, &query, dry_run, json, &mut out),
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

fn show(housekeeper: &Housekeeper, query: &str, all_files: bool, json: bool, out: &mut impl Write) -> Result<ExitCode> {
    lookup(housekeeper, query, "show", json, out, |details, out| {
        if json {
            serde_json::to_writer_pretty(&mut *out, details)?;
            writeln!(out)?;
        } else {
            show::render(details, all_files, out)?;
        }
        Ok(())
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

fn remove(housekeeper: &Housekeeper, query: &str, dry_run: bool, json: bool, out: &mut impl Write) -> Result<ExitCode> {
    if !dry_run {
        writeln!(
            io::stderr().lock(),
            "housekeeper: removing is not available yet (it comes with Housekeeper's root helper); \
             `housekeeper remove --dry-run {query}` shows what it would do"
        )?;
        return Ok(ExitCode::from(3));
    }
    lookup(housekeeper, query, "remove --dry-run", json, out, |details, out| {
        housekeeper.measure_leftovers(&mut details.leftovers, MEASURE_BUDGET);
        if json {
            let dry_run = DryRun {
                package: &details.package,
                removal: &details.removal,
                leftovers: &details.leftovers,
                warnings: &details.warnings,
            };
            serde_json::to_writer_pretty(&mut *out, &dry_run)?;
            writeln!(out)?;
        } else {
            show::render_removal(details, out)?;
        }
        Ok(())
    })
}

/// Finds the one item `query` names and hands it to `found`, or lists the items it matches (exit 1) or says nothing
/// matches (exit 2). `command` is the subcommand to suggest with an item's ID.
fn lookup<W: Write>(
    housekeeper: &Housekeeper,
    query: &str,
    command: &str,
    json: bool,
    out: &mut W,
    found: impl FnOnce(&mut Details, &mut W) -> Result<()>,
) -> Result<ExitCode> {
    let mut err = io::stderr().lock();
    match housekeeper.show(query) {
        Lookup::Found(mut details) => {
            found(&mut details, out)?;
            for warning in &details.warnings {
                writeln!(err, "warning: {warning}")?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Lookup::Several(packages) => {
            if json {
                serde_json::to_writer_pretty(&mut *out, &packages)?;
                writeln!(out)?;
            } else {
                show::render_several(&packages, out)?;
            }
            writeln!(
                err,
                "housekeeper: {} installed items match `{query}`; pick one by its ID (`housekeeper {command} {}`).",
                packages.len(),
                packages.first().map(Package::key).unwrap_or_default()
            )?;
            Ok(ExitCode::from(1))
        }
        Lookup::NotFound => {
            writeln!(err, "housekeeper: nothing installed matches `{query}`")?;
            Ok(ExitCode::from(2))
        }
    }
}
