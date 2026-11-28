use super::*;

#[test]
fn parses_nl_size_date_path_and_quoted_terms_for_linux() {
    let h = parse_inline_hints("dm:today size:>10mb ext:pdf \"2026 报告\"");
    assert_eq!(h.changed_within, Some("1d"));
    assert!(!h.yesterday_only);
    assert_eq!(h.min_size_bytes, Some(10 * 1024 * 1024));
    assert_eq!(h.fd_size_args, vec!["+10m"]);
    assert_eq!(h.exts, vec!["pdf"]);
    assert_eq!(h.terms, "2026 报告");

    let dl = parse_inline_hints("path:Downloads size:<500kb ext:zip;tar dm:last7days");
    assert_eq!(dl.path_filters, vec!["Downloads"]);
    assert_eq!(dl.max_size_bytes, Some(500 * 1024));
    assert_eq!(dl.fd_size_args, vec!["-500k"]);
    assert_eq!(dl.exts, vec!["zip", "tar"]);
    assert_eq!(dl.changed_within, Some("7d"));

    let y = parse_inline_hints("dm:yesterday folder:");
    assert_eq!(y.changed_within, Some("2d"));
    assert!(y.yesterday_only);
    assert!(y.folders_only);
    assert_eq!(parse_inline_hints("dm:thismonth").changed_within, Some("30d"));
    assert_eq!(parse_inline_hints("dm:thisyear").changed_within, Some("365d"));
}

#[test]
fn prefers_plocate_for_plain_queries_and_fd_for_rich_filters() {
    let plain = CombinedQuery::from_inputs("Cargo.toml", SearchFilter::All, None);
    assert!(should_prefer_plocate(&plain, true, true));
    let dated = CombinedQuery::from_inputs("dm:today report", SearchFilter::Pdf, None);
    assert!(!should_prefer_plocate(&dated, true, true));
    let sized = CombinedQuery::from_inputs("size:>5mb video", SearchFilter::All, None);
    assert!(!should_prefer_plocate(&sized, true, true));
    let scoped = CombinedQuery::from_inputs("path:Downloads iso", SearchFilter::All, None);
    assert!(!should_prefer_plocate(&scoped, true, true));
    assert!(should_prefer_plocate(&scoped, true, false));
}
