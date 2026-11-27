//! Pure-Rust bounded `.xlsx` first-sheet aligned table preview extractor.

use crate::preview::{format_aligned_table, ResolvedPreview};
use crate::preview_docs::{
    decode_xml_entities, extract_xml_tag_texts, extract_zip_entry_text,
    read_zip_central_directory, ZipCdEntry,
};
use std::fs::File;
use std::path::Path;

const MAX_SHARED_STRINGS: usize = 2048;
const MAX_PREVIEW_ROWS: usize = 20;
const MAX_PREVIEW_COLS: usize = 8;

pub(crate) fn preview_xlsx(
    path: &Path,
    title: &str,
    size_str: &str,
    modified: &str,
) -> Option<ResolvedPreview> {
    let mut f = File::open(path).ok()?;
    let entries = read_zip_central_directory(&mut f)?;
    let sheets = parse_workbook_sheets(&mut f, &entries);
    let shared = parse_shared_strings(&mut f, &entries);

    if let Some(sheet_ent) = find_first_worksheet(&entries)
        && let Some(sheet_xml) = extract_zip_entry_text(&mut f, sheet_ent)
        && let Some((cols, row_cnt, table)) = parse_sheet_table(&sheet_xml, &shared)
    {
        let mut body = String::new();
        if sheets.len() > 1 {
            body.push_str(&format!("工作表 ({})：{}\n\n", sheets.len(), sheets.join(" / ")));
        }
        body.push_str(&table);
        return Some(ResolvedPreview {
            title: title.to_string(),
            meta_line: format!(
                "XLSX · {} 表 · {} 列/{} 行 · {size_str} · {modified}",
                sheets.len().max(1),
                cols,
                row_cnt
            ),
            body,
            is_monospace: true,
            image_rgba: None,
            badges: Vec::new(),
        });
    }

    let mut lines = Vec::new();
    if !sheets.is_empty() {
        lines.push(format!("工作表 ({})：{}", sheets.len(), sheets.join(" / ")));
        lines.push(String::new());
    }
    if !shared.is_empty() {
        lines.push("文本摘要：".into());
        for s in shared.iter().take(20) {
            lines.push(format!("· {s}"));
        }
    }
    Some(ResolvedPreview {
        title: title.to_string(),
        meta_line: format!("XLSX · {} 表 · {size_str} · {modified}", sheets.len().max(1)),
        body: if lines.is_empty() { "（Excel 表格，按回车打开查看）".into() } else { lines.join("\n") },
        is_monospace: false,
        image_rgba: None,
        badges: Vec::new(),
    })
}

fn parse_workbook_sheets(f: &mut File, entries: &[ZipCdEntry]) -> Vec<String> {
    let mut sheets = Vec::new();
    if let Some(wb) = entries.iter().find(|e| e.name.eq_ignore_ascii_case("xl/workbook.xml"))
        && let Some(xml) = extract_zip_entry_text(f, wb)
    {
        for part in xml.split("<sheet ").skip(1).take(12) {
            if let Some(name) = extract_attr(part, "name") {
                sheets.push(name);
            }
        }
    }
    sheets
}

fn parse_shared_strings(f: &mut File, entries: &[ZipCdEntry]) -> Vec<String> {
    let Some(ss) = entries.iter().find(|e| e.name.eq_ignore_ascii_case("xl/sharedStrings.xml")) else {
        return Vec::new();
    };
    let Some(xml) = extract_zip_entry_text(f, ss) else { return Vec::new(); };
    let mut out = Vec::new();
    for part in xml.split("<si").skip(1).take(MAX_SHARED_STRINGS) {
        let Some(gt) = part.find('>') else { continue; };
        let Some(end) = part[gt + 1..].find("</si>") else { continue; };
        let joined = extract_t_runs(&part[gt + 1..gt + 1 + end]).join("");
        out.push(joined.replace(['\r', '\n', '\t'], " ").trim().to_string());
    }
    out
}

fn extract_t_runs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in xml.split("<t").skip(1) {
        let Some(after) = part.chars().next() else { continue; };
        if !matches!(after, '>' | ' ' | '\t' | '\r' | '\n') {
            continue;
        }
        let Some(gt) = part.find('>') else { continue; };
        let Some(end) = part[gt + 1..].find("</t>") else { continue; };
        out.push(decode_xml_entities(&part[gt + 1..gt + 1 + end]));
        if out.len() >= 16 {
            break;
        }
    }
    out
}

fn find_first_worksheet(entries: &[ZipCdEntry]) -> Option<&ZipCdEntry> {
    if let Some(s1) = entries.iter().find(|e| e.name.eq_ignore_ascii_case("xl/worksheets/sheet1.xml")) {
        return Some(s1);
    }
    entries
        .iter()
        .filter(|e| {
            let low = e.name.to_ascii_lowercase();
            low.starts_with("xl/worksheets/sheet") && low.ends_with(".xml")
        })
        .min_by_key(|e| {
            let low = e.name.to_ascii_lowercase();
            low.trim_start_matches("xl/worksheets/sheet")
                .trim_end_matches(".xml")
                .parse::<u32>()
                .unwrap_or(9999)
        })
}

fn parse_sheet_table(xml: &str, shared: &[String]) -> Option<(usize, usize, String)> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for row_chunk in xml.split("<row ").skip(1) {
        if rows.len() >= MAX_PREVIEW_ROWS {
            break;
        }
        let Some(r_gt) = row_chunk.find('>') else { continue; };
        let Some(r_end) = row_chunk[r_gt + 1..].find("</row>") else { continue; };
        let row_body = &row_chunk[r_gt + 1..r_gt + 1 + r_end];
        let mut cells: Vec<String> = Vec::new();
        for c_chunk in row_body.split("<c ").skip(1) {
            let Some((attrs, body)) = split_element_body(c_chunk, "</c>") else { continue; };
            let col_idx = extract_attr(attrs, "r")
                .and_then(|r| parse_col_index(&r))
                .unwrap_or(cells.len());
            if col_idx >= MAX_PREVIEW_COLS {
                continue;
            }
            let ctype = extract_attr(attrs, "t").unwrap_or_default();
            let val = match ctype.as_str() {
                "s" => first_tag_text(body, "v")
                    .and_then(|idx| idx.trim().parse::<usize>().ok())
                    .and_then(|i| shared.get(i).cloned())
                    .unwrap_or_default(),
                "inlineStr" => extract_t_runs(body).join(""),
                "b" => match first_tag_text(body, "v").as_deref().map(str::trim) {
                    Some("1") => "TRUE".into(),
                    Some("0") => "FALSE".into(),
                    Some(other) => other.into(),
                    None => String::new(),
                },
                _ => first_tag_text(body, "v").unwrap_or_default(),
            };
            let cleaned = val.replace(['\r', '\n', '\t'], " ").trim().to_string();
            if cleaned.is_empty() {
                continue;
            }
            if cells.len() <= col_idx {
                cells.resize(col_idx + 1, String::new());
            }
            cells[col_idx] = cleaned;
        }
        if cells.iter().any(|c| !c.is_empty()) {
            rows.push(cells);
        }
    }
    format_aligned_table(&rows, MAX_PREVIEW_COLS)
}

fn split_element_body<'a>(chunk: &'a str, close_tag: &str) -> Option<(&'a str, &'a str)> {
    let gt = chunk.find('>')?;
    let attrs = &chunk[..gt];
    if attrs.ends_with('/') {
        return None;
    }
    let end = chunk[gt + 1..].find(close_tag)?;
    Some((attrs, &chunk[gt + 1..gt + 1 + end]))
}

fn first_tag_text(xml: &str, tag: &str) -> Option<String> {
    extract_xml_tag_texts(xml, tag, 1).into_iter().next()
}

fn extract_attr(attrs: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let rest = if let Some(rest) = attrs.strip_prefix(&needle) {
        rest
    } else {
        let idx = attrs.find(&format!(" {needle}"))?;
        &attrs[idx + 1 + needle.len()..]
    };
    let end = rest.find('"')?;
    Some(decode_xml_entities(&rest[..end]))
}

pub(crate) fn parse_col_index(cell_ref: &str) -> Option<usize> {
    let mut col = 0usize;
    let mut any = false;
    for b in cell_ref.bytes() {
        if b.is_ascii_alphabetic() {
            any = true;
            col = col.checked_mul(26)?.checked_add((b.to_ascii_uppercase() - b'A') as usize + 1)?;
        } else {
            break;
        }
    }
    any.then(|| col - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview_docs::try_preview_document;

    fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut cd = Vec::new();
        for &(name, data) in entries {
            let off = buf.len() as u32;
            let nb = name.as_bytes();
            buf.extend_from_slice(b"PK\x03\x04\x14\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00");
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(nb);
            buf.extend_from_slice(data);

            cd.extend_from_slice(b"PK\x01\x02\x14\x00\x14\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00");
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0u8; 12]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(nb);
        }
        let cd_off = buf.len() as u32;
        buf.extend_from_slice(&cd);
        buf.extend_from_slice(b"PK\x05\x06\x00\x00\x00\x00");
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        buf.extend_from_slice(&cd_off.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf
    }

    #[test]
    fn parses_xlsx_column_letters_and_renders_aligned_first_sheet_table() {
        assert_eq!(parse_col_index("A1"), Some(0));
        assert_eq!(parse_col_index("Z9"), Some(25));
        assert_eq!(parse_col_index("AA12"), Some(26));
        assert_eq!(parse_col_index("42"), None);

        let wb = br#"<workbook><sheets><sheet name="Summary" sheetId="1"/><sheet name="Raw" sheetId="2"/></sheets></workbook>"#;
        let ss = br#"<sst><si><t>Project</t></si><si><r><t>Amount &amp; </t></r><r><t>Qty</t></r></si><si><t>Tomoyo</t></si></sst>"#;
        let s1 = br#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>1</v></c></row>
            <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>42</v></c><c r="C2" t="b"><v>1</v></c><c r="D2" t="inlineStr"><is><t>OK</t></is></c></row>
        </sheetData></worksheet>"#;

        let dir = std::env::temp_dir().join(format!("tomoyo_xlsxprev_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("report.xlsx");
        std::fs::write(&p, stored_zip(&[
            ("xl/workbook.xml", wb),
            ("xl/sharedStrings.xml", ss),
            ("xl/worksheets/sheet1.xml", s1),
        ])).unwrap();

        let prev = try_preview_document(&p, "xlsx", "report.xlsx", 512, "512 B", "2026-11-27").unwrap();
        assert!(prev.is_monospace);
        assert!(prev.meta_line.contains("XLSX · 2 表 · 4 列/2 行"));
        assert!(prev.body.contains("工作表 (2)：Summary / Raw"));
        assert!(prev.body.contains("Project"));
        assert!(prev.body.contains("Amount & Qty"));
        assert!(prev.body.contains("─┼─"));
        assert!(prev.body.contains("Tomoyo"));
        assert!(prev.body.contains("TRUE"));
        assert!(prev.body.contains("OK"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
