//! Smart region & window pre-selection combining Z-order desktop windows, monitors, and
//! structural visual panel detection directly on the captured BGRA desktop buffer.

use crate::screen::{DesktopSnapshot, ScreenRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    Panel,
    Pane,
    Window,
    Monitor,
    FullScreen,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Panel => "区块",
            Self::Pane => "面板",
            Self::Window => "窗口",
            Self::Monitor => "屏幕",
            Self::FullScreen => "全屏",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapCandidate {
    pub rect: ScreenRect,
    pub kind: SnapKind,
}

pub struct SnapContext {
    pub windows: Vec<ScreenRect>,
    pub fullscreen_wins: Vec<ScreenRect>,
    pub monitors: Vec<ScreenRect>,
    pub screen_w: i32,
    pub screen_h: i32,
    pub cycle_count: usize,
    cycle_idx: usize,
    locked_cycle: bool,
    last_query: (i32, i32),
    last_candidates: Vec<SnapCandidate>,
}

impl SnapContext {
    pub fn new(windows: Vec<ScreenRect>, monitors: Vec<ScreenRect>, screen_w: i32, screen_h: i32) -> Self {
        Self::with_fullscreen(windows, Vec::new(), monitors, screen_w, screen_h)
    }

    pub fn with_fullscreen(
        windows: Vec<ScreenRect>, fullscreen_wins: Vec<ScreenRect>, monitors: Vec<ScreenRect>, screen_w: i32, screen_h: i32,
    ) -> Self {
        let mut mons = monitors;
        if mons.is_empty() && screen_w > 0 && screen_h > 0 {
            mons.push(ScreenRect { x: 0, y: 0, w: screen_w, h: screen_h });
        }
        Self {
            windows, fullscreen_wins, monitors: mons, screen_w, screen_h,
            cycle_count: 0, cycle_idx: 0, locked_cycle: false,
            last_query: (i32::MIN, i32::MIN), last_candidates: Vec::new(),
        }
    }

    /// Computes ordered snap candidates at `(cx, cy)` with anti-jitter container lock.
    pub fn hover_at(&mut self, snap: &DesktopSnapshot, cx: i32, cy: i32) -> Option<SnapCandidate> {
        let moved = (cx as i64 - self.last_query.0 as i64).abs() + (cy as i64 - self.last_query.1 as i64).abs();
        let active_prev = self.last_candidates.get(self.cycle_idx).copied();
        let still_inside = active_prev.is_some_and(|c| c.rect.contains(cx, cy));
        let edge_hold = active_prev.is_some_and(|c| {
            matches!(c.kind, SnapKind::Panel | SnapKind::Pane) && padded_contains(c.rect, cx, cy, 6)
        });
        if (still_inside || edge_hold) && !self.last_candidates.is_empty() && moved < if self.locked_cycle { 28 } else { 14 } {
            return active_prev;
        }
        if edge_hold && !still_inside {
            self.last_query = (cx, cy);
            return active_prev;
        }
        let next = self.build_candidates(snap, cx, cy);
        if next.is_empty() {
            self.last_candidates.clear();
            self.cycle_count = 0; self.cycle_idx = 0; self.locked_cycle = false;
            return None;
        }
        self.last_query = (cx, cy);
        if self.locked_cycle && still_inside
            && active_prev.is_some_and(|p| next.iter().any(|c| approx_same(c.rect, p.rect, 12)))
        {
            return active_prev;
        }
        if still_inside && let Some(prev) = active_prev
            && matches!(prev.kind, SnapKind::Panel | SnapKind::Pane)
        {
            let top = next[0];
            if approx_same(top.rect, prev.rect, 12) || !strictly_smaller(top.rect, prev.rect) {
                return active_prev;
            }
        }
        self.locked_cycle = false;
        let unchanged = self.last_candidates.first().map(|c| c.rect) == next.first().map(|c| c.rect);
        self.cycle_idx = if !unchanged || !still_inside { 0 } else { self.cycle_idx.min(next.len() - 1) };
        self.cycle_count = next.len();
        self.last_candidates = next;
        self.last_candidates.get(self.cycle_idx).copied()
    }

    pub fn cycle_next(&mut self, snap: &DesktopSnapshot, cx: i32, cy: i32) -> Option<SnapCandidate> {
        if self.last_candidates.is_empty() {
            let _ = self.hover_at(snap, cx, cy);
        }
        if self.last_candidates.is_empty() {
            return None;
        }
        self.cycle_idx = (self.cycle_idx + 1) % self.last_candidates.len();
        self.locked_cycle = true;
        self.last_candidates.get(self.cycle_idx).copied()
    }

    fn build_candidates(&self, snap: &DesktopSnapshot, cx: i32, cy: i32) -> Vec<SnapCandidate> {
        let mon = self
            .monitors
            .iter()
            .copied()
            .find(|m| m.contains(cx, cy))
            .unwrap_or(ScreenRect { x: 0, y: 0, w: self.screen_w, h: self.screen_h });
        let full = ScreenRect { x: 0, y: 0, w: self.screen_w, h: self.screen_h };
        let win = self
            .windows
            .iter()
            .copied()
            .find(|r| r.contains(cx, cy))
            .map(|r| r.clamp_to(self.screen_w, self.screen_h))
            .filter(|r| r.w >= 24 && r.h >= 20);
        let is_fs_app = win.is_some_and(|w| self.fullscreen_wins.contains(&w));

        let mut list = Vec::with_capacity(5);
        if is_fs_app {
            if let Some(w) = win { push_unique(&mut list, SnapCandidate { rect: w, kind: SnapKind::Window }); }
            push_unique(&mut list, SnapCandidate { rect: mon, kind: SnapKind::Monitor });
            push_unique(&mut list, SnapCandidate { rect: full, kind: SnapKind::FullScreen });
            return list;
        }

        let search_bounds = win.unwrap_or(mon);
        let tight = scan_container_at(snap, search_bounds, cx, cy, 52, 110, 64);
        let pane = scan_container_at(snap, search_bounds, cx, cy, 112, 160, 96);
        let mon_area = (mon.w as i64) * (mon.h as i64).max(1);
        let win_is_large = win.is_some_and(|w| (w.w as i64) * (w.h as i64) * 10 >= mon_area * 5);

        if win_is_large || win.is_none() {
            if let Some(p) = tight { push_unique(&mut list, SnapCandidate { rect: p, kind: SnapKind::Panel }); }
            if let Some(p) = pane { push_unique(&mut list, SnapCandidate { rect: p, kind: SnapKind::Pane }); }
            if let Some(w) = win { push_unique(&mut list, SnapCandidate { rect: w, kind: SnapKind::Window }); }
        } else {
            if let Some(w) = win { push_unique(&mut list, SnapCandidate { rect: w, kind: SnapKind::Window }); }
            if let Some(p) = tight { push_unique(&mut list, SnapCandidate { rect: p, kind: SnapKind::Panel }); }
            if let Some(p) = pane { push_unique(&mut list, SnapCandidate { rect: p, kind: SnapKind::Pane }); }
        }
        push_unique(&mut list, SnapCandidate { rect: mon, kind: SnapKind::Monitor });
        push_unique(&mut list, SnapCandidate { rect: full, kind: SnapKind::FullScreen });
        list
    }
}

#[inline]
fn padded_contains(r: ScreenRect, px: i32, py: i32, pad: i32) -> bool {
    px >= r.x - pad && px < r.right() + pad && py >= r.y - pad && py < r.bottom() + pad
}

#[inline]
fn approx_same(a: ScreenRect, b: ScreenRect, tol: i32) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol && (a.w - b.w).abs() <= tol * 2 && (a.h - b.h).abs() <= tol * 2
}

#[inline]
fn strictly_smaller(inner: ScreenRect, outer: ScreenRect) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.right() <= outer.right() && inner.bottom() <= outer.bottom()
        && (inner.w as i64) * (inner.h as i64) * 100 < (outer.w as i64) * (outer.h as i64) * 65
}

fn push_unique(out: &mut Vec<SnapCandidate>, item: SnapCandidate) {
    if !item.rect.is_valid() || out.iter().any(|c| approx_same(c.rect, item.rect, 6)) {
        return;
    }
    out.push(item);
}

#[inline]
fn px_delta(snap: &DesktopSnapshot, x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
    let (r0, g0, b0) = snap.pixel_rgb(x0, y0);
    let (r1, g1, b1) = snap.pixel_rgb(x1, y1);
    (r0 as i32 - r1 as i32).abs() + (g0 as i32 - g1 as i32).abs() + (b0 as i32 - b1 as i32).abs()
}

fn is_ui_row_edge(snap: &DesktopSnapshot, ya: i32, yb: i32, x0: i32, x1: i32) -> bool {
    let span = x1 - x0;
    if span < 28 || ya < 0 || yb >= snap.height { return false; }
    let samples = 15;
    let dx = (span / 20).clamp(2, 6);
    let (mut steps, mut fa, mut fb) = (0, 0, 0);
    for i in 0..samples {
        let x = x0 + (span * i) / (samples - 1);
        if px_delta(snap, x, ya, x, yb) >= 18 { steps += 1; }
        let xn = (x + dx).min(x1);
        if px_delta(snap, x, ya, xn, ya) <= 18 { fa += 1; }
        if px_delta(snap, x, yb, xn, yb) <= 18 { fb += 1; }
    }
    steps * 10 >= samples * 8 && fa * 10 >= samples * 7 && fb * 10 >= samples * 7
}

fn is_ui_col_edge(snap: &DesktopSnapshot, xa: i32, xb: i32, y0: i32, y1: i32) -> bool {
    let span = y1 - y0;
    if span < 24 || xa < 0 || xb >= snap.width { return false; }
    let samples = 15;
    let dy = (span / 20).clamp(2, 6);
    let (mut steps, mut fa, mut fb) = (0, 0, 0);
    for i in 0..samples {
        let y = y0 + (span * i) / (samples - 1);
        if px_delta(snap, xa, y, xb, y) >= 18 { steps += 1; }
        let yn = (y + dy).min(y1);
        if px_delta(snap, xa, y, xa, yn) <= 18 { fa += 1; }
        if px_delta(snap, xb, y, xb, yn) <= 18 { fb += 1; }
    }
    steps * 10 >= samples * 8 && fa * 10 >= samples * 7 && fb * 10 >= samples * 7
}

fn find_horiz_edge(snap: &DesktopSnapshot, b: ScreenRect, cx: i32, cy: i32, span: i32, up: bool) -> (i32, bool) {
    let hw = span.min((b.w / 2 - 4).max(16));
    let mut x0 = (cx - hw).max(b.x + 2);
    let x1 = (x0 + hw * 2).min(b.right() - 2);
    if x1 - x0 < hw * 2 { x0 = (x1 - hw * 2).max(b.x + 2); }
    if up {
        for y in (b.y + 2..=cy - 6).rev() {
            if is_ui_row_edge(snap, y - 1, y, x0, x1) { return (y, true); }
        }
        (b.y, false)
    } else {
        for y in cy + 6..=b.bottom() - 2 {
            if is_ui_row_edge(snap, y - 1, y, x0, x1) { return (y, true); }
        }
        (b.bottom(), false)
    }
}

fn find_vert_edge(snap: &DesktopSnapshot, b: ScreenRect, y0: i32, y1: i32, cx: i32, left: bool) -> (i32, bool) {
    let pad = ((y1 - y0) / 8).clamp(6, 18);
    let (vy0, vy1) = (y0 + pad, y1 - pad);
    if left {
        for x in (b.x + 2..=cx - 6).rev() {
            if is_ui_col_edge(snap, x - 1, x, vy0, vy1) { return (x, true); }
        }
        (b.x, false)
    } else {
        for x in cx + 6..=b.right() - 2 {
            if is_ui_col_edge(snap, x - 1, x, vy0, vy1) { return (x, true); }
        }
        (b.right(), false)
    }
}

/// Detects an enclosing UI card, sidebar, header, or split pane around `(cx, cy)`.
pub fn scan_container_at(
    snap: &DesktopSnapshot, bounds: ScreenRect, cx: i32, cy: i32, half_span: i32, min_w: i32, min_h: i32,
) -> Option<ScreenRect> {
    let b = bounds.clamp_to(snap.width, snap.height);
    if !b.contains(cx, cy) || b.w < 96 || b.h < 64 { return None; }
    let (yt, ft) = find_horiz_edge(snap, b, cx, cy, half_span, true);
    let (yb, fb) = find_horiz_edge(snap, b, cx, cy, half_span, false);
    if yb - yt < min_h { return None; }
    let (xl, fl) = find_vert_edge(snap, b, yt, yb, cx, true);
    let (xr, fr) = find_vert_edge(snap, b, yt, yb, cx, false);
    let cand = ScreenRect::from_points(xl, yt, xr, yb);
    if cand.w < min_w || cand.h < min_h { return None; }
    if cand.w * 100 >= b.w * 94 && cand.h * 100 >= b.h * 94 { return None; }
    let interior_edges = u8::from(ft) + u8::from(fb) + u8::from(fl) + u8::from(fr);
    if interior_edges == 0 { return None; }
    let ix = (cand.w / 8).clamp(6, 20);
    let iy = (cand.h / 8).clamp(6, 20);
    let ok_t = !ft || (-1..=1).any(|d| is_ui_row_edge(snap, yt + d - 1, yt + d, xl + ix, xr - ix));
    let ok_b = !fb || (-1..=1).any(|d| is_ui_row_edge(snap, yb + d - 1, yb + d, xl + ix, xr - ix));
    let ok_l = !fl || (-1..=1).any(|d| is_ui_col_edge(snap, xl + d - 1, xl + d, yt + iy, yb - iy));
    let ok_r = !fr || (-1..=1).any(|d| is_ui_col_edge(snap, xr + d - 1, xr + d, yt + iy, yb - iy));
    (ok_t && ok_b && ok_l && ok_r).then_some(cand)
}

#[cfg(target_os = "linux")]
fn json_rect(v: &serde_json::Value, ox: f64, oy: f64, sx: f64, sy: f64, mw: i32, mh: i32) -> ScreenRect {
    let f = |k| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    ScreenRect {
        x: ((f("x") - ox) * sx).round() as i32,
        y: ((f("y") - oy) * sy).round() as i32,
        w: (f("w") * sx).round() as i32,
        h: (f("h") * sy).round() as i32,
    }
    .clamp_to(mw, mh)
}

#[cfg(target_os = "linux")]
pub fn build_linux_snap_context(snap: &DesktopSnapshot) -> (SnapContext, (i32, i32)) {
    use std::ffi::{c_char, c_int, c_void, CStr};
    unsafe extern "C" {
        fn tomoyo_cap_monitors_json() -> *mut c_char;
        fn tomoyo_cap_kwin_snapshot_info() -> *mut c_char;
        fn tomoyo_cap_x11_windows_json(out_cx: *mut c_int, out_cy: *mut c_int) -> *mut c_char;
        fn tomoyo_cap_free(p: *mut c_void);
    }
    let take = |ptr: *mut c_char| -> Option<String> {
        if ptr.is_null() { return None; }
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
        unsafe { tomoyo_cap_free(ptr.cast()); }
        Some(s)
    };

    let mut monitors = Vec::new();
    if let Some(mjs) = take(unsafe { tomoyo_cap_monitors_json() })
        && let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&mjs)
    {
        for v in arr {
            let s = v.get("s").and_then(|x| x.as_f64()).unwrap_or(1.0).max(1.0);
            let r = json_rect(&v, 0.0, 0.0, s, s, snap.width, snap.height);
            if r.is_valid() { monitors.push(r); }
        }
    }

    let (mut windows, mut fullscreen_wins) = (Vec::new(), Vec::new());
    let mut cursor = (snap.width / 2, snap.height / 2);
    if let Some(kjs) = take(unsafe { tomoyo_cap_kwin_snapshot_info() })
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&kjs)
    {
        let gf = |k, def: f64| val.get(k).and_then(|x| x.as_f64()).unwrap_or(def);
        let (vw, vh) = (gf("vw", snap.width as f64).max(1.0), gf("vh", snap.height as f64).max(1.0));
        let (vx, vy) = (gf("vx", 0.0), gf("vy", 0.0));
        let (sx, sy) = (snap.width as f64 / vw, snap.height as f64 / vh);
        cursor = (
            (((gf("cx", vw * 0.5) - vx) * sx).round() as i32).clamp(0, snap.width - 1),
            (((gf("cy", vh * 0.5) - vy) * sy).round() as i32).clamp(0, snap.height - 1),
        );
        if let Some(arr) = val.get("wins").and_then(|x| x.as_array()) {
            for wv in arr {
                let r = json_rect(wv, vx, vy, sx, sy, snap.width, snap.height);
                if r.w >= 48 && r.h >= 24 && windows.len() < 64 {
                    if wv.get("fs").and_then(|x| x.as_i64()).unwrap_or(0) != 0 { fullscreen_wins.push(r); }
                    windows.push(r);
                }
            }
        }
    } else {
        let (mut rx, mut ry) = (0 as c_int, 0 as c_int);
        if let Some(xjs) = take(unsafe { tomoyo_cap_x11_windows_json(&mut rx, &mut ry) }) {
            if rx > 0 || ry > 0 { cursor = (rx.clamp(0, snap.width - 1), ry.clamp(0, snap.height - 1)); }
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&xjs) {
                for wv in arr {
                    let r = json_rect(&wv, 0.0, 0.0, 1.0, 1.0, snap.width, snap.height);
                    if r.w >= 48 && r.h >= 24 && windows.len() < 64 { windows.push(r); }
                }
            }
        }
    }

    (SnapContext::with_fullscreen(windows, fullscreen_wins, monitors, snap.width, snap.height), cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint_card(bgra: &mut [u8], sw: i32, r: ScreenRect, fill: [u8; 3], border: [u8; 3]) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let is_b = x == r.x || x == r.right() - 1 || y == r.y || y == r.bottom() - 1;
                let c = if is_b { border } else { fill };
                let off = ((y * sw + x) * 4) as usize;
                bgra[off..off + 4].copy_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
    }

    #[test]
    fn detects_card_inside_maximized_window_and_cycles_layers() {
        let (sw, sh) = (800, 600);
        let mut bgra = vec![0x20u8; (sw * sh * 4) as usize];
        for px in bgra.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let card = ScreenRect { x: 180, y: 120, w: 260, h: 180 };
        paint_card(&mut bgra, sw, card, [0x38, 0x3E, 0x4C], [0x68, 0x72, 0x86]);
        let snap = DesktopSnapshot { origin_x: 0, origin_y: 0, width: sw, height: sh, bgra };
        let max_win = ScreenRect { x: 0, y: 20, w: 800, h: 580 };
        let mut ctx = SnapContext::new(vec![max_win], vec![ScreenRect { x: 0, y: 0, w: 800, h: 600 }], sw, sh);

        let first = ctx.hover_at(&snap, 300, 200).expect("panel candidate");
        assert_eq!(first.kind, SnapKind::Panel);
        assert!((first.rect.x - card.x).abs() <= 2 && (first.rect.y - card.y).abs() <= 2);
        assert!((first.rect.w - card.w).abs() <= 4 && (first.rect.h - card.h).abs() <= 4);

        let mut noisy = snap.clone();
        paint_card(&mut noisy.bgra, sw, ScreenRect { x: 250, y: 170, w: 72, h: 36 }, [0xF0, 0xF2, 0xF8], [0xFF, 0xFF, 0xFF]);
        assert_eq!(ctx.hover_at(&noisy, 286, 188).expect("panel stays locked").rect, first.rect);
        assert_eq!(ctx.hover_at(&noisy, card.x - 4, 200).expect("border hysteresis holds").rect, first.rect);

        let second = ctx.cycle_next(&snap, 286, 188).expect("pane/window candidate");
        assert_eq!(second.kind, SnapKind::Window);
        assert_eq!(second.rect, max_win);
        assert_eq!(ctx.hover_at(&noisy, 300, 195).expect("tab cycle holds").kind, SnapKind::Window);
    }

    #[test]
    fn detects_dark_ide_sidebar_and_ignores_text_rows() {
        let (sw, sh) = (900, 640);
        let mut bgra = vec![0u8; (sw * sh * 4) as usize];
        for y in 0..sh {
            for x in 0..sw {
                let off = ((y * sw + x) * 4) as usize;
                let c = if x < 220 || y < 56 { [0x15u8, 0x14, 0x14] } else { [0x08u8, 0x07, 0x07] };
                bgra[off..off + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        // Paint noisy text lines inside sidebar (x=24..180, every 24px vertically)
        for row in (90..560).step_by(24) {
            for x in (24..176).step_by(4) {
                let off = ((row * sw + x) * 4) as usize;
                bgra[off..off + 4].copy_from_slice(&[0xD0, 0xD0, 0xD4, 255]);
            }
        }
        let snap = DesktopSnapshot { origin_x: 0, origin_y: 0, width: sw, height: sh, bgra };
        let max_win = ScreenRect { x: 0, y: 0, w: sw, h: sh };
        let mut ctx = SnapContext::new(vec![max_win], vec![max_win], sw, sh);

        let side = ctx.hover_at(&snap, 110, 240).expect("sidebar pane");
        assert!(matches!(side.kind, SnapKind::Pane | SnapKind::Panel));
        assert!(side.rect.x.abs() <= 2 && (side.rect.right() - 220).abs() <= 2);
        assert!(side.rect.y.abs() <= 2 && (side.rect.bottom() - sh).abs() <= 2);

        let main = ctx.hover_at(&snap, 500, 300).expect("main pane");
        assert!(matches!(main.kind, SnapKind::Pane | SnapKind::Panel));
        assert!((main.rect.x - 220).abs() <= 2 && (main.rect.y - 56).abs() <= 2);
        assert!((main.rect.right() - sw).abs() <= 2 && (main.rect.bottom() - sh).abs() <= 2);

        let mut fs_ctx = SnapContext::with_fullscreen(vec![max_win], vec![max_win], vec![max_win], sw, sh);
        let fs_hit = fs_ctx.hover_at(&snap, 110, 240).expect("fullscreen window");
        assert_eq!(fs_hit.rect, max_win);
        assert_eq!(fs_ctx.cycle_count, 1);
    }
}
