//! Plain-text output.

use std::io::{self, Write};

/// Left-aligned columns separated by two spaces. The last column is not padded.
pub struct Table<const N: usize> {
    header: [String; N],
    rows: Vec<[String; N]>,
}

impl<const N: usize> Table<N> {
    pub fn new(header: [&str; N]) -> Self {
        Self {
            header: header.map(str::to_string),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: [String; N]) {
        self.rows.push(cells.map(|cell| cell.replace(['\n', '\t'], " ")));
    }

    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let mut widths = [0; N];
        for row in std::iter::once(&self.header).chain(&self.rows) {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.chars().count());
            }
        }
        for row in std::iter::once(&self.header).chain(&self.rows) {
            let mut line = String::new();
            for (i, (cell, width)) in row.iter().zip(widths).enumerate() {
                if i + 1 == N {
                    line.push_str(cell);
                } else {
                    line.push_str(cell);
                    line.extend(std::iter::repeat_n(' ', width - cell.chars().count() + 2));
                }
            }
            writeln!(out, "{}", line.trim_end())?;
        }
        Ok(())
    }
}

/// Decimal units, as file managers on Linux show them.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(171_800_000), "171.8 MB");
        assert_eq!(human_size(1_700_000_000), "1.7 GB");
    }

    #[test]
    fn columns_align_on_the_widest_cell_including_non_ascii() {
        let mut table = Table::new(["NAME", "SOURCE"]);
        table.row(["Café".into(), "flatpak".into()]);
        table.row(["Godot Engine".into(), "snap".into()]);
        let mut out = Vec::new();
        table.write(&mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "NAME          SOURCE\nCafé          flatpak\nGodot Engine  snap\n"
        );
    }
}
