//! `housekeeper`: every installed app, however it got here.

mod filter;
mod table;
mod who;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use housekeeper_core::{Availability, Host, Housekeeper, Selection, SourceStatus};
use serde::Serialize;

use crate::filter::Filter;
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
            json,
        } => {
            let filter = Filter { all, search };
            list(&housekeeper, &sources, &filter, json, &mut out).map(|()| ExitCode::SUCCESS)
        }
        Command::Who { query, json } => who(&housekeeper, &query, json, &mut out),
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

fn list(housekeeper: &Housekeeper, only: &[String], filter: &Filter, json: bool, out: &mut impl Write) -> Result<()> {
    let known: Vec<String> = housekeeper.sources().map(|info| info.id.to_string()).collect();
    if let Some(unknown) = only.iter().find(|id| !known.contains(id)) {
        bail!("unknown source `{unknown}`; available: {}", known.join(", "));
    }
    let selection = if only.is_empty() {
        Selection::All
    } else {
        Selection::Only(only)
    };
    let mut scan = housekeeper.scan(&selection);
    let scanned = scan.packages.len();
    scan.packages.retain(|package| filter.keeps(package));

    if json {
        serde_json::to_writer_pretty(&mut *out, &scan)?;
        writeln!(out)?;
        return Ok(());
    }

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

    let mut err = io::stderr().lock();
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
