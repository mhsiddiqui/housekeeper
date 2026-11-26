//! `housekeeper history`: what Housekeeper changed on this computer.

use std::io::{self, Write};

use housekeeper_core::history::{Change, ChangeOutcome, HistoryEntry};

use crate::table::Table;

/// One line per change, oldest first, or a sentence when there is none.
pub fn render(entries: &[HistoryEntry], out: &mut impl Write) -> io::Result<()> {
    if entries.is_empty() {
        return writeln!(out, "Housekeeper has not changed anything on this computer yet.");
    }
    let mut table = Table::new(["WHEN (UTC)", "CHANGE", "ITEM", "ID", "OUTCOME"]);
    for entry in entries {
        table.row([
            utc(entry.at),
            match entry.change {
                Change::Remove => "remove".to_string(),
            },
            entry.title.clone(),
            entry.item.clone(),
            match entry.outcome {
                ChangeOutcome::Done => "done",
                ChangeOutcome::Failed => "failed",
                ChangeOutcome::NotDone => "not done",
            }
            .to_string(),
        ]);
    }
    table.write(out)
}

/// `2026-11-26 14:03` for seconds since the Unix epoch, in UTC.
fn utc(at: i64) -> String {
    let (days, seconds) = (at.div_euclid(86_400), at.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60
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
    fn times_are_printed_as_utc_dates() {
        assert_eq!(utc(0), "1970-01-01 00:00");
        assert_eq!(utc(1_794_206_843), "2026-11-09 06:47");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00", "a leap day");
        assert_eq!(utc(-1), "1969-12-31 23:59");
    }
}
