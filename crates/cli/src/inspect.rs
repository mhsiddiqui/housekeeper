//! Text output of `housekeeper inspect`.

use std::io::{self, Write};

use housekeeper_core::{Fit, Inspection, SystemInfo};

/// The file, its format and how it was told, what is inside, the source that lists it once installed, and whether it
/// fits.
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
    Ok(())
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
