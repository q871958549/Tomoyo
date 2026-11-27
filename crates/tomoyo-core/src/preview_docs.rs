//! Strictly bounded, zero-dependency preview extractors for `.zip`, `.docx`, `.xlsx`, `.pptx` & `.pdf`.
//!
//! Uses tail-only PKZIP Central Directory parsing + `flate2` bounded deflate (<= 96 KB/entry)
//! so previewing large archives or Office documents never spikes RAM or triggers ZIP-bomb blowup.

use crate::preview::ResolvedPreview;
use flate2::read::DeflateDecoder;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MAX_OFFICE_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XML_ENTRY_BYTES: u64 = 96 * 1024;
const MAX_TAIL_READ: u64 = 65_558;

#[derive(Debug, Clone)]
pub(crate) struct ZipCdEntry {
    pub(crate) name: String,
    method: u16,
    comp_size: u32,
    uncomp_size: u32,
    local_offset: u32,
}

/// Attempts a rich bounded preview for `.zip`, `.docx`, `.xlsx`, `.pptx`, or `.pdf`.
pub(crate) fn try_preview_document(
    path: &Path,
    ext: &str,
    title: &str,
    size: u64,
    size_str: &str,
    modified: &str,
) -> Option<ResolvedPreview> {
    match ext {
        "zip" => preview_zip(path, title, size_str, modified),
        "docx" if size <= MAX_OFFICE_FILE_BYTES => preview_docx(path, title, size_str, modified),
        "xlsx" if size <= MAX_OFFICE_FILE_BYTES => {
            crate::preview_xlsx::preview_xlsx(path, title, size_str, modified)
        }
        "pptx" if size <= MAX_OFFICE_FILE_BYTES => preview_pptx(path, title, size_str, modified),
        "pdf" => preview_pdf(path, title, size_str, modified),
        _ => None,
    }
}

fn read_le16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}
fn read_le32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

pub(crate) fn read_zip_central_directory(f: &mut File) -> Option<Vec<ZipCdEntry>> {
    let len = f.metadata().ok()?.len();
    if len < 22 { return None; }
    let tail_len = len.min(MAX_TAIL_READ);
    f.seek(SeekFrom::Start(len - tail_len)).ok()?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail).ok()?;

    // Locate EOCD signature 0x06054b50 scanning backwards
    let eocd_pos = (0..=tail.len().saturating_sub(22))
        .rev()
        .find(|&i| &tail[i..i + 4] == b"PK\x05\x06")?;
    let total_entries = read_le16(&tail, eocd_pos + 10)? as usize;
    let cd_size = read_le32(&tail, eocd_pos + 12)? as u64;
    let cd_offset = read_le32(&tail, eocd_pos + 16)? as u64;
    if cd_offset >= len || cd_size > 2 * 1024 * 1024 { return None; }

    let read_cd = cd_size.min(len - cd_offset).min(512 * 1024) as usize;
    f.seek(SeekFrom::Start(cd_offset)).ok()?;
    let mut cd = vec![0u8; read_cd];
    f.read_exact(&mut cd).ok()?;

    let mut entries = Vec::with_capacity(total_entries.min(256));
    let mut pos = 0usize;
    while pos + 46 <= cd.len() && entries.len() < 400 {
        if &cd[pos..pos + 4] != b"PK\x01\x02" { break; }
        let method = read_le16(&cd, pos + 10)?;
        let comp_size = read_le32(&cd, pos + 20)?;
        let uncomp_size = read_le32(&cd, pos + 24)?;
        let name_len = read_le16(&cd, pos + 28)? as usize;
        let extra_len = read_le16(&cd, pos + 30)? as usize;
        let comment_len = read_le16(&cd, pos + 32)? as usize;
        let local_offset = read_le32(&cd, pos + 42)?;
        let name_end = pos + 46 + name_len;
        if name_end > cd.len() { break; }
        let name = String::from_utf8_lossy(&cd[pos + 46..name_end]).into_owned();
        entries.push(ZipCdEntry { name, method, comp_size, uncomp_size, local_offset });
        pos = name_end + extra_len + comment_len;
    }
    Some(entries)
}

pub(crate) fn extract_zip_entry_text(f: &mut File, entry: &ZipCdEntry) -> Option<String> {
    if entry.comp_size as u64 > 2 * 1024 * 1024 { return None; }
    f.seek(SeekFrom::Start(entry.local_offset as u64)).ok()?;
    let mut hdr = [0u8; 30];
    f.read_exact(&mut hdr).ok()?;
    if &hdr[0..4] != b"PK\x03\x04" { return None; }
    let nlen = read_le16(&hdr, 26)? as u64;
    let elen = read_le16(&hdr, 28)? as u64;
    f.seek(SeekFrom::Current((nlen + elen) as i64)).ok()?;

    let comp_len = (entry.comp_size as u64).min(2 * 1024 * 1024) as usize;
    let mut raw = vec![0u8; comp_len];
    f.read_exact(&mut raw).ok()?;
    let mut out = Vec::with_capacity(entry.uncomp_size.min(MAX_XML_ENTRY_BYTES as u32) as usize);
    match entry.method {
        0 => out.extend_from_slice(&raw[..raw.len().min(MAX_XML_ENTRY_BYTES as usize)]),
        8 => {
            let mut dec = DeflateDecoder::new(&raw[..]).take(MAX_XML_ENTRY_BYTES);
            let _ = dec.read_to_end(&mut out);
        }
        _ => return None,
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

fn preview_zip(path: &Path, title: &str, size_str: &str, modified: &str) -> Option<ResolvedPreview> {
    let mut f = File::open(path).ok()?;
    let entries = read_zip_central_directory(&mut f)?;
    let total = entries.len();
    let mut lines: Vec<String> = entries
        .iter()
        .take(18)
        .map(|e| {
            if e.name.ends_with('/') {
                format!("[DIR] {}", e.name)
            } else {
                format!("      {} ({})", e.name, compact_bytes(e.uncomp_size as u64))
            }
        })
        .collect();
    if total > 18 {
        lines.push(format!("... 共 {total} 项（已截断展示前 18 项）"));
    }
    Some(ResolvedPreview {
        title: title.to_string(),
        meta_line: format!("ZIP · {total} 项 · {size_str} · {modified}"),
        body: if lines.is_empty() { "（空压缩包）".into() } else { lines.join("\n") },
        is_monospace: true,
        image_rgba: None,
        badges: Vec::new(),
    })
}

fn preview_docx(path: &Path, title: &str, size_str: &str, modified: &str) -> Option<ResolvedPreview> {
    let mut f = File::open(path).ok()?;
    let entries = read_zip_central_directory(&mut f)?;
    let doc_ent = entries.iter().find(|e| e.name.eq_ignore_ascii_case("word/document.xml"))?;
    let xml = extract_zip_entry_text(&mut f, doc_ent)?;
    let body = extract_xml_paragraphs(&xml, "</w:p>", 24, 1500);
    Some(ResolvedPreview {
        title: title.to_string(),
        meta_line: format!("DOCX · {size_str} · {modified}"),
        body: if body.is_empty() { "（Word 文档无纯文本段落，按回车打开查看）".into() } else { body },
        is_monospace: false,
        image_rgba: None,
        badges: Vec::new(),
    })
}

fn preview_pptx(path: &Path, title: &str, size_str: &str, modified: &str) -> Option<ResolvedPreview> {
    let mut f = File::open(path).ok()?;
    let entries = read_zip_central_directory(&mut f)?;
    let mut slides: Vec<&ZipCdEntry> = entries
        .iter()
        .filter(|e| e.name.starts_with("ppt/slides/slide") && e.name.ends_with(".xml"))
        .collect();
    slides.sort_by_key(|e| {
        e.name.trim_start_matches("ppt/slides/slide").trim_end_matches(".xml").parse::<u32>().unwrap_or(9999)
    });
    let total = slides.len();
    let mut lines = Vec::new();
    for (idx, sl) in slides.into_iter().take(4).enumerate() {
        if let Some(xml) = extract_zip_entry_text(&mut f, sl) {
            let runs = extract_xml_tag_texts(&xml, "a:t", 8);
            if !runs.is_empty() {
                lines.push(format!("[第 {} 页] {}", idx + 1, runs.join(" · ")));
            }
        }
    }
    if total > 4 {
        lines.push(format!("... 共 {total} 页幻灯片"));
    }
    Some(ResolvedPreview {
        title: title.to_string(),
        meta_line: format!("PPTX · {total} 页 · {size_str} · {modified}"),
        body: if lines.is_empty() { format!("PowerPoint 演示文稿（共 {total} 页，按回车打开）") } else { lines.join("\n") },
        is_monospace: false,
        image_rgba: None,
        badges: Vec::new(),
    })
}

fn preview_pdf(path: &Path, title: &str, size_str: &str, modified: &str) -> Option<ResolvedPreview> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let head_len = len.min(16_384) as usize;
    let mut head = vec![0u8; head_len];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    if !head.starts_with(b"%PDF-") { return None; }
    let ver = head.get(5..8).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_else(|| "1.x".into());

    let mut combined = String::from_utf8_lossy(&head).into_owned();
    if len > 16_384 {
        let tail_len = (len - 16_384).min(16_384) as usize;
        if f.seek(SeekFrom::Start(len - tail_len as u64)).is_ok() {
            let mut tail = vec![0u8; tail_len];
            if f.read_exact(&mut tail).is_ok() {
                combined.push('\n');
                combined.push_str(&String::from_utf8_lossy(&tail));
            }
        }
    }

    let pages = extract_pdf_max_count(&combined);
    let mut lines = vec![format!("PDF 版本：PDF {ver}")];
    if let Some(p) = pages { lines.push(format!("估算页数：{p} 页")); }
    for (key, lbl) in [("/Title", "标题"), ("/Author", "作者"), ("/Creator", "生成工具")] {
        if let Some(v) = extract_pdf_literal(&combined, key) {
            lines.push(format!("{lbl}：{v}"));
        }
    }
    lines.push(String::new());
    lines.push(format!("路径：{}", path.display()));
    lines.push("提示：回车用默认阅读器打开 · Ctrl+Enter 定位目录".into());

    let page_badge = pages.map(|p| format!("{p} 页 · ")).unwrap_or_default();
    Some(ResolvedPreview {
        title: title.to_string(),
        meta_line: format!("PDF {ver} · {page_badge}{size_str} · {modified}"),
        body: lines.join("\n"),
        is_monospace: false,
        image_rgba: None,
        badges: Vec::new(),
    })
}

fn extract_pdf_max_count(s: &str) -> Option<u32> {
    let mut best = None;
    for part in s.split("/Count").skip(1) {
        let num_str: String = part.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(v) = num_str.parse::<u32>()
            && v > 0 && v < 100_000 { best = Some(best.map_or(v, |b: u32| b.max(v))); }
    }
    best
}

fn extract_pdf_literal(s: &str, key: &str) -> Option<String> {
    let idx = s.find(key)?;
    let rest = s[idx + key.len()..].trim_start();
    if let Some(inner) = rest.strip_prefix('(') {
        let mut out = String::new();
        let mut esc = false;
        for ch in inner.chars().take(120) {
            if esc { out.push(ch); esc = false; continue; }
            if ch == '\\' { esc = true; continue; }
            if ch == ')' { break; }
            if !ch.is_control() { out.push(ch); }
        }
        let t = out.trim();
        if !t.is_empty() && !t.starts_with('\u{feff}') { return Some(t.to_string()); }
    }
    None
}

fn extract_xml_paragraphs(xml: &str, para_close: &str, max_lines: usize, max_chars: usize) -> String {
    let mut out_lines = Vec::new();
    let mut chars = 0usize;
    for chunk in xml.split(para_close) {
        let mut plain = String::new();
        let mut in_tag = false;
        for ch in chunk.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => plain.push(ch),
                _ => {}
            }
        }
        let cleaned = decode_xml_entities(plain.trim());
        if cleaned.is_empty() { continue; }
        let clipped: String = cleaned.chars().take(max_chars.saturating_sub(chars)).collect();
        chars += clipped.chars().count();
        out_lines.push(clipped);
        if out_lines.len() >= max_lines || chars >= max_chars { break; }
    }
    out_lines.join("\n")
}

pub(crate) fn extract_xml_tag_texts(xml: &str, tag: &str, max_items: usize) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    for part in xml.split(&open).skip(1) {
        let Some(gt) = part.find('>') else { continue; };
        let Some(end) = part[gt + 1..].find(&close) else { continue; };
        let txt = decode_xml_entities(part[gt + 1..gt + 1 + end].trim());
        if !txt.is_empty() {
            out.push(txt.chars().take(90).collect());
            if out.len() >= max_items { break; }
        }
    }
    out
}

pub(crate) fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

fn compact_bytes(b: u64) -> String {
    if b < 1024 { format!("{b} B") }
    else if b < 1024 * 1024 { format!("{:.1} KB", b as f64 / 1024.0) }
    else { format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
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
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u32.to_le_bytes());
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
    fn previews_zip_docx_and_pdf_bounded() {
        let dir = std::env::temp_dir().join(format!("tomoyo_docprev_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let zip_p = dir.join("demo.zip");
        std::fs::write(&zip_p, make_stored_zip(&[("src/main.rs", b"fn main() {}")])).unwrap();
        let zp = try_preview_document(&zip_p, "zip", "demo.zip", 100, "100 B", "2026-11-21").unwrap();
        assert!(zp.body.contains("src/main.rs"));

        let docx_p = dir.join("spec.docx");
        let xml = b"<w:document><w:body><w:p><w:r><w:t>Tomoyo &amp; Rust</w:t></w:r></w:p></w:body></w:document>";
        std::fs::write(&docx_p, make_stored_zip(&[("word/document.xml", xml)])).unwrap();
        let dp = try_preview_document(&docx_p, "docx", "spec.docx", 200, "200 B", "2026-11-21").unwrap();
        assert!(dp.body.contains("Tomoyo & Rust"));

        let pdf_p = dir.join("guide.pdf");
        std::fs::write(&pdf_p, b"%PDF-1.7\n<< /Type /Pages /Count 12 >>\n<< /Title (Tomoyo Guide) /Author (Dev) >>").unwrap();
        let pp = try_preview_document(&pdf_p, "pdf", "guide.pdf", 90, "90 B", "2026-11-21").unwrap();
        assert!(pp.meta_line.contains("12 页"));
        assert!(pp.body.contains("Tomoyo Guide"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
