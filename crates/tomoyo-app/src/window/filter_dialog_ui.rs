//! Pure-Rust SDF + ClearType GDI painter and hit-tester for the Quick Add/Edit Filter modal.

use crate::renderer::{
    dip_rect, draw_text_gdi, resolve_face_and_weight, Canvas, DibSurface, FontKey, FontPack, Rgb,
    ThemePalette,
};
use tomoyo_core::{Lang, TypographyConfig};
use windows::Win32::Graphics::Gdi::{BitBlt, GdiFlush, HDC, SRCCOPY, DT_CENTER, DT_LEFT};

pub(super) const DLG_W_DIP: f32 = 420.0;
pub(super) const DLG_H_DIP: f32 = 372.0;
pub(super) const HEADER_H_DIP: f32 = 56.0;
pub(super) const FOOTER_Y_DIP: f32 = 314.0;

pub(super) const FIELD_RECTS: [(f32, f32, f32, f32); 3] = [
    (20.0, 84.0, 400.0, 118.0),
    (20.0, 148.0, 400.0, 182.0),
    (20.0, 242.0, 400.0, 276.0),
];
const CLOSE_BTN: (f32, f32, f32, f32) = (376.0, 14.0, 402.0, 40.0);
const CANCEL_BTN: (f32, f32, f32, f32) = (216.0, 327.0, 288.0, 359.0);
const SAVE_BTN: (f32, f32, f32, f32) = (296.0, 327.0, 400.0, 359.0);

pub(super) const PRESETS: [(&str, &str, &str); 4] = [
    ("代码", "Code", "rs, ts, tsx, js, py, go, c, cpp, h, toml, json"),
    ("视频", "Video", "mp4, mkv, mov, avi, webm, flv"),
    ("音频", "Audio", "mp3, flac, wav, m4a, ogg, aac"),
    ("设计", "Design", "psd, fig, ai, sketch, xd, svg"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DlgHit {
    Close,
    Preset(usize),
    Cancel,
    Save,
}

fn preset_rect(idx: usize) -> (f32, f32, f32, f32) {
    let x0 = 84.0 + (idx as f32) * 54.0;
    (x0, 190.0, x0 + 48.0, 212.0)
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
    None
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

    pub fn edit_font_face<'a>(&self, typo: &'a TypographyConfig, font_regular: &'a str, font_medium: &'a str) -> &'a str {
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
        let (w_dip, h_dip) = (w as f32 / scale, h as f32 / scale);
        {
            let mut canvas = Canvas { buf: surf.pixels_mut(), w, h, scale };
            canvas.clear(pal.pop_bg);

            // Header accent pill, divider & close button
            canvas.fill_rounded_rect(20.0, 17.0, 24.0, 39.0, 2.0, pal.accent);
            canvas.fill_rect_dip(0.0, HEADER_H_DIP, w_dip, HEADER_H_DIP + 1.0, pal.divider);
            if p.hovered == Some(DlgHit::Close) {
                canvas.fill_rounded_rect(CLOSE_BTN.0, CLOSE_BTN.1, CLOSE_BTN.2, CLOSE_BTN.3, 6.0, pal.card_hover);
            }

            // 3 modern rounded input capsules
            for (i, &r) in FIELD_RECTS.iter().enumerate() {
                let is_foc = p.focused == Some(i as u8);
                canvas.fill_rounded_rect(r.0, r.1, r.2, r.3, 8.0, pal.bg);
                canvas.stroke_rounded_rect(r.0, r.1, r.2, r.3, 8.0, if is_foc { pal.accent } else { pal.border });
                if is_foc {
                    canvas.stroke_rounded_rect(r.0 + 0.6, r.1 + 0.6, r.2 - 0.6, r.3 - 0.6, 7.5, pal.accent);
                }
            }

            // Quick preset chips
            for i in 0..PRESETS.len() {
                let r = preset_rect(i);
                let hov = p.hovered == Some(DlgHit::Preset(i));
                let (bg, bd) = if hov { (pal.chip_hover_bg, pal.chip_hover_border) } else { (pal.chip_bg, pal.chip_border) };
                canvas.fill_rounded_rect(r.0, r.1, r.2, r.3, 6.0, bg);
                canvas.stroke_rounded_rect(r.0, r.1, r.2, r.3, 6.0, bd);
            }

            // Footer band & action buttons
            canvas.fill_rect_dip(0.0, FOOTER_Y_DIP, w_dip, h_dip, pal.footer_bg);
            canvas.fill_rect_dip(0.0, FOOTER_Y_DIP, w_dip, FOOTER_Y_DIP + 1.0, pal.divider);

            let (cbg, cbd) = if p.hovered == Some(DlgHit::Cancel) { (pal.card_hover, pal.text_muted) } else { (pal.pill_bg, pal.border) };
            canvas.fill_rounded_rect(CANCEL_BTN.0, CANCEL_BTN.1, CANCEL_BTN.2, CANCEL_BTN.3, 7.0, cbg);
            canvas.stroke_rounded_rect(CANCEL_BTN.0, CANCEL_BTN.1, CANCEL_BTN.2, CANCEL_BTN.3, 7.0, cbd);

            let save_col = if p.hovered == Some(DlgHit::Save) {
                if pal.is_dark { Rgb(0x8D, 0x94, 0xFF) } else { Rgb(0x2F, 0x4E, 0xCC) }
            } else {
                pal.accent
            };
            canvas.fill_rounded_rect(SAVE_BTN.0, SAVE_BTN.1, SAVE_BTN.2, SAVE_BTN.3, 7.0, save_col);

            // Crisp 1 DIP card border
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
            let sub = if zh { "支持侧栏点击与 @标签 直达过滤" } else { "Instant sidebar filter & @tag syntax" };
            draw_text_gdi(dc, fonts.title, pal.text_primary, title, dip_rect(32.0, 10.0, 366.0, 32.0, scale), DT_LEFT);
            draw_text_gdi(dc, fonts.footer, pal.text_muted, sub, dip_rect(32.0, 32.0, 366.0, 50.0, scale), DT_LEFT);
            let close_col = if p.hovered == Some(DlgHit::Close) { pal.text_primary } else { pal.text_muted };
            draw_text_gdi(dc, fonts.chip, close_col, "×", dip_rect(CLOSE_BTN.0, CLOSE_BTN.1, CLOSE_BTN.2, CLOSE_BTN.3, scale), DT_CENTER);

            let labels = if zh {
                ["标签名称", "文件扩展名", "Everything 表达式（可选）"]
            } else {
                ["Tag Name", "File Extensions", "Everything Expression (Optional)"]
            };
            for (i, lbl) in labels.iter().enumerate() {
                let y0 = FIELD_RECTS[i].1 - 21.0;
                draw_text_gdi(dc, fonts.chip_active, pal.text_secondary, lbl, dip_rect(22.0, y0, 398.0, FIELD_RECTS[i].1 - 2.0, scale), DT_LEFT);
            }

            let tpl_lbl = if zh { "快捷预设" } else { "Presets" };
            draw_text_gdi(dc, fonts.footer, pal.text_muted, tpl_lbl, dip_rect(22.0, 190.0, 82.0, 212.0, scale), DT_LEFT);
            for (i, (cz, ce, _)) in PRESETS.iter().enumerate() {
                let r = preset_rect(i);
                let col = if p.hovered == Some(DlgHit::Preset(i)) { pal.chip_hover_text } else { pal.chip_text };
                draw_text_gdi(dc, fonts.footer, col, if zh { cz } else { ce }, dip_rect(r.0, r.1, r.2, r.3, scale), DT_CENTER);
            }

            if let Some(err) = p.error {
                let ecol = if pal.is_dark { Rgb(0xF8, 0x71, 0x71) } else { Rgb(0xDC, 0x26, 0x26) };
                draw_text_gdi(dc, fonts.footer, ecol, err, dip_rect(20.0, 286.0, 400.0, 306.0, scale), DT_LEFT);
            }
            let tip = if zh { "Enter 保存 · Esc 取消" } else { "Enter Save · Esc Cancel" };
            draw_text_gdi(dc, fonts.footer, pal.text_muted, tip, dip_rect(20.0, 327.0, 208.0, 359.0, scale), DT_LEFT);

            let cancel_txt = if zh { "取消" } else { "Cancel" };
            let save_txt = if zh { "保存并应用" } else { "Save Tag" };
            draw_text_gdi(dc, fonts.chip, pal.text_secondary, cancel_txt, dip_rect(CANCEL_BTN.0, CANCEL_BTN.1, CANCEL_BTN.2, CANCEL_BTN.3, scale), DT_CENTER);
            draw_text_gdi(dc, fonts.chip_active, Rgb(0xFF, 0xFF, 0xFF), save_txt, dip_rect(SAVE_BTN.0, SAVE_BTN.1, SAVE_BTN.2, SAVE_BTN.3, scale), DT_CENTER);

            let _ = BitBlt(screen_dc, 0, 0, w, h, Some(dc), 0, 0, SRCCOPY);
        }
    }
}
