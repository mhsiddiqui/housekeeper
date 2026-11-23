//! Small parsing helpers shared by sources.

/// Parses sizes printed for humans, such as `462.0 MB`, `743.9 kB`, `1.7 GB`, `12 bytes` or `3.2 MiB`.
///
/// Decimal units are powers of 1000, binary units (`KiB`, `MiB`) powers of 1024.
pub fn parse_human_size(text: &str) -> Option<u64> {
    let text = text.trim().replace('\u{a0}', " ");
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.replace(',', ".").parse().ok()?;
    let factor: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" | "byte" | "bytes" => 1.0,
        "kb" | "k" => 1e3,
        "mb" | "m" => 1e6,
        "gb" | "g" => 1e9,
        "tb" | "t" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * factor).round() as u64)
}

/// Splits `KEY=value` lines (os-release, shell-style config), removing matching quotes and common escapes.
/// Comments and blank lines are skipped.
pub fn key_values(text: &str) -> impl Iterator<Item = (&str, String)> {
    text.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        Some((key.trim(), unquote(value.trim())))
    })
}

fn unquote(value: &str) -> String {
    let inner = match value.as_bytes() {
        [b'"', .., b'"'] | [b'\'', .., b'\''] if value.len() >= 2 => &value[1..value.len() - 1],
        _ => value,
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(parse_human_size("462.0 MB"), Some(462_000_000));
        assert_eq!(parse_human_size("743.9 kB"), Some(743_900));
        assert_eq!(parse_human_size("1.7 GB"), Some(1_700_000_000));
        assert_eq!(parse_human_size("12 bytes"), Some(12));
        assert_eq!(parse_human_size("2 MiB"), Some(2 * 1024 * 1024));
        assert_eq!(parse_human_size("1,5\u{a0}GB"), Some(1_500_000_000));
        assert_eq!(parse_human_size("unknown"), None);
        assert_eq!(parse_human_size(""), None);
    }

    #[test]
    fn key_value_lines() {
        let text = "# comment\nNAME=\"Fedora Linux\"\nID=fedora\n\nVARIANT='Silverblue \\\"edition\\\"'\n";
        let pairs: Vec<_> = key_values(text).collect();
        assert_eq!(
            pairs,
            [
                ("NAME", "Fedora Linux".to_string()),
                ("ID", "fedora".to_string()),
                ("VARIANT", "Silverblue \"edition\"".to_string()),
            ]
        );
    }
}
