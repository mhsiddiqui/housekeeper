//! Text output of `housekeeper who`.

use std::io::{self, Write};

use housekeeper_core::{Evidence, Found, InstallReason, Owner, Package, Scope, Who};

/// The first file the query names (the one that runs), its owner and why, then the files hidden behind it.
pub fn render(who: &Who, out: &mut impl Write) -> io::Result<()> {
    let Some((first, hidden)) = who.found.split_first() else {
        return Ok(());
    };
    let chain: Vec<String> = std::iter::once(&first.path)
        .chain(&first.links)
        .map(|path| path.display().to_string())
        .collect();
    writeln!(out, "{}", chain.join(" → "))?;
    match &first.owner {
        Some(owner) => {
            let name = first
                .path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            writeln!(out, "Owner:   {}", describe(owner, &name))?;
            writeln!(out, "Because: {}", because(owner))?;
        }
        None => writeln!(
            out,
            "Owner:   nobody. No package manager or installer on this machine lists it; it was probably copied there \
             by hand or by a script."
        )?,
    }
    for found in hidden {
        writeln!(out, "Hidden:  {}", hidden_line(found))?;
    }
    Ok(())
}

/// `Debian packages · git 1:2.51.0-1 (installed on purpose, for all users)`. `name` is the file asked about.
fn describe(owner: &Owner, name: &str) -> String {
    let label = &owner.source.label;
    let Some(package) = &owner.package else {
        return match &owner.evidence {
            Evidence::ListedFile { packages, .. } if packages.len() > 1 => {
                let mut shown: Vec<&str> = packages.iter().take(3).map(String::as_str).collect();
                if packages.len() > shown.len() {
                    shown.push("…");
                }
                format!("{label}, shared by {} packages ({})", packages.len(), shown.join(", "))
            }
            Evidence::ListedFile { packages, .. } => format!("{label} · {}", packages.join(", ")),
            Evidence::InsideInstall { .. } | Evidence::InsideFolder { .. } => {
                format!("{label}, though none of its packages lists {name}")
            }
        };
    };
    let mut text = format!("{label} · {}", package.display_name());
    if let Some(version) = &package.version {
        text.push(' ');
        text.push_str(version);
    }
    let details: Vec<&str> = reason(package)
        .into_iter()
        .chain(std::iter::once(match package.scope {
            Scope::System => "for all users",
            Scope::User => "for this user",
        }))
        .collect();
    text.push_str(&format!(" ({})", details.join(", ")));
    text
}

fn reason(package: &Package) -> Option<&'static str> {
    match package.reason {
        InstallReason::Explicit => Some("installed on purpose"),
        InstallReason::Dependency => Some("installed as a dependency"),
        InstallReason::System => Some("part of the system"),
        InstallReason::Unknown => None,
    }
}

fn because(owner: &Owner) -> String {
    let label = &owner.source.label;
    let package = owner.package.as_ref().map(Package::display_name);
    match (&owner.evidence, package) {
        (Evidence::ListedFile { path, packages }, _) if packages.len() > 1 => {
            format!("{label} lists {} as a file of each of them", path.display())
        }
        (Evidence::ListedFile { path, .. }, Some(package)) => {
            format!("{label} lists {} as a file of {package}", path.display())
        }
        (Evidence::ListedFile { path, packages }, None) => {
            format!("{label} lists {} as a file of {}", path.display(), packages.join(", "))
        }
        (Evidence::InsideInstall { path, location }, Some(package)) if path == location => {
            format!("{label} lists {} as {package} itself", path.display())
        }
        (Evidence::InsideInstall { path, location }, Some(package)) => format!(
            "{} is inside {}, where {package} is installed",
            path.display(),
            location.display()
        ),
        (Evidence::InsideInstall { path, location }, None) => format!(
            "{} is inside {}, where {label} installs",
            path.display(),
            location.display()
        ),
        (Evidence::InsideFolder { path, folder }, _) => format!(
            "{} is inside {}, where {label} installs programs",
            path.display(),
            folder.display()
        ),
    }
}

fn hidden_line(found: &Found) -> String {
    let owner = found.owner.as_ref().map_or_else(
        || "nobody".to_string(),
        |owner| match &owner.package {
            Some(package) => {
                let version = package.version.as_deref().map(|v| format!(" {v}")).unwrap_or_default();
                format!("{} · {}{version}", owner.source.label, package.display_name())
            }
            None => owner.source.label.to_string(),
        },
    );
    format!("{} ({owner})", found.path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::{SourceId, SourceInfo, SourceKind};
    use std::path::PathBuf;

    fn owner(source: &'static str, label: &'static str, name: &str, evidence: Evidence) -> Owner {
        let mut package = Package::new(SourceId::new(source), name, name, Scope::System);
        package.version = Some("20.19.2".into());
        package.reason = InstallReason::Explicit;
        Owner {
            source: SourceInfo::new(source, label, SourceKind::System),
            package: Some(package),
            evidence,
        }
    }

    #[test]
    fn owner_evidence_and_hidden_matches() {
        let who = Who {
            query: "node".into(),
            found: vec![
                Found {
                    path: PathBuf::from("/home/u/.local/bin/node"),
                    links: vec![PathBuf::from("/opt/node/bin/node")],
                    owner: None,
                },
                Found {
                    path: PathBuf::from("/usr/bin/node"),
                    links: Vec::new(),
                    owner: Some(owner(
                        "dpkg",
                        "Debian packages",
                        "nodejs",
                        Evidence::ListedFile {
                            path: PathBuf::from("/usr/bin/node"),
                            packages: vec!["nodejs".into()],
                        },
                    )),
                },
            ],
            warnings: Vec::new(),
        };
        let mut out = Vec::new();
        render(&who, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.starts_with("/home/u/.local/bin/node → /opt/node/bin/node\nOwner:   nobody."),
            "{text}"
        );
        assert!(
            text.ends_with("Hidden:  /usr/bin/node (Debian packages · nodejs 20.19.2)\n"),
            "{text}"
        );

        let mut out = Vec::new();
        let first = who.found[1].clone();
        render(
            &Who {
                found: vec![first],
                ..who
            },
            &mut out,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "/usr/bin/node\n\
             Owner:   Debian packages · nodejs 20.19.2 (installed on purpose, for all users)\n\
             Because: Debian packages lists /usr/bin/node as a file of nodejs\n"
        );
    }
}
