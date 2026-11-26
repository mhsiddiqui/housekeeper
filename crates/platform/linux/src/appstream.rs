//! AppStream metainfo: the catalog entry an app installs about itself (`/usr/share/metainfo/*.metainfo.xml`). Only
//! the component ID and the launchers it names are read.
//!
//! Files live in `metainfo/` (and the older `appdata/`) under `$XDG_DATA_HOME` and each `$XDG_DATA_DIRS` entry. The
//! format is simple and stable, so it is scanned for the two elements instead of parsed as general XML.

use housekeeper_core::{FileType, Host, Metainfo, ScanContext};

use crate::xdg::data_dirs;

/// Every metainfo file with a component ID. Unreadable files are warnings.
pub(crate) fn metainfo(host: &Host, scan: &ScanContext<'_>) -> Vec<Metainfo> {
    let mut found = Vec::new();
    for base in data_dirs(host) {
        for dir in ["metainfo", "appdata"].map(|name| base.join(name)) {
            let entries = match host.read_dir(&dir) {
                Ok(entries) => entries,
                Err(error) => {
                    scan.warn(error.to_string());
                    continue;
                }
            };
            for entry in entries {
                if !matches!(entry.file_type, Some(FileType::File { .. })) || !entry.name.ends_with(".xml") {
                    continue;
                }
                match host.read_to_string(&entry.path) {
                    Ok(text) => {
                        if let Some((id, launchables)) = parse(&text) {
                            found.push(Metainfo {
                                id,
                                path: entry.path,
                                launchables,
                            });
                        }
                    }
                    Err(error) => scan.warn(error.to_string()),
                }
            }
        }
    }
    found
}

/// The component's ID (the first `<id>` element, which comes before any `<provides>` or `<replaces>` IDs) and its
/// desktop launchables. `None` without an ID.
fn parse(text: &str) -> Option<(String, Vec<String>)> {
    let text = without_comments(text);
    let (_, id) = tagged(&text, "id").next()?;
    let id = unescape(id.trim());
    if id.is_empty() {
        return None;
    }
    let launchables = tagged(&text, "launchable")
        .filter(|(attributes, _)| attribute(attributes, "type") == Some("desktop-id"))
        .map(|(_, body)| unescape(body.trim()))
        .filter(|launchable| !launchable.is_empty())
        .collect();
    Some((id, launchables))
}

/// `(attributes, body)` of every `<name …>body</name>` element, in order. Self-closing elements have no body and are
/// skipped.
fn tagged<'t>(text: &'t str, name: &'t str) -> impl Iterator<Item = (&'t str, &'t str)> + 't {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut rest = text;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find(&open)?;
            let after = &rest[start + open.len()..];
            // `<id>` must not match `<identifier>`.
            if !after.starts_with(['>', ' ', '\t', '\n', '\r']) {
                rest = after;
                continue;
            }
            let end_of_tag = after.find('>')?;
            let attributes = &after[..end_of_tag];
            let body_start = &after[end_of_tag + 1..];
            if attributes.ends_with('/') {
                rest = body_start;
                continue;
            }
            let body_end = body_start.find(&close)?;
            rest = &body_start[body_end + close.len()..];
            return Some((attributes, &body_start[..body_end]));
        }
    })
}

/// The value of `name="…"` (or `'…'`) among an element's attributes.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attributes;
    while let Some(at) = rest.find(name) {
        let before_ok = at == 0 || rest[..at].ends_with(char::is_whitespace);
        let after = rest[at + name.len()..].trim_start();
        if let (true, Some(value)) = (before_ok, after.strip_prefix('=')) {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let value = &value[1..];
                return value.find(quote).map(|end| &value[..end]);
            }
        }
        rest = &rest[at + name.len()..];
    }
    None
}

fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};
    use std::path::PathBuf;

    /// The shape of GNOME Papers' file: comments first, an `<id>` inside `<replaces>` further down.
    const PAPERS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- <id>not.this.one</id> -->
<component type="desktop-application">
  <id>org.gnome.Papers</id>
  <developer id="org.gnome"><name>The GNOME Project</name></developer>
  <launchable type="desktop-id">org.gnome.Papers.desktop</launchable>
  <launchable type="url">https://example.org/</launchable>
  <replaces>
    <id>papers.desktop</id>
  </replaces>
</component>
"#;

    #[test]
    fn reads_the_component_id_and_its_desktop_launchers() {
        assert_eq!(
            parse(PAPERS),
            Some(("org.gnome.Papers".into(), vec!["org.gnome.Papers.desktop".into()]))
        );
        assert_eq!(
            parse("<component><identifier>x</identifier><id> com.example.A&amp;B </id></component>"),
            Some(("com.example.A&B".into(), Vec::new()))
        );
        assert_eq!(parse("<component><name>No id</name></component>"), None);
        assert_eq!(parse("<component><id/></component>"), None);
    }

    #[test]
    fn reads_metainfo_and_appdata_under_every_data_dir() {
        let fixture = Fixture::new();
        fixture.file("/usr/share/metainfo/org.gnome.Papers.metainfo.xml", PAPERS);
        fixture.file(
            "/usr/share/appdata/old.appdata.xml",
            "<component><id>org.example.Old</id></component>",
        );
        fixture.file("/usr/share/metainfo/README", "not metainfo");
        fixture.file(
            &format!("{HOME}/.local/share/metainfo/mine.metainfo.xml"),
            "<component><id>org.example.Mine</id></component>",
        );
        let host = fixture.host(FakeRunner::new());

        let context = ScanContext::default();
        let found = metainfo(&host, &context);
        let ids: Vec<(&str, PathBuf)> = found.iter().map(|m| (m.id.as_str(), m.path.clone())).collect();
        assert_eq!(
            ids,
            [
                (
                    "org.example.Mine",
                    PathBuf::from(format!("{HOME}/.local/share/metainfo/mine.metainfo.xml"))
                ),
                (
                    "org.gnome.Papers",
                    PathBuf::from("/usr/share/metainfo/org.gnome.Papers.metainfo.xml")
                ),
                ("org.example.Old", PathBuf::from("/usr/share/appdata/old.appdata.xml")),
            ]
        );
        assert!(context.into_warnings().is_empty());
    }
}
