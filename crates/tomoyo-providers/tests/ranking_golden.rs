//! Golden ranking & relevance regression benchmark for Tomoyo v1.1.0+.

use std::sync::{Arc, RwLock};
use tomoyo_core::{
    highlight_title_ranges, score_match, score_match_with_alias, FrecencyStore, IndexedText,
    Provider, QueryRouter, TomoyoConfig,
};
use tomoyo_protocol::{ActionKind, ActionSpec, IconSpec, QueryContext, ResultCategory, ResultItem};

struct MockCatalogProvider {
    id: &'static str,
    slow: bool,
    entries: Vec<(String, String, String, ResultCategory, i64)>,
}

impl Provider for MockCatalogProvider {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.id
    }
    fn is_slow_ipc(&self) -> bool {
        self.slow
    }
    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        self.entries
            .iter()
            .filter_map(|(id, title, path, cat, base)| {
                let idx = IndexedText::new(title);
                let matched = score_match(&ctx.trimmed, &idx)?;
                Some(ResultItem {
                    id: id.clone(),
                    provider_id: self.id.into(),
                    title: title.clone(),
                    subtitle: path.clone(),
                    category: *cat,
                    score: matched + base,
                    icon: IconSpec::ShellPath(path.clone()),
                    badge: None,
                    copy_text: Some(path.clone()),
                    actions: vec![ActionSpec {
                        id: "open".into(),
                        label: "Open".into(),
                        shortcut: Some("Enter".into()),
                        kind: ActionKind::LaunchPath {
                            path: path.clone(),
                            args: None,
                            working_dir: None,
                        },
                    }],
                    preview: None,
                })
            })
            .collect()
    }
}

#[test]
fn golden_query_aware_frecency_and_cross_source_dedup() {
    let frecency = Arc::new(RwLock::new(FrecencyStore::default()));
    {
        let mut store = frecency.write().unwrap();
        // Heavy global usage of WeChat under `wx`
        for _ in 0..8 {
            store.record_use_for_query("wx", "app:wechat");
        }
        // Single remembered selection of Android Studio under `as`
        store.record_use_for_query("as", "app:android_studio");
    }

    let apps = Arc::new(MockCatalogProvider {
        id: "builtin.apps",
        slow: false,
        entries: vec![
            (
                "app:wechat".into(),
                "微信 WeChat".into(),
                r"C:\Users\Administrator\DeskBox\应用\WeChat.lnk".into(),
                ResultCategory::App,
                0,
            ),
            (
                "app:android_studio".into(),
                "Android Studio".into(),
                r"C:\Users\Administrator\DeskBox\应用\Android Studio.lnk".into(),
                ResultCategory::App,
                0,
            ),
            (
                "app:analysis_services".into(),
                "Analysis Services".into(),
                r"C:\ProgramData\Start Menu\Analysis Services.lnk".into(),
                ResultCategory::App,
                0,
            ),
        ],
    });
    let ev = Arc::new(MockCatalogProvider {
        id: "builtin.everything",
        slow: true,
        entries: vec![
            // Duplicate of the exact same DeskBox shortcut from Everything
            (
                "ev:android_lnk".into(),
                "Android Studio.lnk".into(),
                r"c:\users\administrator\deskbox\应用\android studio.lnk".into(),
                ResultCategory::File,
                -200,
            ),
            (
                "ev:assets_dir".into(),
                "assets".into(),
                r"C:\Users\Administrator\DeskBox\项目\assets".into(),
                ResultCategory::Folder,
                -100,
            ),
        ],
    });

    let router = QueryRouter::new(vec![apps, ev], None, frecency)
        .with_config(Arc::new(RwLock::new(TomoyoConfig::default())));

    let g1 = router.next_generation();
    let hits_as = router.search("as", g1, 10, None, None);
    assert_eq!(hits_as[0].id, "app:android_studio");
    // Duplicate .lnk path from Everything must be deduplicated out
    assert!(
        hits_as.iter().all(|i| i.id != "ev:android_lnk"),
        "duplicate filesystem launch path should be removed"
    );

    let g2 = router.next_generation();
    let hits_wx = router.search("wx", g2, 10, None, None);
    assert_eq!(hits_wx[0].id, "app:wechat");
}

#[test]
fn golden_polyphone_multitoken_and_highlight_accuracy() {
    let cases = [
        ("重庆", "cq", "重庆"),
        ("重庆", "chongqing", "重庆"),
        ("招商银行", "zsyh", "招商银行"),
        ("网络调试助手", "wlts", "网络调试"),
        ("网易云音乐", "wyyyy", "网易云音乐"),
        ("Visual Studio Code", "visual code", "Visual"),
    ];
    for (title, query, expected_slice) in cases {
        let idx = IndexedText::new(title);
        assert!(
            score_match(query, &idx).is_some(),
            "query `{query}` should match `{title}`"
        );
        let spans = highlight_title_ranges(query, title);
        assert!(!spans.is_empty(), "query `{query}` should highlight `{title}`");
        let joined: String = spans.iter().map(|&(s, e)| &title[s..e]).collect();
        assert!(
            joined.contains(expected_slice),
            "expected highlight `{expected_slice}` in `{joined}` for query `{query}` on `{title}`"
        );
    }

    let primary = IndexedText::new("微信");
    let alias = IndexedText::new("WeChat");
    assert!(score_match_with_alias("wx wechat", &primary, Some(&alias)).is_some());
}

#[test]
fn golden_zero_result_fallback_never_dead_ends() {
    let frecency = Arc::new(RwLock::new(FrecencyStore::default()));
    let router = QueryRouter::new(vec![], None, frecency)
        .with_config(Arc::new(RwLock::new(TomoyoConfig::default())));
    let g = router.next_generation();
    let hits = router.search("nonexistent_xyz_9981", g, 10, None, None);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].category, ResultCategory::Web);
    assert!(matches!(hits[0].actions[0].kind, ActionKind::OpenUrl { .. }));
    assert!(matches!(hits[1].actions[0].kind, ActionKind::FillQuery { .. }));
}
