//! Text output of `housekeeper inspect`.

use std::io::{self, Write};

use housekeeper_core::{Fit, Inspection, InstallPlan, SystemInfo};

use crate::show::{SIMULATED_SHOWN, step_text};

/// The file, its format and how it was told, what is inside, the source that lists it once installed, whether it
/// fits, and how it would be installed.
pub fn render(inspection: &Inspection, system: &SystemInfo, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "{}", inspection.path.display())?;
    writeln!(out, "Format:    {}", inspection.format.title)?;
    writeln!(out, "Told by:   {}", inspection.evidence)?;
    if let Some(package) = &inspection.package {
        let name: Vec<&str> = package
            .name
            .iter()
            .chain(&package.version)
            .map(String::as_str)
            .collect();
        if !name.is_empty() {
            writeln!(out, "Package:   {}", name.join(" "))?;
        }
        if let Some(title) = &package.title {
            writeln!(out, "Title:     {title}")?;
        }
        if let Some(arch) = &package.arch {
            writeln!(out, "Processor: {arch}")?;
        }
        if let Some(origin) = &package.origin {
            writeln!(out, "From:      {origin}")?;
        }
    }
    if let Some(source) = &inspection.source {
        writeln!(out, "Source:    {}", source.label)?;
    }
    writeln!(out, "Fits:      {}", fit(inspection, system))?;
    if inspection.fit == Fit::Fits {
        let lines = match &inspection.install {
            Some(plan) => install(plan),
            None => vec![format!(
                "not planned: Housekeeper has no install plan for {} files yet",
                inspection.format.title
            )],
        };
        for (n, line) in lines.iter().enumerate() {
            writeln!(out, "{:<11}{line}", if n == 0 { "Install:" } else { "" })?;
        }
    }
    Ok(())
}

/// The steps, then what the package manager says comes with them, then what to know and what blocks it.
fn install(plan: &InstallPlan) -> Vec<String> {
    let mut lines: Vec<String> = plan.steps.iter().map(step_text).collect();
    if let Some(version) = &plan.replaces {
        lines.push(format!("replaces version {version}, installed now"));
    }
    // A refusal is listed with the blockers.
    if let Some(simulation) = plan.simulation.as_ref().filter(|s| s.refused.is_empty()) {
        lines.push(format!("checked with {}:", simulation.by));
        if simulation.installs.is_empty() && simulation.needs.is_empty() && simulation.removes.is_empty() {
            lines.push("  nothing would change".into());
        }
        if !simulation.installs.is_empty() {
            lines.push(format!("  installs {}", some(&simulation.installs, "package")));
        }
        if !simulation.needs.is_empty() {
            lines.push(format!(
                "  needs from the repositories {}",
                some(&simulation.needs, "requirement")
            ));
        }
        if !simulation.removes.is_empty() {
            lines.push(format!("  removes {}", some(&simulation.removes, "package")));
        }
    }
    if plan.restart {
        lines.push("takes effect after a restart".into());
    }
    lines.extend(plan.cautions.iter().map(|caution| format!("note: {caution}")));
    lines.extend(plan.blockers.iter().map(|blocker| format!("blocked: {blocker}")));
    lines
}

/// `3 packages: a, b, c`, naming at most [`SIMULATED_SHOWN`].
fn some(names: &[String], noun: &str) -> String {
    let mut shown: Vec<&str> = names.iter().map(String::as_str).take(SIMULATED_SHOWN).collect();
    if names.len() > SIMULATED_SHOWN {
        shown.push("…");
    }
    format!("{}: {}", crate::count(names.len(), noun), shown.join(", "))
}

/// `yes`, or `no` and why.
fn fit(inspection: &Inspection, system: &SystemInfo) -> String {
    match &inspection.fit {
        Fit::Fits => "yes".to_string(),
        Fit::OtherSystem { families } => format!(
            "no, it is a package for {} systems; this is {}",
            families.join(" and "),
            system.name
        ),
        Fit::SourceMissing { reason } => {
            let what = inspection
                .source
                .as_ref()
                .map_or("what installs it", |source| source.label.as_ref());
            format!("no, {what} is not available here: {reason}")
        }
        Fit::OtherArch { arch } => format!("no, it is built for {arch}; this computer is {}", system.arch),
    }
}
