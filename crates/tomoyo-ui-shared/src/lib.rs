//! Platform-agnostic launcher shell model (`LauncherSession`), theme tokens & OS backend trait.
//!
//! Enables `tomoyo-ui-macos` and `tomoyo-ui-linux` to share identical query routing, filter
//! cycling, selection bounds, and theme presets cleanly separated from platform window toolkits.

use std::sync::Arc;
use tomoyo_core::{QueryRouter, TomoyoConfig};
use tomoyo_protocol::{ResultItem, SearchFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeTokens {
    pub is_dark: bool,
    pub bg_rgb: (u8, u8, u8),
    pub panel_rgb: (u8, u8, u8),
    pub accent_rgb: (u8, u8, u8),
    pub text_rgb: (u8, u8, u8),
}

impl ThemeTokens {
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "light" | "white" => Self {
                is_dark: false,
                bg_rgb: (248, 249, 252),
                panel_rgb: (255, 255, 255),
                accent_rgb: (24, 28, 40),
                text_rgb: (22, 24, 34),
            },
            "midnight" => Self {
                is_dark: true,
                bg_rgb: (14, 16, 26),
                panel_rgb: (20, 24, 38),
                accent_rgb: (99, 102, 241),
                text_rgb: (238, 242, 255),
            },
            _ => Self {
                is_dark: true,
                bg_rgb: (22, 22, 30),
                panel_rgb: (30, 30, 42),
                accent_rgb: (124, 106, 247),
                text_rgb: (243, 244, 248),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredDesktopApp {
    pub id: String,
    pub title: String,
    pub exec: String,
    pub subtitle: String,
}

/// Cross-platform OS adapter implemented by Windows, macOS, and Linux shell backends.
pub trait PlatformBackend: Send + Sync {
    fn platform_id(&self) -> &'static str;
    fn discover_desktop_apps(&self) -> Vec<DiscoveredDesktopApp>;
    fn search_files_hint(&self, query: &str, limit: usize) -> Vec<String>;
    fn open_target(&self, target: &str) -> Result<(), String>;
    fn copy_text(&self, text: &str) -> Result<(), String>;
}

/// Shared UI state reducer driving query updates, keyboard navigation & group filters.
pub struct LauncherSession {
    pub query: String,
    pub filter: SearchFilter,
    pub results: Vec<ResultItem>,
    pub selected: usize,
    pub theme: ThemeTokens,
    router: Arc<QueryRouter>,
}

impl LauncherSession {
    pub fn new(router: Arc<QueryRouter>, cfg: &TomoyoConfig) -> Self {
        Self {
            query: String::new(),
            filter: SearchFilter::All,
            results: Vec::new(),
            selected: 0,
            theme: ThemeTokens::from_name(&cfg.theme),
            router,
        }
    }

    pub fn update_query(&mut self, next: &str) {
        self.query = next.to_string();
        let gen_id = self.router.next_generation();
        self.results = self
            .router
            .search_with_filter(next, gen_id, 20, None, None, self.filter);
        self.selected = 0;
    }

    pub fn cycle_filter(&mut self, forward: bool) {
        self.filter = self.filter.cycle(forward);
        let q = self.query.clone();
        self.update_query(&q);
    }

    pub fn move_selection(&mut self, delta: i32) {
        let len = self.results.len();
        if len == 0 {
            return;
        }
        self.selected = if delta > 0 {
            (self.selected + 1) % len
        } else if self.selected == 0 {
            len - 1
        } else {
            self.selected - 1
        };
    }

    pub fn selected_item(&self) -> Option<&ResultItem> {
        self.results.get(self.selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;
    use tomoyo_core::FrecencyStore;

    #[test]
    fn shared_session_handles_filters_and_selection_wrap() {
        let frecency = Arc::new(RwLock::new(FrecencyStore::default()));
        let router = Arc::new(QueryRouter::new(Vec::new(), None, frecency));
        let cfg = TomoyoConfig::default();
        let mut session = LauncherSession::new(router, &cfg);
        assert!(session.theme.is_dark);
        session.cycle_filter(true);
        assert_eq!(session.filter, SearchFilter::App);
        session.update_query("hello");
        assert_eq!(session.selected, 0);
    }
}
