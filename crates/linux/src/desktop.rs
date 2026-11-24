//! The application menu: desktop entries, as the freedesktop.org Desktop Entry Specification (1.5) describes them.
//!
//! Entries live in `applications/` under `$XDG_DATA_HOME` and each `$XDG_DATA_DIRS` entry, most important first. An
//! entry's id is its path below `applications/` with `/` turned into `-`; the first file with an id is the one menus
//! use, and it hides the others (a user's copy in `~/.local/share/applications` hides the system's).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use housekeeper_core::{FileType, Host, Launcher, ScanContext};

/// `$XDG_DATA_DIRS` when it is not set.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share/:/usr/share/";

/// How deep subfolders of `applications/` are read (`kde4/`, `wine/Programs/…`).
const MAX_DEPTH: usize = 4;

/// Every application entry, one per id, the one that takes precedence. `command_dirs` is where a program named
/// without a path (`Exec=gimp`) is looked for.
pub(crate) fn launchers(host: &Host, command_dirs: &[PathBuf], scan: &ScanContext<'_>) -> Vec<Launcher> {
    let mut files: Vec<(String, Vec<PathBuf>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for dir in application_dirs(host) {
        let mut found = Vec::new();
        desktop_files(host, &dir, "", 0, &mut found, scan);
        for (id, path) in found {
            match index.get(&id) {
                Some(&i) => files[i].1.push(path),
                None => {
                    index.insert(id.clone(), files.len());
                    files.push((id, vec![path]));
                }
            }
        }
    }

    let mut launchers = Vec::new();
    for (id, mut paths) in files {
        let path = paths.remove(0);
        let text = match host.read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                scan.warn(error.to_string());
                continue;
            }
        };
        let Some(entry) = parse(&text) else {
            continue;
        };
        launchers.push(Launcher {
            id,
            name: entry.name,
            icon: entry.icon,
            path,
            shadows: paths,
            program: entry
                .exec
                .as_deref()
                .and_then(program)
                .and_then(|program| resolve(host, &program, command_dirs)),
            hidden: entry.hidden,
        });
    }
    launchers
}

/// `applications/` under `$XDG_DATA_HOME`, then under each `$XDG_DATA_DIRS` entry, without repeats.
fn application_dirs(host: &Host) -> Vec<PathBuf> {
    let data_dirs = host.var("XDG_DATA_DIRS").unwrap_or(DEFAULT_DATA_DIRS);
    let mut dirs: Vec<PathBuf> = vec![host.data_home()];
    for dir in data_dirs
        .split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(PathBuf::from)
    {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs.into_iter().map(|dir| dir.join("applications")).collect()
}

/// `(id, path)` of every `.desktop` file below `dir`, whose id starts with `prefix`.
fn desktop_files(
    host: &Host,
    dir: &Path,
    prefix: &str,
    depth: usize,
    found: &mut Vec<(String, PathBuf)>,
    scan: &ScanContext<'_>,
) {
    let entries = match host.read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            scan.warn(error.to_string());
            return;
        }
    };
    for entry in entries {
        match entry.file_type {
            Some(FileType::Dir) if depth < MAX_DEPTH => {
                let prefix = format!("{prefix}{}-", entry.name);
                desktop_files(host, &entry.path, &prefix, depth + 1, found, scan);
            }
            Some(FileType::File { .. }) if entry.name.ends_with(".desktop") => {
                found.push((format!("{prefix}{}", entry.name), entry.path));
            }
            _ => {}
        }
    }
}

/// What a launcher needs from a desktop entry.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    name: String,
    icon: Option<String>,
    /// The command line, with the string escapes resolved; `Exec` quoting still applies.
    exec: Option<String>,
    hidden: bool,
}

/// The `[Desktop Entry]` group of an application entry; `None` for links, folders and files without a name.
fn parse(text: &str) -> Option<Entry> {
    let mut in_entry = false;
    let mut kind = None;
    let mut name = None;
    let mut icon = None;
    let mut exec = None;
    let mut hidden = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if in_entry {
                break;
            }
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim_start();
        match key.trim_end() {
            "Type" => kind = Some(value),
            "Name" => name = Some(unescape(value)),
            "Icon" => icon = Some(unescape(value)).filter(|icon| !icon.is_empty()),
            "Exec" => exec = Some(unescape(value)),
            // `Hidden` means the entry was deleted; `NoDisplay`, that menus leave it out.
            "NoDisplay" | "Hidden" => hidden |= value == "true",
            _ => {}
        }
    }
    if kind != Some("Application") {
        return None;
    }
    Some(Entry {
        name: name.filter(|name| !name.is_empty())?,
        icon,
        exec,
        hidden,
    })
}

/// Resolves the escapes of a string value: `\s`, `\n`, `\t`, `\r` and `\\`. Others are kept for `Exec` quoting.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The program an `Exec` command line starts, as written: past `env` and its variables. `None` when the line cannot
/// be read or runs a shell, whose script could start anything.
fn program(exec: &str) -> Option<String> {
    let mut args = split(exec)?.into_iter();
    let mut program = args.next()?;
    if base_name(&program) == "env" {
        program = loop {
            let arg = args.next()?;
            if matches!(arg.as_str(), "-u" | "--unset" | "-C" | "--chdir") {
                args.next()?;
            } else if !arg.starts_with('-') && !arg.contains('=') {
                break arg;
            }
        };
    }
    if matches!(base_name(&program), "sh" | "bash" | "dash" | "zsh" | "fish") {
        return None;
    }
    Some(program)
}

/// Splits an `Exec` value into arguments. Double quotes group an argument; inside them `\"`, `` \` ``, `\$` and `\\`
/// stand for the character. `None` when a quote is left open.
fn split(exec: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut arg = String::new();
    let mut started = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if started {
                    args.push(std::mem::take(&mut arg));
                    started = false;
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => arg.push(chars.next()?),
                        other => arg.push(other),
                    }
                }
            }
            other => {
                started = true;
                arg.push(other);
            }
        }
    }
    if started {
        args.push(arg);
    }
    Some(args)
}

fn base_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// An absolute program as it is, a bare name looked up in `dirs`; a relative path cannot be told.
fn resolve(host: &Host, program: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if program.starts_with('/') {
        return Some(PathBuf::from(program));
    }
    if program.contains('/') {
        return None;
    }
    dirs.iter()
        .map(|dir| dir.join(program))
        .find(|path| host.file_type(path) == Some(FileType::File { executable: true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};

    fn entry(name: &str, exec: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}")
    }

    #[test]
    fn reads_the_main_group_of_application_entries_only() {
        let text = "# comment\n[Desktop Entry]\nVersion=1.5\nType=Application\nName=Firefox\nName[de]=Feuerfuchs\n\
                    Icon=firefox\nExec=firefox %u\nNoDisplay = false\n\n[Desktop Action new-window]\nName=New Window\n\
                    Exec=firefox --new-window %u\n";
        assert_eq!(
            parse(text),
            Some(Entry {
                name: "Firefox".into(),
                icon: Some("firefox".into()),
                exec: Some("firefox %u".into()),
                hidden: false,
            })
        );
        assert_eq!(
            parse("[Desktop Entry]\nType=Link\nName=Docs\nURL=https://example.org\n"),
            None
        );
        assert_eq!(parse("[Desktop Entry]\nType=Application\nExec=nameless\n"), None);
        let hidden = |extra: &str| parse(&entry("Handler", "x", extra)).unwrap().hidden;
        assert!(hidden("NoDisplay=true\n"));
        assert!(hidden("Hidden=true\n"));
        assert!(!hidden(""));
    }

    #[test]
    fn exec_lines_name_the_program_they_start() {
        let program_of = |line: &str| program(&parse(&entry("App", line, "")).unwrap().exec.unwrap());
        let program_of = |line: &str| program_of(line).map(PathBuf::from);
        let some = |path: &str| Some(PathBuf::from(path));
        assert_eq!(
            program_of(r#""/opt/T3 Code (Beta)/t3code" %U"#),
            some("/opt/T3 Code (Beta)/t3code")
        );
        assert_eq!(
            program_of(
                "env BAMF_DESKTOP_FILE_HINT=/var/lib/snapd/desktop/applications/firefox_firefox.desktop \
                 /snap/bin/firefox %u"
            ),
            some("/snap/bin/firefox")
        );
        assert_eq!(program_of("/usr/bin/env -u GTK_IM_MODULE LANG=C gimp %F"), some("gimp"));
        // In the file, `\\"` is a string escape (`\\` → `\`) and then an Exec escape (`\"` → `"`).
        assert_eq!(
            program_of(r#""/opt/say \\"hi\\"/run" %f"#),
            some(r#"/opt/say "hi"/run"#)
        );
        assert_eq!(
            program_of(r"my\sapp --flag"),
            some("my"),
            "\\s is a space, so it splits"
        );
        assert_eq!(program_of(r#"sh -c "cd ~/game && ./run""#), None);
        assert_eq!(program_of(r#""/opt/unterminated %U"#), None);
        assert_eq!(program_of(""), None);
    }

    #[test]
    fn the_first_file_with_an_id_wins_and_hides_the_rest() {
        let fixture = Fixture::new();
        let local = format!("{HOME}/.local/share/applications");
        fixture.file(
            &format!("{local}/org.godotengine.Godot.desktop"),
            entry(
                "Godot (my copy)",
                "/usr/bin/flatpak run org.godotengine.Godot",
                "Icon=org.godotengine.Godot\n",
            ),
        );
        let exported = "/var/lib/flatpak/exports/share/applications/org.godotengine.Godot.desktop";
        fixture.file(
            exported,
            entry("Godot Engine", "/usr/bin/flatpak run org.godotengine.Godot", ""),
        );
        fixture.file(
            "/usr/share/applications/kde4/okular.desktop",
            entry("Okular", "okular %U", ""),
        );
        fixture.file("/usr/share/applications/readme.txt", "not an entry");
        fixture.file(
            "/usr/share/applications/url.desktop",
            "[Desktop Entry]\nType=Link\nName=Web\n",
        );
        fixture.executable("/usr/bin/okular", "");
        let host = fixture.host_with_vars(
            FakeRunner::new(),
            &[("XDG_DATA_DIRS", "/var/lib/flatpak/exports/share:/usr/share/")],
        );

        let context = ScanContext::default();
        let launchers = launchers(&host, &[PathBuf::from("/usr/bin")], &context);
        let ids: Vec<&str> = launchers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["org.godotengine.Godot.desktop", "kde4-okular.desktop"]);

        let godot = &launchers[0];
        assert_eq!(godot.name, "Godot (my copy)");
        assert_eq!(
            godot.path,
            PathBuf::from(format!("{local}/org.godotengine.Godot.desktop"))
        );
        assert_eq!(godot.shadows, [PathBuf::from(exported)]);
        assert_eq!(godot.program, Some(PathBuf::from("/usr/bin/flatpak")));
        let okular = &launchers[1];
        assert_eq!(
            okular.program,
            Some(PathBuf::from("/usr/bin/okular")),
            "bare names are looked up"
        );
        assert!(context.into_warnings().is_empty());
    }

    #[test]
    fn without_xdg_data_dirs_the_standard_folders_are_read() {
        let fixture = Fixture::new();
        fixture.file("/usr/local/share/applications/tool.desktop", entry("Tool", "tool", ""));
        fixture.file(
            "/usr/share/applications/tool.desktop",
            entry("Packaged tool", "tool", ""),
        );
        let host = fixture.host(FakeRunner::new());

        let launchers = launchers(&host, &[], &ScanContext::default());
        assert_eq!(launchers.len(), 1);
        assert_eq!(launchers[0].name, "Tool");
        assert_eq!(launchers[0].program, None, "tool is not installed");
    }
}
