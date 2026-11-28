//! `housekeeper history`: what Housekeeper changed on this computer.

use std::io::{self, Write};

use housekeeper_core::history::{Change, ChangeOutcome, HistoryEntry};
use housekeeper_core::text::utc_timestamp;

use crate::table::Table;

/// One line per change, oldest first, or a sentence when there is none.
pub fn render(entries: &[HistoryEntry], out: &mut impl Write) -> io::Result<()> {
    if entries.is_empty() {
        return writeln!(out, "Housekeeper has not changed anything on this computer yet.");
    }
    let mut table = Table::new(["WHEN (UTC)", "CHANGE", "ITEM", "ID", "OUTCOME"]);
    for entry in entries {
        table.row([
            // `2026-11-26 14:03`.
            utc_timestamp(entry.at)[..16].replace('T', " "),
            match entry.change {
                Change::Remove => "remove".to_string(),
                Change::Install => "install".to_string(),
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
