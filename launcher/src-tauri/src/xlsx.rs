//! The first sheet of an Excel result (`.xlsx`), as a table: the values Excel would show, dates
//! as dates, without opening Excel. Only reads, and only what the table needs: the workbook's
//! first visible sheet, its shared strings and which cell styles are dates.

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use anyhow::Context;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use crate::table::{COLUMNS, ROWS, TablePreview, clip};

/// Bytes read from one part of the file once unpacked: a report is far smaller, and a file that
/// unpacks to more is not worth the memory (or is built to fill it).
const PART: u64 = 64 << 20;

pub fn read(path: &Path) -> anyhow::Result<TablePreview> {
    let file = File::open(path)?;
    let mut zip =
        zip::ZipArchive::new(BufReader::new(file)).context("this is not an Excel (.xlsx) file")?;
    let workbook =
        part(&mut zip, "xl/workbook.xml")?.context("this is not an Excel (.xlsx) file")?;
    let (id, date1904) = first_sheet(&workbook)?;
    let rels = part(&mut zip, "xl/_rels/workbook.xml.rels")?.unwrap_or_default();
    let sheet = target(&rels, &id)?.unwrap_or_else(|| "xl/worksheets/sheet1.xml".into());
    let strings = match part(&mut zip, "xl/sharedStrings.xml")? {
        Some(xml) => shared_strings(&xml)?,
        None => Vec::new(),
    };
    let styles = match part(&mut zip, "xl/styles.xml")? {
        Some(xml) => date_styles(&xml)?,
        None => Vec::new(),
    };
    let xml = part(&mut zip, &sheet)?.context("the workbook's first sheet is missing")?;
    rows(
        &xml,
        &Book {
            strings,
            styles,
            date1904,
        },
    )
}

/// One part of the file, or `None` when the workbook has no such part.
fn part<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e).context("this Excel file could not be read"),
    };
    let mut bytes = Vec::new();
    entry.take(PART + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > PART {
        anyhow::bail!("this sheet is too large to show here");
    }
    Ok(Some(bytes))
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok())
        .map(|v| v.into_owned())
}

/// Text and the references in it (`&amp;`, `&#10;`), which the reader gives apart.
fn push_text(out: &mut String, event: &Event) {
    match event {
        Event::Text(t) => out.push_str(&t.xml10_content()),
        Event::CData(t) => out.push_str(t),
        Event::GeneralRef(r) => {
            if let Ok(Some(c)) = r.resolve_char_ref() {
                out.push(c);
            } else {
                out.push_str(match r.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
        }
        _ => {}
    }
}

/// The relationship id of the first sheet that is not hidden, and whether dates count from 1904
/// (old Mac workbooks).
fn first_sheet(xml: &[u8]) -> anyhow::Result<(String, bool)> {
    let mut reader = Reader::from_reader(xml);
    let mut date1904 = false;
    let mut first = None;
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "workbookPr" => {
                    date1904 = matches!(attr(&e, "date1904").as_deref(), Some("1" | "true"));
                }
                "sheet" if first.is_none() => {
                    let hidden = attr(&e, "state").is_some_and(|s| s != "visible");
                    if !hidden {
                        first = attr(&e, "id");
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((
        first.context("the workbook has no sheet to show")?,
        date1904,
    ))
}

/// Where the sheet with relationship `id` is, as a name inside the file.
fn target(rels: &[u8], id: &str) -> anyhow::Result<Option<String>> {
    let mut reader = Reader::from_reader(rels);
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "Relationship" => {
                if attr(&e, "Id").as_deref() == Some(id)
                    && let Some(t) = attr(&e, "Target")
                {
                    return Ok(Some(match t.strip_prefix('/') {
                        Some(absolute) => absolute.to_string(),
                        None => format!("xl/{t}"),
                    }));
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}

/// The workbook's strings, which cells name by number. Rich text is joined; the pronunciation
/// guides of Japanese text (`rPh`) are left out, as Excel shows them only on request.
fn shared_strings(xml: &[u8]) -> anyhow::Result<Vec<String>> {
    let mut reader = Reader::from_reader(xml);
    let mut out = Vec::new();
    let mut current = String::new();
    let (mut in_t, mut in_rph) = (false, false);
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) => match e.local_name().as_ref() {
                "si" => current.clear(),
                "t" => in_t = true,
                "rPh" => in_rph = true,
                _ => {}
            },
            Event::Empty(e) if e.local_name().as_ref() == "si" => out.push(String::new()),
            Event::End(e) => match e.local_name().as_ref() {
                "si" => out.push(std::mem::take(&mut current)),
                "t" => in_t = false,
                "rPh" => in_rph = false,
                _ => {}
            },
            Event::Eof => break,
            _ if in_t && !in_rph => push_text(&mut current, &event),
            _ => {}
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum When {
    Date,
    Time,
    DateTime,
}

/// For each cell style (`s="3"` on a cell), whether it shows a number as a date or a time.
fn date_styles(xml: &[u8]) -> anyhow::Result<Vec<Option<When>>> {
    let mut reader = Reader::from_reader(xml);
    let mut custom: Vec<(u32, Option<When>)> = Vec::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut in_cell_xfs = false;
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "numFmt" => {
                    if let (Some(id), Some(code)) = (attr(&e, "numFmtId"), attr(&e, "formatCode"))
                        && let Ok(id) = id.parse()
                    {
                        custom.push((id, format_kind(&code)));
                    }
                }
                "cellXfs" => in_cell_xfs = true,
                "xf" if in_cell_xfs => {
                    ids.push(
                        attr(&e, "numFmtId")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0),
                    );
                }
                _ => {}
            },
            Event::End(e) if e.local_name().as_ref() == "cellXfs" => in_cell_xfs = false,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(ids
        .into_iter()
        .map(|id| match custom.iter().find(|(c, _)| *c == id) {
            Some((_, kind)) => *kind,
            None => built_in_kind(id),
        })
        .collect())
}

/// Excel's own number formats that are dates or times.
fn built_in_kind(id: u32) -> Option<When> {
    match id {
        14..=17 | 27..=31 | 34..=36 | 50..=58 => Some(When::Date),
        18..=21 | 32 | 33 | 45..=47 => Some(When::Time),
        22 => Some(When::DateTime),
        _ => None,
    }
}

/// Whether a format code such as `dd/mm/yyyy hh:mm` shows a date, a time or both. Quoted text,
/// escaped characters and `[...]` (colours, locales) are not part of it; a duration (`[h]:mm`)
/// is a number of hours, not a time of day, so it stays a number.
fn format_kind(code: &str) -> Option<When> {
    let section = code.split(';').next().unwrap_or(code);
    if section.contains("[h") || section.contains("[m") || section.contains("[s") {
        return None;
    }
    let mut plain = String::new();
    let mut chars = section.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                }
            }
            '[' => {
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                chars.next();
            }
            c => plain.push(c.to_ascii_lowercase()),
        }
    }
    let date = plain.contains('y') || plain.contains('d');
    let time = plain.contains('h') || plain.contains('s');
    match (date, time) {
        (true, true) => Some(When::DateTime),
        (true, false) => Some(When::Date),
        (false, true) => Some(When::Time),
        (false, false) => None,
    }
}

struct Book {
    strings: Vec<String>,
    styles: Vec<Option<When>>,
    date1904: bool,
}

/// The sheet's rows: the first one as the column names, as for a CSV. Empty rows are left out.
fn rows(xml: &[u8], book: &Book) -> anyhow::Result<TablePreview> {
    let mut reader = Reader::from_reader(xml);
    let mut kept: Vec<Vec<String>> = Vec::new();
    let mut total = 0u64;
    let mut row: Vec<String> = Vec::new();
    // The cell being read: its column, type and style, and the text of its value.
    let mut col = 0usize;
    let mut kind = String::new();
    let mut style = 0usize;
    let mut value = String::new();
    let (mut in_value, mut in_rph) = (false, false);
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                match e.local_name().as_ref() {
                    "row" => {
                        row.clear();
                        col = 0;
                    }
                    "c" => {
                        col = attr(e, "r").and_then(|r| column(&r)).unwrap_or(col);
                        kind = attr(e, "t").unwrap_or_default();
                        style = attr(e, "s").and_then(|s| s.parse().ok()).unwrap_or(0);
                        value.clear();
                        if empty {
                            col += 1;
                        }
                    }
                    "v" | "t" if !empty => in_value = true,
                    "rPh" if !empty => in_rph = true,
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                "v" | "t" => in_value = false,
                "rPh" => in_rph = false,
                "c" => {
                    if col < COLUMNS {
                        if row.len() <= col {
                            row.resize(col + 1, String::new());
                        }
                        row[col] = shown(&kind, style, &value, book);
                    }
                    col += 1;
                }
                "row" if row.iter().any(|c| !c.is_empty()) => {
                    total += 1;
                    if kept.len() <= ROWS {
                        kept.push(clip(std::mem::take(&mut row)));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ if in_value && !in_rph => push_text(&mut value, &event),
            _ => {}
        }
    }
    let mut kept = kept.into_iter();
    Ok(TablePreview {
        columns: kept.next().unwrap_or_default(),
        rows: kept.collect(),
        total: Some(total.saturating_sub(1)),
    })
}

/// `B12` → column 1 (from 0).
fn column(reference: &str) -> Option<usize> {
    let letters: Vec<u8> = reference
        .bytes()
        .take_while(u8::is_ascii_alphabetic)
        .collect();
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let n = letters.iter().fold(0usize, |n, b| {
        n * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize
    });
    Some(n - 1)
}

/// A cell as Excel shows it, near enough: the string it names, `TRUE`, the error, or the number
/// (as a date when its style is one).
fn shown(kind: &str, style: usize, value: &str, book: &Book) -> String {
    match kind {
        "s" => value
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|i| book.strings.get(i).cloned())
            .unwrap_or_default(),
        "b" => if value.trim() == "1" { "TRUE" } else { "FALSE" }.to_string(),
        // Text of a formula, text written in the cell, an error (`#DIV/0!`), a date as text.
        "str" | "inlineStr" | "e" | "d" => value.to_string(),
        _ => match book.styles.get(style).copied().flatten() {
            Some(when) => value
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(|serial| date_text(serial, when, book.date1904))
                .unwrap_or_else(|| value.to_string()),
            None => number_text(value),
        },
    }
}

/// A number to the 15 significant digits Excel shows: files keep the float's last digits
/// (`99.90000000000001` for 99.9). Very large and very small numbers stay as written.
fn number_text(value: &str) -> String {
    let Ok(n) = value.trim().parse::<f64>() else {
        return value.to_string();
    };
    if n == 0.0 {
        return "0".into();
    }
    let magnitude = n.abs().log10().floor() as i32;
    if !(-9..15).contains(&magnitude) {
        return value.to_string();
    }
    let decimals = (14 - magnitude).max(0) as usize;
    let text = format!("{n:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

/// An Excel date number as `2026-11-09`, `14:30` or `2026-11-09 14:30` (seconds when there are).
fn date_text(serial: f64, when: When, date1904: bool) -> Option<String> {
    if !(0.0..=2_958_465.0).contains(&serial) {
        return None;
    }
    let mut days = serial.floor() as i64;
    let mut seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    if seconds >= 86_400 {
        days += 1;
        seconds -= 86_400;
    }
    let date = if date1904 {
        civil(days + days_from_civil(1904, 1, 1))
    } else if days == 60 {
        // Excel counts a 29 February 1900 that never was, as Lotus 1-2-3 did.
        (1900, 2, 29)
    } else if days < 60 {
        civil(days + days_from_civil(1899, 12, 31))
    } else {
        civil(days + days_from_civil(1899, 12, 30))
    };
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    let time = if s == 0 {
        format!("{h:02}:{m:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    };
    let day = format!("{:04}-{:02}-{:02}", date.0, date.1, date.2);
    Some(match when {
        When::Date => day,
        When::Time => time,
        When::DateTime if seconds == 0 => day,
        When::DateTime => format!("{day} {time}"),
    })
}

/// Days since 1970-01-01 (H. Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// A workbook as Excel and openpyxl write one: shared strings, a date style, a hidden first
    /// sheet, a gap between cells and a cell far to the right.
    fn workbook(sheet: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("r.xlsx");
        let mut zip = zip::ZipWriter::new(File::create(&p).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let parts = [
            (
                "xl/workbook.xml",
                r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr/><sheets><sheet name="Hidden" sheetId="1" state="hidden" r:id="rId1"/><sheet name="Report" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="x" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="x" Target="/xl/worksheets/sheet2.xml"/></Relationships>"#,
            ),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>Name</t></si><si><t>Date</t></si><si><r><t>Ros</t></r><r><t xml:space="preserve">si &amp; co</t></r><rPh><t>ロ</t></rPh></si><si/></sst>"#,
            ),
            (
                "xl/styles.xml",
                r#"<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="dd/mm/yyyy\ hh:mm"/></numFmts><cellStyleXfs><xf numFmtId="14"/></cellStyleXfs><cellXfs><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="164"/><xf numFmtId="4"/></cellXfs></styleSheet>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                "<worksheet><sheetData><row><c><v>1</v></c></row></sheetData></worksheet>",
            ),
            ("xl/worksheets/sheet2.xml", sheet),
        ];
        for (name, text) in parts {
            zip.start_file(name, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        (tmp, p)
    }

    #[test]
    fn the_first_visible_sheet_as_excel_shows_it() {
        let (_t, p) = workbook(
            r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="D1" t="inlineStr"><is><t>Paid</t></is></c></row>
            <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2" s="1"><v>46335</v></c><c r="C2" s="3"><v>1234.5</v></c><c r="D2" t="b"><v>1</v></c></row>
            <row r="3"/>
            <row r="4"><c r="A4" t="str"><f>A2</f><v>x</v></c><c r="B4" s="2"><v>46335.604166666664</v></c><c r="C4" t="e"><v>#DIV/0!</v></c><c r="ZZ4"><v>9</v></c></row>
            </sheetData></worksheet>"#,
        );
        let t = read(&p).unwrap();
        assert_eq!(t.columns, ["Name", "Date", "", "Paid"]);
        assert_eq!(
            t.rows,
            [
                vec!["Rossi & co", "2026-11-09", "1234.5", "TRUE"],
                vec!["x", "2026-11-09 14:30", "#DIV/0!"],
            ]
        );
        assert_eq!(t.total, Some(2));
    }

    #[test]
    fn a_long_sheet_shows_its_start_and_counts_the_rest() {
        let mut sheet = String::from("<worksheet><sheetData>");
        for i in 0..1000 {
            sheet.push_str(&format!(
                "<row><c><v>{i}</v></c><c t=\"inlineStr\"><is><t>row {i}</t></is></c></row>"
            ));
        }
        sheet.push_str("</sheetData></worksheet>");
        let (_t, p) = workbook(&sheet);
        let t = read(&p).unwrap();
        assert_eq!(t.columns, ["0", "row 0"]);
        assert_eq!(t.rows.len(), ROWS);
        assert_eq!(t.rows[ROWS - 1], ["50", "row 50"]);
        assert_eq!(t.total, Some(999));
    }

    #[test]
    fn not_a_workbook() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("r.xlsx");
        std::fs::write(&p, "Name,Amount\n").unwrap();
        assert!(read(&p).is_err());
    }

    #[test]
    fn dates_and_formats() {
        assert_eq!(date_text(1.0, When::Date, false).unwrap(), "1900-01-01");
        assert_eq!(date_text(59.0, When::Date, false).unwrap(), "1900-02-28");
        assert_eq!(date_text(61.0, When::Date, false).unwrap(), "1900-03-01");
        assert_eq!(date_text(0.0, When::Date, true).unwrap(), "1904-01-01");
        assert_eq!(date_text(0.5, When::Time, false).unwrap(), "12:00");
        assert_eq!(
            date_text(45000.99999999, When::DateTime, false).unwrap(),
            "2023-03-16"
        );
        assert_eq!(format_kind("yyyy-mm-dd h:mm:ss"), Some(When::DateTime));
        assert_eq!(format_kind("[$-410]d mmmm yyyy"), Some(When::Date));
        assert_eq!(format_kind("[h]:mm"), None);
        assert_eq!(format_kind("#,##0.00 \"days\""), None);
        assert_eq!(format_kind("0.00_);[Red]\\(0.00\\)"), None);
        for (written, shown) in [
            ("99.90000000000001", "99.9"),
            ("7", "7"),
            ("-0.25", "-0.25"),
            ("1234567.891", "1234567.891"),
            ("0.30000000000000004", "0.3"),
            ("1E-3", "0.001"),
            ("1.5E+20", "1.5E+20"),
            ("x", "x"),
        ] {
            assert_eq!(number_text(written), shown, "{written}");
        }
        assert_eq!(column("A1"), Some(0));
        assert_eq!(column("AB7"), Some(27));
    }
}
