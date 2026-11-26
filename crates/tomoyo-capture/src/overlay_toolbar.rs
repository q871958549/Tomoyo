//! Contextual floating toolbar & recording setup sub-bar geometry for `tomoyo-capture`.

use crate::annotate::{AnnotColor, ToolKind, COLOR_PRESETS};
use crate::screen::ScreenRect;
use tomoyo_core::CaptureToolHotkeys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum ToolbarCmd {
    Tool(ToolKind),
    Color(usize),
    Thickness(i32),
    ShapeFilled(bool),
    MosaicBlur(bool),
    TextBold(bool),
    Undo,
    ScrollLong,
    Record,
    RecFormat(bool),
    RecFps(u8),
    RecAudio(u8),
    RecCursor,
    RecStart,
    Pin,
    Ocr,
    OcrCopy,
    Translate,
    Save,
    Copy,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct ToolbarHit {
    pub cmd: ToolbarCmd,
    pub rect: ScreenRect,
    pub tooltip: String,
    pub pill_text: &'static str,
    pub active: bool,
    pub accent: bool,
    pub danger: bool,
    pub swatch: Option<AnnotColor>,
    pub thickness_dot: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolbarLayout {
    pub main_bar: Option<ScreenRect>,
    pub sub_bar: Option<ScreenRect>,
    pub dividers: Vec<(i32, i32, i32)>,
    pub hits: Vec<ToolbarHit>,
}

pub struct ToolbarStyleState {
    pub active_tool: ToolKind,
    pub active_color: usize,
    pub active_thickness: i32,
    pub text_size: i32,
    pub text_bold: bool,
    pub shape_filled: bool,
    pub mosaic_blur: bool,
    pub allow_record: bool,
    pub record_open: bool,
    pub rec_is_gif: bool,
    pub rec_fps: u8,
    pub rec_audio: u8,
    pub rec_cursor: bool,
}

impl Default for ToolbarStyleState {
    fn default() -> Self {
        Self {
            active_tool: ToolKind::Select,
            active_color: 0,
            active_thickness: 4,
            text_size: 22,
            text_bold: false,
            shape_filled: false,
            mosaic_blur: false,
            allow_record: true,
            record_open: false,
            rec_is_gif: false,
            rec_fps: 30,
            rec_audio: 0,
            rec_cursor: true,
        }
    }
}

fn tip_with_key(label: &str, key: &str) -> String {
    if key.is_empty() { label.to_string() } else { format!("{label} ({key})") }
}

pub fn build_toolbar_layout(
    sel: ScreenRect,
    screen_w: i32,
    screen_h: i32,
    st: &ToolbarStyleState,
    hk: &CaptureToolHotkeys,
) -> ToolbarLayout {
    enum Slot {
        Btn(ToolbarCmd, String, i32, bool, bool, bool),
        Sep,
    }
    let mut slots = vec![
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Rect), tip_with_key("矩形", &hk.rect), 30, st.active_tool == ToolKind::Rect, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Ellipse), tip_with_key("椭圆", &hk.ellipse), 30, st.active_tool == ToolKind::Ellipse, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Arrow), tip_with_key("箭头 · 拖中点自由弯折", &hk.arrow), 30, st.active_tool == ToolKind::Arrow, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Pen), tip_with_key("画笔", &hk.pen), 30, st.active_tool == ToolKind::Pen, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Highlighter), tip_with_key("荧光笔", &hk.highlighter), 30, st.active_tool == ToolKind::Highlighter, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Mosaic), tip_with_key("马赛克/模糊", &hk.mosaic), 30, st.active_tool == ToolKind::Mosaic, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::StepNumber), tip_with_key("序号", &hk.step), 30, st.active_tool == ToolKind::StepNumber, false, false),
        Slot::Btn(ToolbarCmd::Tool(ToolKind::Text), tip_with_key("文字", &hk.text), 30, st.active_tool == ToolKind::Text, false, false),
        Slot::Sep,
        Slot::Btn(ToolbarCmd::Undo, "撤销 (Ctrl+Z)".into(), 30, false, false, false),
        Slot::Sep,
        Slot::Btn(ToolbarCmd::ScrollLong, tip_with_key("长截图 (纵/横)", &hk.scroll), 30, false, false, false),
    ];
    if st.allow_record {
        slots.push(Slot::Btn(ToolbarCmd::Record, tip_with_key("屏幕录制 MP4/GIF", &hk.record), 30, st.record_open, false, false));
    }
    slots.extend([
        Slot::Btn(ToolbarCmd::Pin, tip_with_key("贴图", &hk.pin), 30, false, false, false),
        Slot::Btn(
            ToolbarCmd::Ocr,
            format!("识字 OCR ({}) · 直接复制 ({})", hk.ocr, hk.ocr_copy),
            30,
            false,
            false,
            false,
        ),
        Slot::Btn(ToolbarCmd::Translate, tip_with_key("截图翻译", &hk.translate), 30, false, false, false),
        Slot::Btn(ToolbarCmd::Save, "保存 PNG (Ctrl+S)".into(), 30, false, false, false),
        Slot::Sep,
        Slot::Btn(ToolbarCmd::Cancel, "取消 (Esc)".into(), 30, false, false, true),
        Slot::Btn(ToolbarCmd::Copy, "复制完成 (Enter)".into(), 38, false, true, false),
    ]);

    let mut total_w = 12;
    for s in &slots {
        total_w += match s { Slot::Btn(_, _, w, _, _, _) => w + 2, Slot::Sep => 9 };
    }
    let bar_h = 40;
    let show_sub = (st.allow_record && st.record_open) || st.active_tool != ToolKind::Select;
    let sub_h = 34;
    let stack_h = if show_sub { bar_h + 6 + sub_h } else { bar_h };
    let bar_x = (sel.right() - total_w).clamp(8, (screen_w - total_w - 8).max(8));
    let below_fits = sel.bottom() + 8 + stack_h <= screen_h - 6;
    let above_fits = sel.y - 8 - stack_h >= 6;
    let (bar_y, sub_above) = if below_fits {
        (sel.bottom() + 8, false)
    } else if above_fits {
        (sel.y - 8 - bar_h, true)
    } else {
        ((sel.bottom() - bar_h - 8).max(8), true)
    };

    let main_bar = ScreenRect { x: bar_x, y: bar_y, w: total_w, h: bar_h };
    let mut hits = Vec::with_capacity(32);
    let mut dividers = Vec::with_capacity(8);
    let mut cx = bar_x + 7;
    for s in slots {
        match s {
            Slot::Btn(cmd, tooltip, w, active, accent, danger) => {
                hits.push(ToolbarHit {
                    cmd, rect: ScreenRect { x: cx, y: bar_y + 5, w, h: 30 }, tooltip,
                    pill_text: "", active, accent, danger, swatch: None, thickness_dot: None,
                });
                cx += w + 2;
            }
            Slot::Sep => {
                dividers.push((cx + 3, bar_y + 11, bar_y + bar_h - 11));
                cx += 9;
            }
        }
    }

    let sub_y = if sub_above { (bar_y - 6 - sub_h).max(6) } else { (bar_y + bar_h + 6).min(screen_h - sub_h - 6) };
    let sub_bar = if st.allow_record && st.record_open {
        Some(build_record_sub_bar(bar_x, sub_y, sub_h, screen_w, st, &mut dividers, &mut hits))
    } else if show_sub {
        Some(build_annot_sub_bar(bar_x, sub_y, sub_h, screen_w, st, &mut dividers, &mut hits))
    } else {
        None
    };

    ToolbarLayout { main_bar: Some(main_bar), sub_bar, dividers, hits }
}

fn build_record_sub_bar(
    bar_x: i32,
    sub_y: i32,
    sub_h: i32,
    screen_w: i32,
    st: &ToolbarStyleState,
    dividers: &mut Vec<(i32, i32, i32)>,
    hits: &mut Vec<ToolbarHit>,
) -> ScreenRect {
    let fmt_items = [
        (ToolbarCmd::RecFormat(false), "MP4", 38, !st.rec_is_gif, false),
        (ToolbarCmd::RecFormat(true), "GIF", 36, st.rec_is_gif, false),
    ];
    let fps_items = [
        (ToolbarCmd::RecFps(15), "15fps", 42, st.rec_fps == 15, false),
        (ToolbarCmd::RecFps(30), "30fps", 42, st.rec_fps == 30, false),
        (ToolbarCmd::RecFps(60), "60fps", 42, st.rec_fps == 60, false),
    ];
    let audio_items: &[(ToolbarCmd, &str, i32, bool, bool)] = if st.rec_is_gif {
        &[]
    } else {
        &[
            (ToolbarCmd::RecAudio(0), "静音", 36, st.rec_audio == 0, false),
            (ToolbarCmd::RecAudio(1), "系统声", 44, st.rec_audio == 1, false),
            (ToolbarCmd::RecAudio(2), "麦", 26, st.rec_audio == 2, false),
            (ToolbarCmd::RecAudio(3), "双音", 36, st.rec_audio == 3, false),
        ]
    };
    let tail_items = [
        (ToolbarCmd::RecCursor, "光标", 38, st.rec_cursor, false),
        (ToolbarCmd::RecStart, "● 开始录制", 84, false, true),
    ];
    type PillSpec<'a> = (ToolbarCmd, &'a str, i32, bool, bool);
    let groups: [&[PillSpec<'_>]; 4] = [&fmt_items, &fps_items, audio_items, &tail_items];
    let mut rec_w = 12;
    let mut non_empty = 0;
    for g in groups {
        if g.is_empty() { continue; }
        if non_empty > 0 { rec_w += 9; }
        non_empty += 1;
        for (_, _, w, _, _) in g { rec_w += *w + 3; }
    }
    let sub_x = bar_x.clamp(8, (screen_w - rec_w - 8).max(8));
    let mut sx = sub_x + 7;
    let mut placed_groups = 0;
    for g in groups {
        if g.is_empty() { continue; }
        if placed_groups > 0 {
            dividers.push((sx + 2, sub_y + 9, sub_y + sub_h - 9));
            sx += 8;
        }
        placed_groups += 1;
        for &(cmd, lbl, w, act, danger) in g {
            hits.push(ToolbarHit {
                cmd, rect: ScreenRect { x: sx, y: sub_y + 5, w, h: 24 }, tooltip: String::new(),
                pill_text: lbl, active: act, accent: false, danger, swatch: None, thickness_dot: None,
            });
            sx += w + 3;
        }
    }
    ScreenRect { x: sub_x, y: sub_y, w: (sx - sub_x + 5).max(80), h: sub_h }
}

fn build_annot_sub_bar(
    bar_x: i32,
    sub_y: i32,
    sub_h: i32,
    screen_w: i32,
    st: &ToolbarStyleState,
    dividers: &mut Vec<(i32, i32, i32)>,
    hits: &mut Vec<ToolbarHit>,
) -> ScreenRect {
    let style_pills: Vec<(ToolbarCmd, &'static str, bool)> = match st.active_tool {
        ToolKind::Rect | ToolKind::Ellipse => vec![(ToolbarCmd::ShapeFilled(!st.shape_filled), "填充", st.shape_filled)],
        ToolKind::Mosaic => vec![
            (ToolbarCmd::MosaicBlur(false), "马赛克", !st.mosaic_blur),
            (ToolbarCmd::MosaicBlur(true), "模糊", st.mosaic_blur),
        ],
        ToolKind::Text => vec![(ToolbarCmd::TextBold(!st.text_bold), "加粗", st.text_bold)],
        _ => Vec::new(),
    };
    let style_w: i32 = if style_pills.is_empty() { 0 } else { style_pills.len() as i32 * 44 + 9 };
    let sub_w = 12 + 3 * 26 + style_w + 9 + COLOR_PRESETS.len() as i32 * 24 + 4;
    let sub_x = bar_x.clamp(8, (screen_w - sub_w - 8).max(8));
    let mut sx = sub_x + 8;

    let is_text = st.active_tool == ToolKind::Text;
    let size_dots = if is_text {
        [
            (2, "小字 16px · 滚轮可无级调节", st.text_size <= 18),
            (4, "中字 22px · 滚轮可无级调节", (19..=27).contains(&st.text_size)),
            (8, "大字 32px · 滚轮可无级调节", st.text_size >= 28),
        ]
    } else {
        [
            (2, "细 · 滚轮可无级调节", (st.active_thickness - 2).abs() <= 1),
            (4, "中 · 滚轮可无级调节", (st.active_thickness - 4).abs() <= 1),
            (8, "粗 · 滚轮可无级调节", (st.active_thickness - 8).abs() <= 1),
        ]
    };
    for (t, tip, active) in size_dots {
        hits.push(ToolbarHit {
            cmd: ToolbarCmd::Thickness(t),
            rect: ScreenRect { x: sx, y: sub_y + 4, w: 24, h: 26 },
            tooltip: tip.into(),
            pill_text: "",
            active,
            accent: false, danger: false, swatch: None, thickness_dot: Some(t),
        });
        sx += 26;
    }
    if !style_pills.is_empty() {
        dividers.push((sx + 3, sub_y + 9, sub_y + sub_h - 9));
        sx += 9;
        for (cmd, lbl, act) in style_pills {
            hits.push(ToolbarHit {
                cmd, rect: ScreenRect { x: sx, y: sub_y + 5, w: 42, h: 24 },
                tooltip: String::new(), pill_text: lbl, active: act,
                accent: false, danger: false, swatch: None, thickness_dot: None,
            });
            sx += 44;
        }
    }
    dividers.push((sx + 3, sub_y + 9, sub_y + sub_h - 9));
    sx += 9;
    for (i, &col) in COLOR_PRESETS.iter().enumerate() {
        hits.push(ToolbarHit {
            cmd: ToolbarCmd::Color(i),
            rect: ScreenRect { x: sx, y: sub_y + 5, w: 22, h: 24 },
            tooltip: String::new(), pill_text: "", active: st.active_color == i,
            accent: false, danger: false, swatch: Some(col), thickness_dot: None,
        });
        sx += 24;
    }
    ScreenRect { x: sub_x, y: sub_y, w: sub_w, h: sub_h }
}

pub fn dim_badge_rect(sel: ScreenRect, screen_w: i32) -> ScreenRect {
    let (w, h) = (128, 24);
    let x = sel.x.clamp(8, (screen_w - w - 8).max(8));
    let y = if sel.y >= 32 { sel.y - h - 6 } else { sel.y + 8 };
    ScreenRect { x, y, w, h }
}

pub fn rec_hint_rect(sub: ScreenRect, screen_w: i32, screen_h: i32) -> ScreenRect {
    let (w, h) = (368, 24);
    let x = sub.x.clamp(8, (screen_w - w - 8).max(8));
    let y = if sub.bottom() + h + 10 <= screen_h { sub.bottom() + 6 } else { (sub.y - h - 6).max(6) };
    ScreenRect { x, y, w, h }
}

#[inline]
fn rects_overlap(a: ScreenRect, b: ScreenRect, pad: i32) -> bool {
    a.x < b.right() + pad && a.right() + pad > b.x && a.y < b.bottom() + pad && a.bottom() + pad > b.y
}

pub fn snap_badge_rect_avoiding(
    snap_r: ScreenRect,
    screen_w: i32,
    screen_h: i32,
    avoid: Option<ScreenRect>,
) -> ScreenRect {
    let (w, h) = (248, 24);
    let clamp_x = |x: i32| x.clamp(8, (screen_w - w - 8).max(8));
    let clamp_y = |y: i32| y.clamp(8, (screen_h - h - 8).max(8));
    let top_y = if snap_r.y >= 32 { snap_r.y - h - 6 } else { clamp_y(snap_r.y + 8) };
    let bot_y = if snap_r.bottom() + h + 10 <= screen_h { snap_r.bottom() + 6 } else { clamp_y(snap_r.bottom() - h - 8) };
    let cands = [
        ScreenRect { x: clamp_x(snap_r.x), y: top_y, w, h },
        ScreenRect { x: clamp_x(snap_r.x + 8), y: clamp_y(snap_r.y + 8), w, h },
        ScreenRect { x: clamp_x(snap_r.right() - w), y: top_y, w, h },
        ScreenRect { x: clamp_x(snap_r.x), y: bot_y, w, h },
    ];
    if let Some(av) = avoid {
        for &c in &cands {
            if !rects_overlap(c, av, 4) {
                return c;
            }
        }
    }
    cands[0]
}

#[allow(dead_code)]
pub fn snap_badge_rect(snap_r: ScreenRect, screen_w: i32) -> ScreenRect {
    snap_badge_rect_avoiding(snap_r, screen_w, screen_w.max(1080), None)
}

pub fn tooltip_rect_for(btn: &ToolbarHit, screen_w: i32, screen_h: i32) -> Option<ScreenRect> {
    if btn.tooltip.is_empty() { return None; }
    let text_w: i32 = btn.tooltip.chars().map(|c| if c.is_ascii() { 7 } else { 13 }).sum();
    let (w, h) = ((text_w + 16).clamp(56, 220), 24);
    let x = (btn.rect.x + btn.rect.w / 2 - w / 2).clamp(8, (screen_w - w - 8).max(8));
    let y = if btn.rect.y >= 36 { btn.rect.y - h - 7 } else { (btn.rect.bottom() + 7).min(screen_h - h - 6) };
    Some(ScreenRect { x, y, w, h })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_and_record_subbar_stay_inside_viewport() {
        let st = ToolbarStyleState { active_tool: ToolKind::Arrow, ..Default::default() };
        let lay = build_toolbar_layout(ScreenRect { x: 50, y: 1050, w: 200, h: 20 }, 1920, 1080, &st, &CaptureToolHotkeys::default());
        let bar = lay.main_bar.expect("main bar");
        assert!(bar.x >= 0 && bar.right() <= 1920 && bar.y >= 0 && bar.bottom() <= 1080);
        let sub = lay.sub_bar.expect("sub bar");
        assert!(sub.x >= 0 && sub.right() <= 1920 && sub.y >= 0 && sub.bottom() <= 1080);
        assert!(lay.hits.iter().any(|h| h.cmd == ToolbarCmd::Record));
        assert!(lay.hits.iter().filter(|h| !h.pill_text.is_empty()).count() == 0);

        let rec_st = ToolbarStyleState { record_open: true, rec_is_gif: false, ..Default::default() };
        let rec_lay = build_toolbar_layout(ScreenRect { x: 400, y: 200, w: 500, h: 400 }, 1920, 1080, &rec_st, &CaptureToolHotkeys::default());
        let rec_sub = rec_lay.sub_bar.expect("record sub bar");
        assert!(rec_sub.x >= 0 && rec_sub.right() <= 1920 && rec_sub.y >= 0 && rec_sub.bottom() <= 1080);
        assert!(rec_lay.hits.iter().any(|h| h.cmd == ToolbarCmd::RecStart));

        let win_st = ToolbarStyleState { allow_record: false, record_open: true, ..Default::default() };
        let win_lay = build_toolbar_layout(ScreenRect { x: 400, y: 200, w: 500, h: 400 }, 1920, 1080, &win_st, &CaptureToolHotkeys::default());
        assert!(!win_lay.hits.iter().any(|h| h.cmd == ToolbarCmd::Record));
        assert!(win_lay.sub_bar.is_none());

        let snap_r = ScreenRect { x: 200, y: 200, w: 600, h: 420 };
        let loupe_blocking = ScreenRect { x: 200, y: 165, w: 164, h: 192 };
        let reloc = snap_badge_rect_avoiding(snap_r, 1920, 1080, Some(loupe_blocking));
        assert!(!rects_overlap(reloc, loupe_blocking, 4));
    }
}
