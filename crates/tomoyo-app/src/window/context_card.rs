//! Windows empty-launcher contextual clipboard card wrapper delegating to `crate::context_card`.

use super::AppShell;
use crate::context_card::{build_context_card, build_onboarding_card};
use tomoyo_protocol::SearchFilter;
use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
use windows::Win32::System::Ole::CF_DIB;

impl AppShell {
    /// Prepends at most one contextual clipboard suggestion card on empty `All` view.
    /// Skipped in headless `--self-test` (`self.tray.is_none()`) to keep benchmarks deterministic.
    pub(super) fn inject_context_clipboard_card(&mut self) {
        if self.tray.is_none()
            || !self.current_query.trim().is_empty()
            || self.active_filter != SearchFilter::All
            || self.attached.is_some()
        {
            return;
        }
        let Ok(cfg) = self.config.read().map(|c| c.clone()) else {
            return;
        };
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let has_dib = unsafe { IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok() };
        let mut idx = 0usize;
        if let Some(card) = build_context_card(
            self.last_clip_snapshot.as_ref(),
            has_dib,
            now_unix,
            &cfg,
            self.lang,
        ) && !self.results.iter().any(|r| r.id == card.id)
        {
            self.results.insert(0, card);
            idx = 1;
        }
        if let Some(guide) = build_onboarding_card(&cfg, self.router.is_first_run_frecency(), self.lang)
            && !self.results.iter().any(|r| r.id == guide.id)
        {
            self.results.insert(idx.min(self.results.len()), guide);
        }
        if self.results.len() > 16 {
            self.results.truncate(16);
        }
    }
}
