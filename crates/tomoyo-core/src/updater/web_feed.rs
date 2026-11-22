//! Zero-API-quota GitHub Releases resolver (`releases/latest`, `releases.atom`, `expanded_assets`).
//!
//! Ensures update checks keep working cleanly even when `api.github.com` returns HTTP 403
//! unauthenticated rate limits on shared IPs/NATs or proxies.

use super::installer::{pick_release_assets, CandidateAsset};
use super::{normalize_version, ReleaseInfo};
use crate::http::{http_get, http_get_detailed, http_head_redirect};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomReleaseEntry {
    pub tag: String,
    pub title: String,
    pub updated: String,
    pub html_url: String,
    pub notes: String,
}

pub fn extract_tag_from_release_url(url: &str) -> Option<String> {
    let idx = url.find("/releases/tag/")?;
    let rest = &url[idx + "/releases/tag/".len()..];
    let tag: String = rest
        .chars()
        .take_while(|c| !matches!(c, '"' | '\'' | '<' | '>' | '?' | '#' | '&' | ' ' | '/'))
        .collect();
    if tag.is_empty() { None } else { Some(tag) }
}

fn unescape_xml_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

fn strip_html_to_notes(raw_html: &str) -> String {
    let decoded = unescape_xml_entities(raw_html);
    let mut out = String::with_capacity(decoded.len().min(1600));
    let mut in_tag = false;
    let mut tag_buf = String::new();
    for ch in decoded.chars() {
        if ch == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let low = tag_buf.trim().to_ascii_lowercase();
                if (low.starts_with("br")
                    || low == "/p"
                    || low == "/li"
                    || low.starts_with("/h")
                    || low == "/tr")
                    && !out.ends_with('\n')
                {
                    out.push('\n');
                } else if low.starts_with("li") {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("• ");
                }
            } else {
                tag_buf.push(ch);
            }
            continue;
        }
        out.push(ch);
    }
    out.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(1500)
        .collect()
}

fn extract_xml_tag(block: &str, tag_name: &str) -> Option<String> {
    let open = format!("<{tag_name}");
    let start_idx = block.find(&open)?;
    let after_open = block[start_idx..].find('>')? + start_idx + 1;
    let close = format!("</{tag_name}>");
    let end_idx = block[after_open..].find(&close)? + after_open;
    Some(block[after_open..end_idx].trim().to_string())
}

pub fn parse_atom_entries(xml: &str) -> Vec<AtomReleaseEntry> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(s) = rest.find("<entry>") {
        let after = &rest[s + "<entry>".len()..];
        let Some(e) = after.find("</entry>") else {
            break;
        };
        let block = &after[..e];
        rest = &after[e + "</entry>".len()..];

        let mut tag_opt = None;
        let mut html_url = String::new();
        if let Some(href_pos) = block.find("href=\"") {
            let href_rest = &block[href_pos + 6..];
            if let Some(end_q) = href_rest.find('"') {
                let href = unescape_xml_entities(&href_rest[..end_q]);
                tag_opt = extract_tag_from_release_url(&href);
                html_url = href;
            }
        }
        if tag_opt.is_none()
            && let Some(id_val) = extract_xml_tag(block, "id")
                && let Some(last) = id_val.rsplit('/').next()
                    && !last.is_empty() {
                        tag_opt = Some(last.to_string());
                    }
        let Some(tag) = tag_opt else {
            continue;
        };
        let title = extract_xml_tag(block, "title")
            .map(|t| unescape_xml_entities(&t))
            .unwrap_or_else(|| tag.clone());
        let updated = extract_xml_tag(block, "updated").unwrap_or_default();
        let notes = extract_xml_tag(block, "content")
            .map(|c| strip_html_to_notes(&c))
            .unwrap_or_default();
        out.push(AtomReleaseEntry {
            tag,
            title,
            updated,
            html_url,
            notes,
        });
        if out.len() >= 10 {
            break;
        }
    }
    out
}

pub fn parse_expanded_assets(html: &str, clean_repo: &str, tag: &str) -> Vec<CandidateAsset> {
    let needle = format!("/{clean_repo}/releases/download/{tag}/");
    let mut assets = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find(&needle) {
        let after_path = &rest[pos + needle.len()..];
        let name: String = after_path
            .chars()
            .take_while(|c| !matches!(c, '"' | '\'' | '<' | '>' | '?' | '#' | ' '))
            .collect();
        rest = &rest[pos + needle.len()..];
        if name.is_empty() || assets.iter().any(|a: &CandidateAsset| a.name == name) {
            continue;
        }
        assets.push(CandidateAsset {
            url: format!("https://github.com/{clean_repo}/releases/download/{tag}/{name}"),
            name,
        });
    }
    assets
}

pub fn fetch_release_via_github_web(clean_repo: &str) -> Result<Option<ReleaseInfo>, String> {
    let latest_url = format!("https://github.com/{clean_repo}/releases/latest");
    let mut resolved_tag: Option<String> = None;
    let mut saw_404 = false;

    if let Ok(head) = http_head_redirect(&latest_url, 6000) {
        if head.status_code == 404 {
            saw_404 = true;
        } else if let Some(loc) = &head.location {
            resolved_tag = extract_tag_from_release_url(loc);
        } else if (200..300).contains(&head.status_code) {
            resolved_tag = extract_tag_from_release_url(&head.final_url);
        }
    }

    if resolved_tag.is_none() && !saw_404
        && let Ok(page) = http_get_detailed(&latest_url, &[], 7000, 256 * 1024) {
            if page.status_code == 404 {
                saw_404 = true;
            } else {
                resolved_tag = extract_tag_from_release_url(&page.final_url)
                    .or_else(|| extract_tag_from_release_url(&page.text_lossy()));
            }
        }

    let atom_url = format!("https://github.com/{clean_repo}/releases.atom");
    let atom_entries = http_get(&atom_url, 7000, 128 * 1024)
        .ok()
        .filter(|r| r.status_code == 200)
        .map(|r| parse_atom_entries(&r.text_lossy()))
        .unwrap_or_default();

    if resolved_tag.is_none() {
        resolved_tag = atom_entries.first().map(|e| e.tag.clone());
    }
    let Some(tag) = resolved_tag else {
        return if saw_404 || !atom_entries.is_empty() {
            Ok(None)
        } else {
            Err("无法连接 GitHub Web 发布页（请检查网络或代理设置）".into())
        };
    };

    let matched_atom = atom_entries
        .iter()
        .find(|e| e.tag.eq_ignore_ascii_case(&tag))
        .or_else(|| atom_entries.first());

    let assets_url = format!("https://github.com/{clean_repo}/releases/expanded_assets/{tag}");
    let mut assets = http_get(&assets_url, 7000, 256 * 1024)
        .ok()
        .filter(|r| r.status_code == 200)
        .map(|r| parse_expanded_assets(&r.text_lossy(), clean_repo, &tag))
        .unwrap_or_default();

    if assets.is_empty() && clean_repo.to_ascii_lowercase().ends_with("/tomoyo") {
        let vtag = if tag.starts_with(['v', 'V']) {
            tag.clone()
        } else {
            format!("v{tag}")
        };
        assets = vec![
            CandidateAsset {
                name: "SHA256SUMS.txt".into(),
                url: format!("https://github.com/{clean_repo}/releases/download/{tag}/SHA256SUMS.txt"),
            },
            CandidateAsset {
                name: format!("Tomoyo-{vtag}-windows-x64-setup.exe"),
                url: format!("https://github.com/{clean_repo}/releases/download/{tag}/Tomoyo-{vtag}-windows-x64-setup.exe"),
            },
            CandidateAsset {
                name: format!("Tomoyo-{vtag}-windows-x64-portable.zip"),
                url: format!("https://github.com/{clean_repo}/releases/download/{tag}/Tomoyo-{vtag}-windows-x64-portable.zip"),
            },
        ];
    }

    let picked = pick_release_assets(&assets);
    let version = normalize_version(&tag);
    Ok(Some(ReleaseInfo {
        tag: tag.clone(),
        version,
        name: matched_atom
            .map(|e| e.title.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Tomoyo {tag}")),
        notes: matched_atom.map(|e| e.notes.clone()).unwrap_or_default(),
        html_url: matched_atom
            .map(|e| e.html_url.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("https://github.com/{clean_repo}/releases/tag/{tag}")),
        published_at: matched_atom.map(|e| e.updated.clone()).unwrap_or_default(),
        download_url: picked.download_url,
        asset_name: picked.asset_name,
        sha256_url: picked.sha256_url,
        portable_url: picked.portable_url,
        portable_name: picked.portable_name,
        source: "github_web".into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_tag_from_redirect_and_html() {
        assert_eq!(
            extract_tag_from_release_url("https://github.com/q871958549/Tomoyo/releases/tag/v1.4.0")
                .as_deref(),
            Some("v1.4.0")
        );
        assert_eq!(
            extract_tag_from_release_url(
                r#"<link rel="canonical" href="https://github.com/q871958549/Tomoyo/releases/tag/v1.4.0">"#
            )
            .as_deref(),
            Some("v1.4.0")
        );
    }

    #[test]
    fn parses_releases_atom_feed_and_notes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <id>tag:github.com,2008:Repository/1/v1.4.0</id>
    <updated>2026-11-20T10:44:41Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/q871958549/Tomoyo/releases/tag/v1.4.0"/>
    <title>Tomoyo v1.4.0</title>
    <content type="html">&lt;p&gt;Features:&lt;/p&gt;&lt;ul&gt;&lt;li&gt;Translator&lt;/li&gt;&lt;/ul&gt;</content>
  </entry>
</feed>"#;
        let entries = parse_atom_entries(xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tag, "v1.4.0");
        assert_eq!(entries[0].title, "Tomoyo v1.4.0");
        assert!(entries[0].notes.contains("Features:"));
        assert!(entries[0].notes.contains("• Translator"));
    }

    #[test]
    fn parses_expanded_assets_html_links() {
        let html = r#"
          <a href="/q871958549/Tomoyo/releases/download/v1.4.0/SHA256SUMS.txt">SHA256SUMS.txt</a>
          <a href="/q871958549/Tomoyo/releases/download/v1.4.0/Tomoyo-v1.4.0-windows-x64-setup.exe">setup</a>
          <a href="/q871958549/Tomoyo/releases/download/v1.4.0/Tomoyo-v1.4.0-windows-x64-portable.zip">zip</a>
          <a href="/q871958549/Tomoyo/releases/download/v1.4.0/Tomoyo-v1.4.0-linux-x86_64.tar.gz">tgz</a>
        "#;
        let assets = parse_expanded_assets(html, "q871958549/Tomoyo", "v1.4.0");
        assert_eq!(assets.len(), 4);
        let picked = pick_release_assets(&assets);
        assert!(picked.sha256_url.as_deref().unwrap().ends_with("SHA256SUMS.txt"));
        assert!(picked.download_url.is_some());
        if cfg!(target_os = "linux") {
            assert!(picked.portable_url.as_deref().unwrap().ends_with("-linux-x86_64.tar.gz"));
        } else {
            assert!(picked.portable_url.as_deref().unwrap().ends_with("-portable.zip"));
        }
    }
}
