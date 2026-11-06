//! The start of a CSV result, to show as a table in the launcher without opening another
//! program. Only reads: the file is one `output_target` allowed.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use serde::Serialize;

/// Rows sent to the window: what the panel shows, as for a table output.
pub const ROWS: usize = 50;
/// Columns and characters per cell sent to the window; the rest is in the file.
const COLUMNS: usize = 60;
const CELL: usize = 300;
/// Bytes read for the rows shown.
const HEAD: u64 = 1 << 20;
/// Larger files are not counted through: their total is unknown.
const COUNT_LIMIT: u64 = 256 << 20;

#[derive(Debug, Serialize, PartialEq)]
pub struct TablePreview {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Data rows in the whole file (the header row not counted); `None` for a very large file.
    pub total: Option<u64>,
}

/// Whether a result file is one the launcher shows as a table.
pub fn is_table_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("tsv"))
}

pub fn read(path: &Path) -> anyhow::Result<TablePreview> {
    if !is_table_file(path) {
        anyhow::bail!("only CSV results are shown as a table");
    }
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut head = Vec::new();
    BufReader::new(file).take(HEAD).read_to_end(&mut head)?;
    let text = decode(&head);
    let tab = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tsv"));
    let delimiter = if tab { '\t' } else { sniff(&text) };
    // A file cut by the byte limit ends mid-row: that row is left out.
    let complete = size <= HEAD;
    let mut records = parse(&text, delimiter, ROWS + 2);
    if !complete && records.len() > ROWS + 1 {
        records.truncate(ROWS + 1);
    }
    let mut records = records.into_iter();
    let columns = records.next().map(clip).unwrap_or_default();
    let rows: Vec<Vec<String>> = records.take(ROWS).map(clip).collect();
    let total = if complete {
        Some(count(&head).saturating_sub(1))
    } else if size <= COUNT_LIMIT {
        let mut all = Vec::new();
        File::open(path)?.read_to_end(&mut all)?;
        Some(count(&all).saturating_sub(1))
    } else {
        None
    };
    Ok(TablePreview {
        columns,
        rows,
        total,
    })
}

fn clip(row: Vec<String>) -> Vec<String> {
    row.into_iter()
        .take(COLUMNS)
        .map(|cell| {
            if cell.chars().count() > CELL {
                cell.chars().take(CELL).chain(['…']).collect()
            } else {
                cell
            }
        })
        .collect()
}

/// UTF-8 (with or without the mark Excel adds), else Windows-1252, which Excel writes for
/// "CSV" on a Western European Windows.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        // Cut by the byte limit inside a character: the valid part.
        Err(e) if e.error_len().is_none() => {
            String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned()
        }
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// The delimiter of the first line, outside quotes: `;` where Excel uses the comma for decimals.
fn sniff(text: &str) -> char {
    let mut counts = [0usize; 3];
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' if !quoted => break,
            ',' if !quoted => counts[0] += 1,
            ';' if !quoted => counts[1] += 1,
            '\t' if !quoted => counts[2] += 1,
            _ => {}
        }
    }
    let best = (0..3).max_by_key(|&i| (counts[i], 2 - i)).unwrap_or(0);
    if counts[best] == 0 {
        ','
    } else {
        [',', ';', '\t'][best]
    }
}

/// Up to `limit` records, with quoted cells (`"a, ""b"""`) and line breaks inside quotes.
fn parse(text: &str, delimiter: char, limit: usize) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else if c == '\r' && chars.peek() == Some(&'\n') {
                // A line break inside a cell, as Windows writes it: one break.
            } else {
                cell.push(c);
            }
        } else if c == '"' && cell.is_empty() {
            quoted = true;
        } else if c == delimiter {
            record.push(std::mem::take(&mut cell));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            record.push(std::mem::take(&mut cell));
            if !(record.len() == 1 && record[0].is_empty()) {
                records.push(std::mem::take(&mut record));
                if records.len() >= limit {
                    return records;
                }
            }
            record.clear();
        } else {
            cell.push(c);
        }
    }
    if !cell.is_empty() || !record.is_empty() {
        record.push(cell);
        records.push(record);
    }
    records
}

/// Records in the file, counted on the bytes (quotes respected, empty lines left out).
fn count(bytes: &[u8]) -> u64 {
    let mut n = 0u64;
    let mut quoted = false;
    let mut blank = true;
    for &b in bytes {
        match b {
            b'"' => {
                quoted = !quoted;
                blank = false;
            }
            b'\n' if !quoted => {
                if !blank {
                    n += 1;
                }
                blank = true;
            }
            b'\r' => {}
            _ => blank = false,
        }
    }
    if !blank {
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        (tmp, p)
    }

    #[test]
    fn a_csv_as_excel_writes_it() {
        let (_t, p) = write(
            "r.csv",
            "\u{feff}Name;Amount;Note\r\nRossi;1,5;\"a; b\"\r\nBianchi;2;\"two\r\nlines\"\r\n"
                .as_bytes(),
        );
        let t = read(&p).unwrap();
        assert_eq!(t.columns, ["Name", "Amount", "Note"]);
        assert_eq!(
            t.rows,
            [
                vec!["Rossi", "1,5", "a; b"],
                vec!["Bianchi", "2", "two\nlines"]
            ]
        );
        assert_eq!(t.total, Some(2));
    }

    #[test]
    fn quotes_commas_and_old_encodings() {
        let (_t, p) = write("r.csv", b"a,b\n\"x, \"\"y\"\"\",caf\xe9 \x80\n\n");
        let t = read(&p).unwrap();
        assert_eq!(t.rows, [vec!["x, \"y\"", "café €"]]);
        assert_eq!(t.total, Some(1));
        let (_t, p) = write("r.tsv", b"a\tb,c\n1\t2,3\n");
        assert_eq!(read(&p).unwrap().rows, [vec!["1", "2,3"]]);
    }

    #[test]
    fn a_long_file_shows_its_start_and_counts_the_rest() {
        let mut text = String::from("n,text\n");
        for i in 0..100_000 {
            text.push_str(&format!("{i},\"row {i}\"\n"));
        }
        let (_t, p) = write("big.csv", text.as_bytes());
        let t = read(&p).unwrap();
        assert_eq!(t.rows.len(), ROWS);
        assert_eq!(t.rows[ROWS - 1], ["49", "row 49"]);
        assert_eq!(t.total, Some(100_000));
    }

    #[test]
    fn only_csv_files() {
        let (_t, p) = write("r.xlsx", b"a,b\n");
        assert!(read(&p).is_err());
        let (_t, p) = write("empty.csv", b"");
        assert_eq!(
            read(&p).unwrap(),
            TablePreview {
                columns: vec![],
                rows: vec![],
                total: Some(0)
            }
        );
    }
}
