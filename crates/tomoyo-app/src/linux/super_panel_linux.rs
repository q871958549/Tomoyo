//! Lightweight native zero-WebView Linux Super Panel (`Alt+Q` / tray / IPC) reusing `PRIMARY` selection & shared classifier.

use crate::actions::{
    execute_action, read_clipboard_files, read_primary_selection_text, set_clipboard_text,
    shell_open,
};
use crate::context_card::{classify_text_kind, normalize_url, TextKind};
use crate::linux::ipc::schedule_on_ui;
use crate::linux::shell_state::{with_current_core, AppCore};
use crate::linux::tomoyo_linux_super_panel_show;
use crate::renderer::Rgb;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::Path;
use std::time::Duration;
use tomoyo_core::{
    format_css_font_stack, Lang, TomoyoConfig, PLUGIN_CAPTURE, PLUGIN_CLIPBOARD, PLUGIN_NOTES,
    PLUGIN_OCR, PLUGIN_TRANSLATE,
};
use tomoyo_plugin_host::PluginHost;
use tomoyo_protocol::{ActionKind, AttachedContext, OpenWebViewRequest};

#[derive(Debug, Clone)]
pub(super) enum LinuxSuperKind {
    OpenTranslator(String),
    OpenUrl(String),
    WebSearch(String),
    CopyText { text: String, toast: String },
    FillLauncher(String),
    AttachFiles(Vec<String>),
    CoreAction(ActionKind),
}

#[derive(Debug, Clone)]
pub(super) struct LinuxSuperAction {
    pub label: String,
    pub kind: LinuxSuperKind,
}

thread_local! {
    static PENDING_ACTIONS: RefCell<Vec<LinuxSuperAction>> = const { RefCell::new(Vec::new()) };
}

fn rgb_u32(c: Rgb) -> u32 {
    ((c.0 as u32) << 16) | ((c.1 as u32) << 8) | (c.2 as u32)
}

unsafe extern "C" fn on_super_panel_pick(_ctx: *mut c_void, idx: c_int) {
    let picked = PENDING_ACTIONS.with(|cell| {
        let mut list = cell.borrow_mut();
        let out = (idx >= 0).then(|| list.get(idx as usize).cloned()).flatten();
        list.clear();
        out
    });
    if let Some(act) = picked {
        schedule_on_ui(move || with_current_core(|st| st.exec_linux_super_action(act)));
    }
}

pub(super) fn build_linux_super_actions(
    sel_text: Option<&str>,
    clip_files: Option<&[String]>,
    cfg: &TomoyoConfig,
    host: &PluginHost,
    lang: Lang,
) -> (String, Vec<LinuxSuperAction>) {
    let (has_tr, has_notes, has_cap, has_ocr, has_clip) = (
        cfg.is_plugin_enabled(PLUGIN_TRANSLATE),
        cfg.is_plugin_enabled(PLUGIN_NOTES),
        cfg.is_plugin_enabled(PLUGIN_CAPTURE),
        cfg.is_plugin_enabled(PLUGIN_OCR),
        cfg.is_plugin_enabled(PLUGIN_CLIPBOARD),
    );
    let mut actions = Vec::new();
    let push = |out: &mut Vec<LinuxSuperAction>, lbl: &str, kind: LinuxSuperKind| {
        out.push(LinuxSuperAction { label: lbl.into(), kind });
    };

    if let Some(raw) = sel_text.map(str::trim).filter(|s| !s.is_empty()) {
        let kind = classify_text_kind(raw);
        let clip_preview: String = raw.replace(['\r', '\n', '\t'], " ").chars().take(26).collect();
        let hdr = format!("选中: {clip_preview}");
        if has_tr {
            push(&mut actions, lang.pick("多源对照翻译", "Translate"), LinuxSuperKind::OpenTranslator(raw.into()));
        }
        if kind == TextKind::Url && let Some(url) = normalize_url(raw) {
            push(&mut actions, lang.pick("打开链接", "Open URL"), LinuxSuperKind::OpenUrl(url));
        }
        if kind == TextKind::Path {
            let clean = raw.trim_matches('"').to_string();
            push(&mut actions, lang.pick("打开路径", "Open Path"), LinuxSuperKind::CoreAction(ActionKind::LaunchPath { path: clean, args: None, working_dir: None }));
        }
        if kind == TextKind::Color && has_cap {
            push(&mut actions, lang.pick("贴图色卡", "Pin Color"), LinuxSuperKind::CoreAction(ActionKind::PluginExecute {
                plugin_id: PLUGIN_CAPTURE.into(), action_id: "pin_text".into(), payload: serde_json::json!({ "text": raw }),
            }));
        }
        for spec in host.text_actions_for(raw, kind.as_str()).into_iter().take(2) {
            if !actions.iter().any(|a| a.label == spec.label) {
                push(&mut actions, &spec.label, LinuxSuperKind::CoreAction(spec.kind));
            }
        }
        push(&mut actions, lang.pick("网页搜索", "Web Search"), LinuxSuperKind::WebSearch(raw.into()));
        if has_notes && !actions.iter().any(|a| a.label.contains("备忘")) {
            let snip: String = raw.replace(['\r', '\n'], " ").chars().take(180).collect();
            push(&mut actions, lang.pick("存为速记", "Save Note"), LinuxSuperKind::FillLauncher(format!("note + {snip}")));
        }
        push(&mut actions, lang.pick("复制纯文本", "Copy Text"), LinuxSuperKind::CopyText {
            text: raw.into(), toast: lang.pick("✓ 已复制纯文本", "✓ Copied text").into(),
        });
        actions.truncate(6);
        return (hdr, actions);
    }

    if let Some(paths) = clip_files.filter(|p| !p.is_empty()) {
        let first = &paths[0];
        let name = Path::new(first).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| first.clone());
        let hdr = if paths.len() > 1 { format!("文件 ({}): {name}", paths.len()) } else { format!("文件: {name}") };
        let dir = Path::new(first).parent().filter(|_| !Path::new(first).is_dir()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| first.clone());
        let all_imgs = paths.iter().all(|p| matches!(Path::new(p).extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(), Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico")));
        push(&mut actions, lang.pick("Tomoyo 智能匹配", "Tomoyo Match"), LinuxSuperKind::AttachFiles(paths.to_vec()));
        push(&mut actions, lang.pick("复制文件路径", "Copy Paths"), LinuxSuperKind::CopyText {
            text: paths.join("\n"), toast: lang.pick("✓ 已复制文件路径", "✓ Copied paths").into(),
        });
        push(&mut actions, lang.pick("在此打开终端", "Open Terminal"), LinuxSuperKind::CoreAction(ActionKind::OpenTerminalAt { dir }));
        if all_imgs && has_ocr {
            push(&mut actions, lang.pick("离线识图 OCR", "Offline OCR"), LinuxSuperKind::CoreAction(ActionKind::PluginExecute {
                plugin_id: PLUGIN_OCR.into(), action_id: "ocr_file".into(), payload: serde_json::json!({ "path": first }),
            }));
        }
        if all_imgs && has_cap {
            push(&mut actions, lang.pick("桌面贴图", "Pin Image"), LinuxSuperKind::CoreAction(ActionKind::PluginExecute {
                plugin_id: PLUGIN_CAPTURE.into(), action_id: "pin_image".into(), payload: serde_json::json!({ "path": first }),
            }));
        }
        actions.truncate(6);
        return (hdr, actions);
    }

    if has_cap {
        push(&mut actions, lang.pick("区域截图", "Region Capture"), LinuxSuperKind::CoreAction(ActionKind::PluginExecute {
            plugin_id: PLUGIN_CAPTURE.into(), action_id: "run_region".into(), payload: serde_json::json!({ "mode": "region" }),
        }));
        push(&mut actions, lang.pick("桌面贴图", "Pin Clipboard"), LinuxSuperKind::CoreAction(ActionKind::PluginExecute {
            plugin_id: PLUGIN_CAPTURE.into(), action_id: "run_pin_clipboard".into(), payload: serde_json::json!({ "mode": "pin_clipboard" }),
        }));
    }
    if has_tr { push(&mut actions, lang.pick("多源翻译", "Translator"), LinuxSuperKind::OpenTranslator(String::new())); }
    if has_clip { push(&mut actions, lang.pick("剪贴板历史", "Clipboard"), LinuxSuperKind::FillLauncher("cb ".into())); }
    if has_notes { push(&mut actions, lang.pick("快速备忘录", "Quick Notes"), LinuxSuperKind::FillLauncher("note ".into())); }
    push(&mut actions, lang.pick("呼出 Tomoyo", "Open Tomoyo"), LinuxSuperKind::FillLauncher(String::new()));
    actions.truncate(6);
    (lang.pick("Tomoyo 超级面板", "Tomoyo Super Panel").into(), actions)
}

pub(super) fn pick_super_panel_source<'a>(
    sel: Option<&'a str>,
    files: Option<&'a [String]>,
    recent_rich_clip: bool,
) -> (Option<&'a str>, Option<&'a [String]>) {
    if recent_rich_clip && files.is_some_and(|f| !f.is_empty()) {
        return (None, files);
    }
    if sel.is_some_and(|s| !s.trim().is_empty()) {
        return (sel, None);
    }
    (None, files)
}

impl AppCore {
    pub(super) fn open_super_panel(&mut self) {
        let recent_rich_clip = self
            .last_rich_clip_at
            .is_some_and(|t| t.elapsed() <= Duration::from_secs(20));
        let sel = read_primary_selection_text();
        let files = if recent_rich_clip || sel.is_none() {
            read_clipboard_files()
        } else {
            None
        };
        let (eff_sel, eff_files) = pick_super_panel_source(sel.as_deref(), files.as_deref(), recent_rich_clip);
        let Ok(cfg) = self.config.read().map(|c| c.clone()) else { return; };
        let (header, actions) = build_linux_super_actions(eff_sel, eff_files, &cfg, &self.plugin_host, self.lang);
        if actions.is_empty() { return; }
        let c_hdr = CString::new(header).unwrap_or_default();
        let c_labels: Vec<CString> = actions.iter().map(|a| CString::new(a.label.as_str()).unwrap_or_default()).collect();
        let ptrs: Vec<*const c_char> = c_labels.iter().map(|c| c.as_ptr()).collect();
        let font_css = CString::new(format_css_font_stack(&self.typography.font_family, "sans-serif")).unwrap_or_default();
        PENDING_ACTIONS.with(|cell| *cell.borrow_mut() = actions);
        unsafe {
            tomoyo_linux_super_panel_show(
                c_hdr.as_ptr(), ptrs.as_ptr(), ptrs.len(),
                rgb_u32(self.theme.pop_bg), rgb_u32(self.theme.text_primary), rgb_u32(self.theme.text_muted),
                rgb_u32(self.theme.border), rgb_u32(self.theme.accent), font_css.as_ptr(),
                on_super_panel_pick, std::ptr::null_mut(),
            );
        }
    }

    fn exec_linux_super_action(&mut self, act: LinuxSuperAction) {
        match act.kind {
            LinuxSuperKind::OpenTranslator(text) => {
                let req = OpenWebViewRequest { initial_query: text, ..Default::default() };
                if let Some(wv) = self.plugin_host.resolve_webview_open(PLUGIN_TRANSLATE, &req) {
                    let out = execute_action(&wv, "sp:tr", &self.plugin_host, self.theme.is_dark);
                    if let Some(t) = out.toast { self.show_toast(&t); }
                }
            }
            LinuxSuperKind::OpenUrl(url) => { let _ = shell_open(&url, None, None); }
            LinuxSuperKind::WebSearch(q) => { self.show_launcher_with_query(Some(&format!("g {q}"))); }
            LinuxSuperKind::CopyText { text, toast } => { let _ = set_clipboard_text(&text); self.show_toast(&toast); }
            LinuxSuperKind::FillLauncher(q) => { self.show_launcher_with_query(Some(&q)); }
            LinuxSuperKind::AttachFiles(paths) => {
                self.attached = Some(AttachedContext::Files { paths });
                self.show_launcher_with_query(Some(""));
            }
            LinuxSuperKind::CoreAction(kind) => {
                let out = execute_action(&kind, "sp:act", &self.plugin_host, self.theme.is_dark);
                if let Some(q) = out.fill_query { self.show_launcher_with_query(Some(&q)); }
                if let Some(t) = out.toast { self.show_toast(&t); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_capped_contextual_actions_for_text_files_and_empty() {
        let mut cfg = TomoyoConfig::default();
        let host = PluginHost::new(Vec::new());
        let (_h, acts) = build_linux_super_actions(Some("https://github.com/q871958549/Tomoyo"), None, &cfg, &host, Lang::Zh);
        assert!(acts.len() <= 6 && acts.iter().any(|a| matches!(a.kind, LinuxSuperKind::OpenUrl(_))));
        let (_h2, facts) = build_linux_super_actions(None, Some(&["/tmp/a.png".into()]), &cfg, &host, Lang::Zh);
        assert!(facts.len() <= 6 && facts.iter().any(|a| matches!(a.kind, LinuxSuperKind::AttachFiles(_))));
        cfg.disabled_plugins.push(PLUGIN_TRANSLATE.into());
        let (_h3, eacts) = build_linux_super_actions(None, None, &cfg, &host, Lang::Zh);
        assert!(eacts.len() <= 6 && !eacts.iter().any(|a| matches!(a.kind, LinuxSuperKind::OpenTranslator(_))));
    }

    #[test]
    fn recent_rich_clipboard_beats_stale_primary_selection() {
        let files = vec!["/tmp/shot.png".to_string()];
        assert_eq!(pick_super_panel_source(Some("stale terminal text"), Some(&files), true), (None, Some(files.as_slice())));
        assert_eq!(pick_super_panel_source(Some("fresh selection"), Some(&files), false), (Some("fresh selection"), None));
        assert_eq!(pick_super_panel_source(Some("   "), Some(&files), false), (None, Some(files.as_slice())));
    }
}
