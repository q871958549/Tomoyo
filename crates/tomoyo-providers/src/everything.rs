use crate::everything_items::{build_entry_item, empty_result_item, hint_items, offline_item};
use crate::everything_rank::compute_ev_score;
#[cfg(not(target_os = "linux"))]
use crate::everything_rank::{build_prefix_query, merge_unique_entries};
use std::sync::{Arc, RwLock};
use tomoyo_core::{Lang, Provider, TomoyoConfig};
#[cfg(not(target_os = "linux"))]
use tomoyo_everything::{EverythingQueryOptions, SortOrder};
use tomoyo_protocol::{QueryContext, ResultItem, SearchFilter};

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

        if !dedicated && !filtered_file_mode && search_text.chars().count() < 2 {
            return Vec::new();
        }
        if search_text.starts_with("http") {
            return Vec::new();
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
                timeout_ms: if expanded { 800 } else { 450 },
                ..Default::default()
            };

            match tomoyo_everything::query(&effective_query, &opts) {
                Ok(mut resp) => {
                    let is_plain_token = !search_text.is_empty()
                        && !search_text.contains(char::is_whitespace)
                        && !search_text.contains(':');
                    if is_plain_token && resp.total_items > fetch_limit {
                        let prefix_q =
                            build_prefix_query(search_text, ctx.filter, &_extra_filter, expanded);
                        let prefix_opts = EverythingQueryOptions {
                            max_results: 32,
                            sort: SortOrder::NameAsc,
                            timeout_ms: 250,
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
                    tracing::warn!("Everything query `{effective_query}` failed: {e}");
                    Vec::new()
                }
            }
        }
    }
}
