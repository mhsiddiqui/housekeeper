//! Small parsing and formatting helpers shared by sources and services.

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

/// `2026-11-26T14:03:09` for seconds since the Unix epoch, in UTC (ISO 8601 without a zone, as the freedesktop Trash
/// writes dates).
pub fn utc_timestamp(secs: i64) -> String {
    let (days, seconds) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
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

    #[test]
    fn utc_timestamps() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00");
        assert_eq!(utc_timestamp(1_794_206_843), "2026-11-09T06:47:23");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29T00:00:00", "a leap day");
        assert_eq!(utc_timestamp(-1), "1969-12-31T23:59:59");
    }
}
