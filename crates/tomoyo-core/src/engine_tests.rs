//! Unit tests for `QueryRouter` routing, filters, fallbacks, and attached-image plugin dispatch.

use super::*;
use tomoyo_protocol::ResultCategory;

struct StubProvider {
    id: &'static str,
    slow: bool,
}

impl Provider for StubProvider {
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
        vec![ResultItem {
            id: format!("{}:{:?}:{}", self.id, ctx.filter, ctx.trimmed),
            provider_id: self.id.into(),
            title: format!("{}:{}", self.id, ctx.trimmed),
            subtitle: String::new(),
            category: if self.slow { ResultCategory::File } else { ResultCategory::App },
            score: 5000,
            icon: Default::default(),
            badge: None,
            copy_text: None,
            actions: vec![],
            preview: None,
        }]
    }
}

#[test]
fn routes_inline_tags_and_group_filters_correctly() {
    let dir = std::env::temp_dir().join("tomoyo_router_test_frecency.json");
    let frecency = Arc::new(RwLock::new(FrecencyStore::load_or_default(dir)));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider { id: "builtin.apps", slow: false }),
        Arc::new(StubProvider { id: "builtin.everything", slow: true }),
    ];
    let router = QueryRouter::new(providers, None, frecency);

    let g1 = router.next_generation();
    let pdf_hits = router.search("@pdf 报价", g1, 20, None, None);
    assert_eq!(pdf_hits.len(), 1);
    assert_eq!(pdf_hits[0].id, "builtin.everything:Pdf:报价");

    let g2 = router.next_generation();
    let app_hits = router.search_with_filter("oc", g2, 20, None, None, SearchFilter::App);
    assert_eq!(app_hits.len(), 1);
    assert_eq!(app_hits[0].id, "builtin.apps:App:oc");
}

#[test]
fn empty_global_search_falls_back_to_web_and_everything() {
    let frecency = Arc::new(RwLock::new(FrecencyStore::default()));
    let router = QueryRouter::new(vec![], None, frecency)
        .with_config(Arc::new(RwLock::new(TomoyoConfig::default())));
    let g = router.next_generation();
    let hits = router.search("zzzz_no_such_local_item", g, 20, None, None);
    assert_eq!(hits.len(), 2);
    assert!(hits[0].id.starts_with("fallback:web:"));
    assert!(hits[1].id.starts_with("fallback:ev:"));
}

struct MockOcrBridge;
impl ExternalPluginBridge for MockOcrBridge {
    fn matches_keyword(&self, kw: &str) -> bool {
        kw == "ocr"
    }
    fn query_keyword(&self, _kw: &str, ctx: &QueryContext) -> Vec<ResultItem> {
        assert!(ctx.attached.is_some(), "attached image must be forwarded to ocr plugin");
        vec![ResultItem {
            id: "ocr:result".into(),
            provider_id: "dev.tomoyo.ocr".into(),
            title: "recognized text".into(),
            subtitle: ctx.args.clone(),
            category: ResultCategory::Plugin,
            score: 10_600,
            icon: Default::default(),
            badge: None,
            copy_text: None,
            actions: vec![],
            preview: None,
        }]
    }
    fn query_global(&self, _ctx: &QueryContext) -> Vec<ResultItem> { Vec::new() }
    fn reload(&self) {}
    fn reap_idle(&self) {}
    fn summary_lines(&self) -> Vec<PluginSummary> { Vec::new() }
}

#[test]
fn attached_image_routes_explicit_plugin_keyword_like_ocr() {
    let frecency = Arc::new(RwLock::new(FrecencyStore::default()));
    let router = QueryRouter::new(vec![], Some(Arc::new(MockOcrBridge)), frecency);
    let g = router.next_generation();
    let attached = Some(AttachedContext::Image {
        path: "demo.png".into(),
        width: 800,
        height: 600,
    });
    let hits = router.search("ocr en", g, 10, None, attached);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "ocr:result");
    assert_eq!(hits[0].subtitle, "en");
}
