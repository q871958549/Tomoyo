use crate::everything_items::{build_entry_item, empty_result_item, hint_items, offline_item};
use crate::everything_rank::compute_ev_score;
#[cfg(not(target_os = "linux"))]
use crate::everything_rank::{build_prefix_query, merge_unique_entries};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tomoyo_core::{Lang, Provider, TomoyoConfig};
#[cfg(not(target_os = "linux"))]
use tomoyo_everything::{EverythingQueryOptions, SortOrder};
use tomoyo_protocol::{QueryContext, ResultItem, SearchFilter};

#[cfg(not(target_os = "linux"))]
static EV_TIMEOUT_STREAK: AtomicU64 = AtomicU64::new(0);
#[cfg(not(target_os = "linux"))]
static EV_COOLDOWN_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(not(target_os = "linux"))]
static LAST_WARN_KEY: OnceLock<Mutex<String>> = OnceLock::new();

#[cfg(not(target_os = "linux"))]
fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Skips global unprefixed Everything IPC for non-file inputs (math like `5564*0.05` or 2-char ASCII).
pub(crate) fn should_skip_global_everything(search_text: &str) -> bool {
    let s = search_text.trim();
    if s.is_empty() || s.starts_with("http") || matches!(s.as_bytes().first(), Some(b'=' | b'?' | b'>' | b':')) {
        return true;
    }
    if looks_like_pure_math(s) {
        return true;
    }
    let has_path_or_ext = s.contains('.') || s.contains(':') || s.contains('\\') || s.contains('/');
    !has_path_or_ext && s.is_ascii() && s.len() < 3
}

fn looks_like_pure_math(s: &str) -> bool {
    let low = s.to_ascii_lowercase();
    if low.starts_with("0x") && low.len() > 2 && low[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    if low.starts_with("0b") && low.len() > 2 && low[2..].bytes().all(|b| b == b'0' || b == b'1') {
        return true;
    }
    let has_digit = s.bytes().any(|b| b.is_ascii_digit());
    let has_op = s.bytes().any(|b| matches!(b, b'+' | b'-' | b'*' | b'/' | b'%' | b'^'));
    if !has_digit || !has_op {
        return false;
    }
    s.bytes().all(|b| {
        b.is_ascii_digit() || matches!(b, b' ' | b'.' | b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'(' | b')')
    })
}

/// First-class file search provider: Everything 1.4/1.5a IPC on Windows, `fd`/`plocate` on Linux.
pub struct EverythingProvider {
    config: Arc<RwLock<TomoyoConfig>>,
}

impl EverythingProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        Self { config }
    }

    fn lang(&self) -> Lang {
        self.config.read().map(|c| c.lang()).unwrap_or(Lang::Zh)
    }
}

impl Provider for EverythingProvider {
    fn id(&self) -> &'static str {
        "builtin.everything"
    }

    fn name(&self) -> &'static str {
        "File Search (Everything / fd)"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["f", "ev"]
    }

    fn supports_global(&self) -> bool {
        true
    }

    fn is_slow_ipc(&self) -> bool {
        true
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let lang = self.lang();
        let dedicated = ctx.keyword.is_some();
        let filtered_file_mode = ctx.filter.is_file_only();
        let search_text = if dedicated {
            ctx.args.trim()
        } else {
            ctx.trimmed.as_str()
        };

        if search_text.is_empty() && !filtered_file_mode {
            return if dedicated {
                hint_items(lang, &self.config)
            } else {
                Vec::new()
            };
        }

        if !dedicated && !filtered_file_mode && should_skip_global_everything(search_text) {
            return Vec::new();
        }
        if dedicated && search_text.is_empty() && !filtered_file_mode {
            return hint_items(lang, &self.config);
        }

        let (cfg_global, cfg_ded, _extra_filter, _custom_expr, custom_tab) = self
            .config
            .read()
            .map(|c| {
                let ct = match ctx.filter {
                    SearchFilter::Custom(i) => c.custom_filters.get(i as usize).cloned(),
                    _ => None,
                };
                let ce = ct.as_ref().and_then(|t| t.effective_everything_expr());
                (
                    c.everything.max_global_results,
                    c.everything.max_dedicated_results,
                    c.everything.extra_filter.clone(),
                    ce,
                    ct,
                )
            })
            .unwrap_or((6, 30, String::new(), None, None));

        let expanded = dedicated || filtered_file_mode;
        if !expanded && cfg_global == 0 {
            return Vec::new();
        }
        let target_cap = if expanded {
            ctx.limit.min(cfg_ded).clamp(1, 50)
        } else {
            ctx.limit.min(cfg_global).clamp(1, 10)
        };

        #[cfg(target_os = "linux")]
        {
            let _ = _custom_expr;
            match crate::everything_linux::query_linux_files(
                search_text,
                ctx.filter,
                custom_tab.as_ref(),
                target_cap,
            ) {
                Ok(resp) => {
                    if resp.entries.is_empty() && expanded {
                        return vec![empty_result_item(self.id(), search_text, ctx.filter, lang)];
                    }
                    let mut items: Vec<ResultItem> = resp
                        .entries
                        .into_iter()
                        .enumerate()
                        .map(|(idx, entry)| {
                            let score = compute_ev_score(search_text, &entry, expanded, idx);
                            build_entry_item(self.id(), entry, score, lang)
                        })
                        .collect();
                    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
                    items.truncate(target_cap);
                    items
                }
                Err(_) => {
                    if expanded {
                        vec![offline_item(self.id())]
                    } else {
                        Vec::new()
                    }
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = custom_tab;
            if !expanded && now_epoch_ms() < EV_COOLDOWN_UNTIL_MS.load(Ordering::Relaxed) {
                tracing::debug!("Everything global query `{search_text}` skipped during IPC cooldown");
                return Vec::new();
            }
            let mut parts: Vec<String> = Vec::with_capacity(3);
            if !search_text.is_empty() {
                parts.push(search_text.to_string());
            }
            if let Some(expr) = ctx.filter.everything_expr() {
                parts.push(expr.to_string());
            } else if let Some(expr) = _custom_expr {
                parts.push(expr);
            }
            if expanded && !_extra_filter.trim().is_empty() {
                parts.push(_extra_filter.trim().to_string());
            }
            let effective_query = parts.join(" ");
            let fetch_limit = if search_text.is_empty() {
                target_cap as u32
            } else {
                (target_cap * 6).clamp(48, 80) as u32
            };
            let opts = EverythingQueryOptions {
                max_results: fetch_limit,
                sort: if search_text.is_empty() {
                    SortOrder::DateModifiedDesc
                } else {
                    SortOrder::NameAsc
                },
                timeout_ms: if expanded { 900 } else { 650 },
                ..Default::default()
            };

            match tomoyo_everything::query(&effective_query, &opts) {
                Ok(mut resp) => {
                    EV_TIMEOUT_STREAK.store(0, Ordering::Relaxed);
                    EV_COOLDOWN_UNTIL_MS.store(0, Ordering::Relaxed);
                    let is_plain_token = !search_text.is_empty()
                        && !search_text.contains(char::is_whitespace)
                        && !search_text.contains(':');
                    if is_plain_token && resp.total_items > fetch_limit {
                        let prefix_q =
                            build_prefix_query(search_text, ctx.filter, &_extra_filter, expanded);
                        let prefix_opts = EverythingQueryOptions {
                            max_results: 32,
                            sort: SortOrder::NameAsc,
                            timeout_ms: 280,
                            ..Default::default()
                        };
                        if let Ok(pref_resp) = tomoyo_everything::query(&prefix_q, &prefix_opts) {
                            merge_unique_entries(&mut resp.entries, pref_resp.entries);
                        }
                    }

                    if resp.entries.is_empty() && expanded {
                        return vec![empty_result_item(self.id(), search_text, ctx.filter, lang)];
                    }

                    let mut items: Vec<ResultItem> = resp
                        .entries
                        .into_iter()
                        .enumerate()
                        .map(|(idx, entry)| {
                            let score = compute_ev_score(search_text, &entry, expanded, idx);
                            build_entry_item(self.id(), entry, score, lang)
                        })
                        .collect();

                    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
                    items.truncate(target_cap);
                    items
                }
                Err(tomoyo_everything::EverythingError::NotRunning) => {
                    if expanded {
                        vec![offline_item(self.id())]
                    } else {
                        Vec::new()
                    }
                }
                Err(e) => {
                    let is_timeout = matches!(e, tomoyo_everything::EverythingError::Timeout(_));
                    if is_timeout {
                        let streak = EV_TIMEOUT_STREAK.fetch_add(1, Ordering::Relaxed) + 1;
                        if streak >= 2 {
                            EV_COOLDOWN_UNTIL_MS.store(now_epoch_ms().saturating_add(2_500), Ordering::Relaxed);
                        }
                        if expanded || streak == 1 {
                            tracing::warn!("Everything query `{effective_query}` failed: {e} (hint: Everything indexing/UI busy; coalescing bursts)");
                        } else {
                            tracing::debug!("Everything query `{effective_query}` timed out (streak={streak}): {e}");
                        }
                    } else {
                        let msg_key = e.to_string();
                        let lock = LAST_WARN_KEY.get_or_init(|| Mutex::new(String::new()));
                        if let Ok(mut prev) = lock.lock()
                            && *prev != msg_key
                        {
                            *prev = msg_key;
                            tracing::warn!("Everything query `{effective_query}` failed: {e}");
                        }
                    }
                    Vec::new()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::should_skip_global_everything;

    #[test]
    fn skips_math_and_short_ascii_in_global_everything() {
        assert!(should_skip_global_everything("5564*0.05"));
        assert!(should_skip_global_everything("5564*"));
        assert!(should_skip_global_everything("55"));
        assert!(should_skip_global_everything("ru"));
        assert!(should_skip_global_everything("oc"));
        assert!(!should_skip_global_everything("rust"));
        assert!(!should_skip_global_everything("a.rs"));
        assert!(!should_skip_global_everything("报告"));
        assert!(!should_skip_global_everything("v2-4"));
    }
}
