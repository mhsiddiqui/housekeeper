//! The application menu: desktop entries, as the freedesktop.org Desktop Entry Specification (1.5) describes them.
//!
//! Entries live in `applications/` under `$XDG_DATA_HOME` and each `$XDG_DATA_DIRS` entry, most important first. An
//! entry's id is its path below `applications/` with `/` turned into `-`; the first file with an id is the one menus
//! use, and it hides the others (a user's copy in `~/.local/share/applications` hides the system's).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use housekeeper_core::{FileType, Host, Launcher, ScanContext};

use crate::xdg::data_dirs;

/// How deep subfolders of `applications/` are read (`kde4/`, `wine/Programs/…`).
const MAX_DEPTH: usize = 4;

/// Every application entry, one per id, the one that takes precedence. `command_dirs` is where a program named
/// without a path (`Exec=gimp`) is looked for.
pub(crate) fn launchers(host: &Host, command_dirs: &[PathBuf], scan: &ScanContext<'_>) -> Vec<Launcher> {
    let mut files: Vec<(String, Vec<PathBuf>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for dir in data_dirs(host).into_iter().map(|dir| dir.join("applications")) {
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

/// The program an `Exec` command line starts, as written: past `env` and its variables, `%%` read as `%`. `None` when
/// the line cannot be read or runs a shell, whose script could start anything.
fn program(exec: &str) -> Option<String> {
    let (program, _) = command(exec)?;
    if matches!(base_name(&program), "sh" | "bash" | "dash" | "zsh" | "fish") {
        return None;
    }
    Some(program.replace("%%", "%"))
}

/// The program an `Exec` command line starts, past `env` and its variables, and the arguments it is given.
fn command(exec: &str) -> Option<(String, Vec<String>)> {
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
    Some((program, args.collect()))
}

/// The `Icon` of an application entry's main group, as written.
pub(crate) fn icon_of(text: &str) -> Option<String> {
    parse(text)?.icon
}

/// A menu entry for the AppImage at `program` (logical, absolute), made from `entry`, the one the AppImage carries:
/// every `Exec` starts `program` with the arguments it had (past `env` and its variables), `TryExec` and
/// `DBusActivatable` go (nothing else is installed to answer them), and the main group's `Icon` is `icon` when there is
/// one. Without a usable entry (`None`, or not an application's), a plain one named `name`.
pub(crate) fn appimage_entry(entry: Option<&str>, program: &Path, name: &str, icon: Option<&Path>) -> String {
    let header = format!(
        "# Made by Housekeeper for {}; removing the AppImage with Housekeeper removes this too.\n",
        program.display()
    );
    let icon_line = icon.map(|icon| format!("Icon={}", escape(&icon.to_string_lossy())));
    let Some(entry) = entry.filter(|entry| parse(entry).is_some()) else {
        let mut text = format!(
            "{header}[Desktop Entry]\nType=Application\nName={}\nExec={}\nTerminal=false\n",
            escape(name),
            escape(&exec_arg(&program.to_string_lossy(), true))
        );
        if let Some(line) = icon_line {
            text.push_str(&line);
            text.push('\n');
        }
        return text;
    };
    let mut out = header;
    let mut group = "";
    for line in entry.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            group = trimmed;
            out.push_str(line);
            out.push('\n');
            if group == "[Desktop Entry]" {
                if let Some(line) = &icon_line {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            continue;
        }
        let key = trimmed.split_once('=').map(|(key, _)| key.trim_end());
        match key {
            Some("TryExec" | "DBusActivatable") => continue,
            Some("Icon") if group == "[Desktop Entry]" && icon_line.is_some() => continue,
            Some("Exec") => {
                let value = trimmed.split_once('=').map_or("", |(_, value)| value.trim_start());
                let args = command(&unescape(value)).map(|(_, args)| args).unwrap_or_default();
                let exec: Vec<String> = std::iter::once(exec_arg(&program.to_string_lossy(), true))
                    .chain(args.iter().map(|arg| exec_arg(arg, false)))
                    .collect();
                out.push_str("Exec=");
                out.push_str(&escape(&exec.join(" ")));
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// One argument of an `Exec` line: in double quotes when it holds a character the specification reserves, with `"`,
/// `` ` ``, `$` and `\` escaped inside them. A `literal` path has its `%` doubled, which would otherwise start a field
/// code; an argument copied from another `Exec` keeps its field codes.
fn exec_arg(arg: &str, literal: bool) -> String {
    let text = if literal {
        arg.replace('%', "%%")
    } else {
        arg.to_string()
    };
    let reserved = |c: char| " \t\n\"'\\><~|&;$*?#()`".contains(c);
    if !text.contains(reserved) {
        return text;
    }
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// A string value for a desktop entry: `\`, newlines, tabs and carriage returns escaped, as [`unescape`] reads them.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
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

    #[test]
    fn an_appimage_entry_starts_the_appimage_wherever_it_is_and_shows_its_icon() {
        let appimage = Path::new(r#"/home/user/Applications/My "100%" App $1\.AppImage"#);
        let icon = Path::new("/home/user/.local/share/icons/appimage/my-app/icon.png");
        let carried = "[Desktop Entry]\nType=Application\nName=My App\nExec=env LANG=C app --open %U\n\
                       TryExec=app\nIcon=app\nDBusActivatable=true\n\n[Desktop Action new]\nName=New\nExec=app --new\n";
        let made = appimage_entry(Some(carried), appimage, "My", Some(icon));
        let read = parse(&made).unwrap();
        assert_eq!(read.name, "My App");
        assert_eq!(read.icon.as_deref(), Some(icon.to_str().unwrap()));
        let exec = read.exec.unwrap();
        assert_eq!(program(&exec).as_deref(), appimage.to_str());
        assert!(exec.ends_with(" --open %U"), "{exec}");
        assert!(!made.contains("TryExec") && !made.contains("DBusActivatable"), "{made}");
        assert!(made.contains("[Desktop Action new]\nName=New\nExec=\""), "{made}");

        let plain = parse(&appimage_entry(
            None,
            Path::new("/home/user/Applications/x.AppImage"),
            "X",
            None,
        ))
        .unwrap();
        assert_eq!(
            (plain.name.as_str(), plain.exec.as_deref(), plain.icon),
            ("X", Some("/home/user/Applications/x.AppImage"), None)
        );
    }
}
