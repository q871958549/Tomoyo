//! Pure-Rust SDF + ClearType GDI painter and hit-tester for the Quick Add/Edit Filter modal.

use crate::renderer::{
    dip_rect, draw_text_gdi, resolve_face_and_weight, Canvas, DibSurface, FontKey, FontPack, Rgb,
    ThemePalette,
};
use tomoyo_core::{Lang, TypographyConfig};
use windows::Win32::Graphics::Gdi::{
    BitBlt, GdiFlush, HDC, SRCCOPY, DT_CENTER, DT_LEFT, DT_RIGHT,
};

pub(super) const DLG_W_DIP: f32 = 432.0;
pub(super) const DLG_H_DIP: f32 = 424.0;
pub(super) const HEADER_H_DIP: f32 = 56.0;
pub(super) const FOOTER_Y_DIP: f32 = 370.0;

pub(super) const FIELD_RECTS: [(f32, f32, f32, f32); 3] = [
    (22.0, 90.0, 410.0, 124.0),
    (22.0, 152.0, 410.0, 186.0),
    (22.0, 246.0, 410.0, 280.0),
];
const PREVIEW_BOX: (f32, f32, f32, f32) = (22.0, 324.0, 410.0, 356.0);
const CLOSE_BTN: (f32, f32, f32, f32) = (384.0, 15.0, 410.0, 41.0);
const CANCEL_BTN: (f32, f32, f32, f32) = (228.0, 382.0, 300.0, 412.0);
const SAVE_BTN: (f32, f32, f32, f32) = (308.0, 382.0, 410.0, 412.0);

pub(super) const PRESETS: [(&str, &str, &str); 4] = [
    ("代码", "Code", "rs, ts, tsx, js, py, go, c, cpp, h, toml, json"),
    ("视频", "Video", "mp4, mkv, mov, avi, webm, flv"),
    ("音频", "Audio", "mp3, flac, wav, m4a, ogg, aac"),
    ("设计", "Design", "psd, fig, ai, sketch, xd, svg"),
];

pub(super) const COND_CHIPS: [(&str, &str, &str); 5] = [
    ("今日修改", "Today", "dm:today"),
    ("近7天", "7 Days", "dm:last7days"),
    ("大文件", ">50MB", "size:>50mb"),
    ("仅文件夹", "Folders", "folder:"),
    ("下载目录", "Downloads", "path:Downloads"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DlgHit {
    Close,
    Preset(usize),
    CondChip(usize),
    Cancel,
    Save,
}

fn preset_rect(idx: usize) -> (f32, f32, f32, f32) {
    let x0 = 80.0 + (idx as f32) * 56.0;
    (x0, 194.0, x0 + 50.0, 216.0)
}

fn cond_chip_rect(idx: usize) -> (f32, f32, f32, f32) {
    const WIDTHS: [f32; 5] = [72.0, 62.0, 68.0, 74.0, 76.0];
    let mut x0 = 22.0;
    for w in WIDTHS.iter().take(idx) {
        x0 += w + 9.0;
    }
    (x0, 288.0, x0 + WIDTHS[idx], 311.0)
}

fn contains(r: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    (r.0..=r.2).contains(&x) && (r.1..=r.3).contains(&y)
}

pub(super) fn hit_test_dlg(x_dip: f32, y_dip: f32) -> Option<DlgHit> {
    if contains(CLOSE_BTN, x_dip, y_dip) {
        return Some(DlgHit::Close);
    }
    if contains(CANCEL_BTN, x_dip, y_dip) {
        return Some(DlgHit::Cancel);
    }
    if contains(SAVE_BTN, x_dip, y_dip) {
        return Some(DlgHit::Save);
    }
    for i in 0..PRESETS.len() {
        if contains(preset_rect(i), x_dip, y_dip) {
            return Some(DlgHit::Preset(i));
        }
    }
    for i in 0..COND_CHIPS.len() {
        if contains(cond_chip_rect(i), x_dip, y_dip) {
            return Some(DlgHit::CondChip(i));
        }
    }
    None
}

pub(super) fn expr_has_token(expr: &str, token: &str) -> bool {
    expr.split_whitespace().any(|t| t.eq_ignore_ascii_case(token))
}

pub(super) fn toggle_expr_token(expr: &str, token: &str) -> String {
    let mut kept: Vec<&str> = expr.split_whitespace().collect();
    if let Some(pos) = kept.iter().position(|t| t.eq_ignore_ascii_case(token)) {
        kept.remove(pos);
    } else {
        kept.push(token);
    }
    kept.join(" ")
}

fn describe_expr_token(tok: &str, zh: bool) -> String {
    let low = tok.to_ascii_lowercase();
    match low.as_str() {
        "dm:today" => return if zh { "今日修改" } else { "Modified today" }.into(),
        "dm:yesterday" => return if zh { "昨日修改" } else { "Modified yesterday" }.into(),
        "dm:last7days" => return if zh { "近 7 天修改" } else { "Last 7 days" }.into(),
        "dm:thisweek" => return if zh { "本周修改" } else { "This week" }.into(),
        "dm:thismonth" => return if zh { "本月修改" } else { "This month" }.into(),
        "folder:" => return if zh { "仅文件夹" } else { "Folders only" }.into(),
        "path:downloads" => return if zh { "限定下载目录" } else { "In Downloads" }.into(),
        "path:desktop" => return if zh { "限定桌面" } else { "In Desktop" }.into(),
        "path:documents" => return if zh { "限定文档目录" } else { "In Documents" }.into(),
        _ => {}
    }
    if let Some(sz) = low.strip_prefix("size:") {
        return if zh { format!("体积 {sz}") } else { format!("Size {sz}") };
    }
    if let Some(ext) = low.strip_prefix("ext:") {
        return if zh { format!("扩展名 {ext}") } else { format!("Ext {ext}") };
    }
    tok.to_string()
}

pub(super) fn format_rule_preview(exts: &[String], expr: &str, lang: Lang) -> String {
    let zh = lang == Lang::Zh;
    let mut parts: Vec<String> = Vec::new();
    if !exts.is_empty() {
        let shown: Vec<&str> = exts.iter().take(4).map(|s| s.as_str()).collect();
        let mut s = shown.join(" / ");
        if exts.len() > 4 {
            s.push_str(if zh { " 等" } else { "…" });
        }
        parts.push(if zh { format!("扩展名 {s}") } else { format!("Ext {s}") });
    }
    for tok in expr.split_whitespace() {
        parts.push(describe_expr_token(tok, zh));
    }
    if parts.is_empty() {
        return if zh {
            "生效规则：填写扩展名或点选上方条件即可即时预览"
        } else {
            "Active rule: enter extensions or click chips above"
        }
        .into();
    }
    if zh {
        format!("生效规则：{}", parts.join(" · "))
    } else {
        format!("Active rule: {}", parts.join(" · "))
    }
}

pub(super) struct DlgRenderParams<'a> {
    pub pal: ThemePalette,
    pub lang: Lang,
    pub typo: &'a TypographyConfig,
    pub font_regular: &'static str,
    pub font_medium: &'static str,
    pub is_edit: bool,
    pub scale: f32,
    pub focused: Option<u8>,
    pub hovered: Option<DlgHit>,
    pub has_rule: bool,
    pub live_expr: &'a str,
    pub preview: &'a str,
    pub error: Option<&'a str>,
}

pub(super) struct DlgGfx {
    surface: Option<DibSurface>,
    fonts: Option<FontPack>,
}

impl DlgGfx {
    pub fn new() -> Self {
        Self { surface: None, fonts: None }
    }

    pub fn edit_font_face<'a>(
        &self,
        typo: &'a TypographyConfig,
        font_regular: &'a str,
        font_medium: &'a str,
    ) -> &'a str {
        resolve_face_and_weight(&typo.font_family, 400, font_regular, font_medium).0
    }

    pub fn paint(&mut self, screen_dc: HDC, w: i32, h: i32, p: &DlgRenderParams<'_>) {
        if w <= 0 || h <= 0 {
            return;
        }
        let scale = p.scale.max(1.0);
        let want_key = FontKey::from_config(scale, p.typo);
        if self.fonts.as_ref().map(|f| &f.key) != Some(&want_key) {
            self.fonts = Some(FontPack::create(scale, p.font_regular, p.font_medium, p.typo));
        }
        if self.surface.as_ref().map(|s| (s.width, s.height)) != Some((w, h)) {
            self.surface = DibSurface::create(screen_dc, w, h);
        }
        let (Some(surf), Some(fonts)) = (self.surface.as_mut(), self.fonts.as_ref()) else {
            return;
        };

        let pal = p.pal;
        let zh = p.lang == Lang::Zh;
        let err_col = if pal.is_dark { Rgb(0xF8, 0x71, 0x71) } else { Rgb(0xDC, 0x26, 0x26) };
        let (w_dip, h_dip) = (w as f32 / scale, h as f32 / scale);
        {
            let mut canvas = Canvas { buf: surf.pixels_mut(), w, h, scale };
            canvas.clear(pal.pop_bg);

            canvas.fill_rect_dip(0.0, HEADER_H_DIP, w_dip, HEADER_H_DIP + 1.0, pal.divider);
            if p.hovered == Some(DlgHit::Close) {
                canvas.fill_rounded_rect(
                    CLOSE_BTN.0, CLOSE_BTN.1, CLOSE_BTN.2, CLOSE_BTN.3, 6.0, pal.card_hover,
                );
            }

            for (i, &r) in FIELD_RECTS.iter().enumerate() {
                let is_foc = p.focused == Some(i as u8);
                canvas.fill_rounded_rect(r.0, r.1, r.2, r.3, 8.0, pal.bg);
                canvas.stroke_rounded_rect(
                    r.0, r.1, r.2, r.3, 8.0,
                    if is_foc { pal.accent } else { pal.border },
                );
                if is_foc {
                    canvas.stroke_rounded_rect(
                        r.0 + 0.5, r.1 + 0.5, r.2 - 0.5, r.3 - 0.5, 7.5, pal.accent,
                    );
                }
            }

            for i in 0..PRESETS.len() {
                let r = preset_rect(i);
                let hov = p.hovered == Some(DlgHit::Preset(i));
                let (bg, bd) = if hov {
                    (pal.chip_hover_bg, pal.chip_hover_border)
                } else {
                    (pal.chip_bg, pal.chip_border)
                };
                canvas.fill_rounded_rect(r.0, r.1, r.2, r.3, 6.0, bg);
                canvas.stroke_rounded_rect(r.0, r.1, r.2, r.3, 6.0, bd);
            }

            for (i, (_, _, tok)) in COND_CHIPS.iter().enumerate() {
                let r = cond_chip_rect(i);
                let active = expr_has_token(p.live_expr, tok);
                let hov = p.hovered == Some(DlgHit::CondChip(i));
                let (bg, bd) = if active {
                    (pal.chip_active_bg, pal.chip_active_border)
                } else if hov {
                    (pal.chip_hover_bg, pal.chip_hover_border)
                } else {
                    (pal.chip_bg, pal.chip_border)
                };
                canvas.fill_rounded_rect(r.0, r.1, r.2, r.3, 6.0, bg);
                canvas.stroke_rounded_rect(r.0, r.1, r.2, r.3, 6.0, bd);
            }

            canvas.fill_rounded_rect(
                PREVIEW_BOX.0, PREVIEW_BOX.1, PREVIEW_BOX.2, PREVIEW_BOX.3, 7.0, pal.footer_bg,
            );
            canvas.stroke_rounded_rect(
                PREVIEW_BOX.0, PREVIEW_BOX.1, PREVIEW_BOX.2, PREVIEW_BOX.3, 7.0, pal.divider,
            );
            let dot_col = if p.error.is_some() {
                err_col
            } else if p.has_rule {
                pal.accent
            } else {
                pal.text_muted
            };
            canvas.fill_rounded_rect(32.0, 337.0, 38.0, 343.0, 3.0, dot_col);

            canvas.fill_rect_dip(0.0, FOOTER_Y_DIP, w_dip, h_dip, pal.footer_bg);
            canvas.fill_rect_dip(0.0, FOOTER_Y_DIP, w_dip, FOOTER_Y_DIP + 1.0, pal.divider);

            let (cbg, cbd) = if p.hovered == Some(DlgHit::Cancel) {
                (pal.card_hover, pal.text_muted)
            } else {
                (pal.pill_bg, pal.border)
            };
            canvas.fill_rounded_rect(
                CANCEL_BTN.0, CANCEL_BTN.1, CANCEL_BTN.2, CANCEL_BTN.3, 7.0, cbg,
            );
            canvas.stroke_rounded_rect(
                CANCEL_BTN.0, CANCEL_BTN.1, CANCEL_BTN.2, CANCEL_BTN.3, 7.0, cbd,
            );

            let save_col = if p.hovered == Some(DlgHit::Save) {
                if pal.is_dark { Rgb(0x8D, 0x94, 0xFF) } else { Rgb(0x2F, 0x4E, 0xCC) }
            } else {
                pal.accent
            };
            canvas.fill_rounded_rect(SAVE_BTN.0, SAVE_BTN.1, SAVE_BTN.2, SAVE_BTN.3, 7.0, save_col);

            canvas.fill_rect_dip(0.0, 0.0, w_dip, 1.0, pal.border);
            canvas.fill_rect_dip(0.0, h_dip - 1.0, w_dip, h_dip, pal.border);
            canvas.fill_rect_dip(0.0, 0.0, 1.0, h_dip, pal.border);
            canvas.fill_rect_dip(w_dip - 1.0, 0.0, w_dip, h_dip, pal.border);
        }

        let dc = surf.mem_dc;
        unsafe {
            let _ = GdiFlush();
            let title = match (p.is_edit, zh) {
                (true, true) => "编辑自定义标签",
                (true, false) => "Edit Custom Tag",
                (false, true) => "快速添加搜索标签",
                (false, false) => "Quick Add Search Tag",
            };
            let sub = if zh {
                "支持侧栏点击与 @标签 直达过滤"
            } else {
                "Instant sidebar filter & @tag syntax"
            };
            draw_text_gdi(dc, fonts.title, pal.text_primary, title, dip_rect(22.0, 10.0, 374.0, 31.0, scale), DT_LEFT);
            draw_text_gdi(dc, fonts.footer, pal.text_muted, sub, dip_rect(22.0, 31.0, 374.0, 49.0, scale), DT_LEFT);
            let close_col = if p.hovered == Some(DlgHit::Close) { pal.text_primary } else { pal.text_muted };
            draw_text_gdi(dc, fonts.chip, close_col, "×", dip_rect(CLOSE_BTN.0, CLOSE_BTN.1, CLOSE_BTN.2, CLOSE_BTN.3, scale), DT_CENTER);

            let labels = if zh {
                ["标签名称", "文件扩展名", "高级筛选条件"]
            } else {
                ["Tag Name", "File Extensions", "Advanced Filter"]
            };
            for (i, lbl) in labels.iter().enumerate() {
                let y0 = FIELD_RECTS[i].1 - 20.0;
                draw_text_gdi(dc, fonts.chip_active, pal.text_secondary, lbl, dip_rect(22.0, y0, 260.0, FIELD_RECTS[i].1 - 3.0, scale), DT_LEFT);
            }
            let f2_hint = if zh { "可选 · 点下方一键添加" } else { "Optional · Click chips" };
            draw_text_gdi(dc, fonts.footer, pal.text_muted, f2_hint, dip_rect(240.0, FIELD_RECTS[2].1 - 20.0, 408.0, FIELD_RECTS[2].1 - 3.0, scale), DT_RIGHT);

            let tpl_lbl = if zh { "快捷预设" } else { "Presets" };
            draw_text_gdi(dc, fonts.footer, pal.text_muted, tpl_lbl, dip_rect(22.0, 194.0, 78.0, 216.0, scale), DT_LEFT);
            for (i, (cz, ce, _)) in PRESETS.iter().enumerate() {
                let r = preset_rect(i);
                let col = if p.hovered == Some(DlgHit::Preset(i)) { pal.chip_hover_text } else { pal.chip_text };
                draw_text_gdi(dc, fonts.footer, col, if zh { cz } else { ce }, dip_rect(r.0, r.1, r.2, r.3, scale), DT_CENTER);
            }

            for (i, (cz, ce, tok)) in COND_CHIPS.iter().enumerate() {
                let r = cond_chip_rect(i);
                let active = expr_has_token(p.live_expr, tok);
                let hov = p.hovered == Some(DlgHit::CondChip(i));
                let col = if active { pal.chip_active_text } else if hov { pal.chip_hover_text } else { pal.chip_text };
                draw_text_gdi(dc, fonts.footer, col, if zh { cz } else { ce }, dip_rect(r.0, r.1, r.2, r.3, scale), DT_CENTER);
            }

            let (msg_text, msg_col) = if let Some(err) = p.error { (err, err_col) } else { (p.preview, pal.text_secondary) };
            draw_text_gdi(dc, fonts.footer, msg_col, msg_text, dip_rect(46.0, 326.0, 400.0, 354.0, scale), DT_LEFT);

            let tip = if zh { "Enter 保存 · Esc 取消" } else { "Enter Save · Esc Cancel" };
            draw_text_gdi(dc, fonts.footer, pal.text_muted, tip, dip_rect(22.0, 382.0, 216.0, 412.0, scale), DT_LEFT);

            let cancel_txt = if zh { "取消" } else { "Cancel" };
            let save_txt = if zh { "保存并应用" } else { "Save Tag" };
            draw_text_gdi(dc, fonts.chip, pal.text_secondary, cancel_txt, dip_rect(CANCEL_BTN.0, CANCEL_BTN.1, CANCEL_BTN.2, CANCEL_BTN.3, scale), DT_CENTER);
            draw_text_gdi(dc, fonts.chip_active, Rgb(0xFF, 0xFF, 0xFF), save_txt, dip_rect(SAVE_BTN.0, SAVE_BTN.1, SAVE_BTN.2, SAVE_BTN.3, scale), DT_CENTER);

            let _ = BitBlt(screen_dc, 0, 0, w, h, Some(dc), 0, 0, SRCCOPY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_condition_tokens_cleanly() {
        assert_eq!(toggle_expr_token("", "dm:today"), "dm:today");
        assert_eq!(toggle_expr_token("dm:today", "size:>50mb"), "dm:today size:>50mb");
        assert_eq!(toggle_expr_token("dm:today size:>50mb", "DM:TODAY"), "size:>50mb");
        assert!(expr_has_token("size:>50mb dm:last7days", "dm:last7days"));
    }

    #[test]
    fn formats_human_readable_rule_preview() {
        let exts = vec!["rs".into(), "ts".into(), "py".into()];
        let zh = format_rule_preview(&exts, "dm:today size:>50mb", Lang::Zh);
        assert!(zh.contains("rs / ts / py") && zh.contains("今日修改") && zh.contains(">50mb"));
        let empty = format_rule_preview(&[], "", Lang::En);
        assert!(empty.contains("Active rule"));
    }
}
