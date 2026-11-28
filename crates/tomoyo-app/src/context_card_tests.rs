use super::*;

#[test]
fn builds_context_cards_for_fresh_url_json_color_and_image_only() {
    let cfg = TomoyoConfig::default();
    let now = 1_700_000_000;

    let url_snap = ClipboardSnapshot { text: "https://github.com/q871958549/Tomoyo".into(), captured_at_unix: now - 10 };
    let url_card = build_context_card(Some(&url_snap), false, now, &cfg, Lang::Zh).unwrap();
    assert_eq!(url_card.id, "ctx:clip:url");
    assert!(matches!(url_card.actions[0].kind, ActionKind::OpenUrl { .. }));

    let json_snap = ClipboardSnapshot { text: r#"{"ok":true,"v":3}"#.into(), captured_at_unix: now - 5 };
    let json_card = build_context_card(Some(&json_snap), false, now, &cfg, Lang::Zh).unwrap();
    assert_eq!(json_card.id, "ctx:clip:json");
    assert!(json_card.copy_text.as_deref().unwrap().contains('\n'));

    let stale_snap = ClipboardSnapshot { text: "https://example.com".into(), captured_at_unix: now - 300 };
    assert!(build_context_card(Some(&stale_snap), false, now, &cfg, Lang::Zh).is_none());

    let img_card = build_context_card(None, true, now, &cfg, Lang::Zh).unwrap();
    assert_eq!(img_card.id, "ctx:clip:image");
    assert_eq!(img_card.actions.len(), 2);

    let short_txt = ClipboardSnapshot { text: "TranslateMessage(&msg)".into(), captured_at_unix: now - 5 };
    let short_card = build_context_card(Some(&short_txt), false, now, &cfg, Lang::Zh).unwrap();
    assert!(short_card.preview.is_none());

    let long_txt = ClipboardSnapshot { text: "Line 1\nLine 2 longer multi-line snippet".into(), captured_at_unix: now - 5 };
    let long_card = build_context_card(Some(&long_txt), false, now, &cfg, Lang::Zh).unwrap();
    assert!(long_card.preview.is_some());
}

#[test]
fn classifies_selected_text_kinds_accurately() {
    assert_eq!(classify_text_kind("https://github.com/q871958549/Tomoyo"), TextKind::Url);
    assert_eq!(classify_text_kind("www.rust-lang.org"), TextKind::Url);
    assert_eq!(classify_text_kind("#7C6AF7"), TextKind::Color);
    assert_eq!(classify_text_kind("rgb(16, 185, 129)"), TextKind::Color);
    assert_eq!(classify_text_kind(r#"{"ok":true,"n":42}"#), TextKind::Json);
    assert_eq!(classify_text_kind("serendipity"), TextKind::Plain);
    assert_eq!(classify_text_kind("选中文本快捷翻译"), TextKind::Plain);
    assert_eq!(
        classify_text_kind("fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}"),
        TextKind::Code
    );
    assert_eq!(normalize_url("www.example.com/a").as_deref(), Some("https://www.example.com/a"));
    assert!(normalize_url("javascript:alert(1)").is_none());
}

#[test]
fn onboarding_card_only_shows_on_first_run_before_dismiss() {
    let mut cfg = TomoyoConfig::default();
    assert!(build_onboarding_card(&cfg, true, Lang::Zh).is_some());
    assert!(build_onboarding_card(&cfg, false, Lang::Zh).is_none());
    cfg.onboarding_dismissed = true;
    assert!(build_onboarding_card(&cfg, true, Lang::Zh).is_none());
}
