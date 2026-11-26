//! Velocity gating, dwell settling, fallback hysteresis, and smooth magnetic glide for smart snap.

use crate::screen::{DesktopSnapshot, ScreenRect};
use crate::snap_detect::{SnapCandidate, SnapContext, SnapKind};

const FAST_SPEED_PX_PER_SEC: f32 = 820.0;
const FAST_STEP_PX: f32 = 42.0;
const DWELL_SETTLE_MS: u64 = 58;
const FALLBACK_HOLD_MS: u64 = 105;
const LERP_TAU_MS: f32 = 24.0;
const SETTLE_EPS_PX: f32 = 0.6;
const FRAME_INTERVAL_MS: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapTickRequest {
    pub needs_redraw: bool,
    pub next_timer_ms: Option<u32>,
}

pub struct SnapMotion {
    target: Option<SnapCandidate>,
    visual: Option<[f32; 4]>,
    last_pos: Option<(i32, i32)>,
    last_move_ms: u64,
    smooth_speed: f32,
    pending_pos: Option<(i32, i32)>,
    pending_due_ms: u64,
    last_anim_ms: u64,
    pub bypass_active: bool,
    pub fast_moving: bool,
}

impl SnapMotion {
    pub fn new(initial: Option<SnapCandidate>, now_ms: u64) -> Self {
        Self {
            target: initial,
            visual: initial.map(|c| rect_to_f32(c.rect)),
            last_pos: None,
            last_move_ms: now_ms,
            smooth_speed: 0.0,
            pending_pos: None,
            pending_due_ms: 0,
            last_anim_ms: now_ms,
            bypass_active: false,
            fast_moving: false,
        }
    }

    pub fn target(&self) -> Option<SnapCandidate> {
        (!self.bypass_active).then_some(self.target).flatten()
    }

    pub fn visual_rect(&self) -> Option<ScreenRect> {
        if self.bypass_active {
            return None;
        }
        let v = self.visual?;
        Some(ScreenRect::from_points(
            v[0].round() as i32,
            v[1].round() as i32,
            v[2].round() as i32,
            v[3].round() as i32,
        ))
    }

    pub fn clear_pending(&mut self) {
        self.pending_pos = None;
        self.pending_due_ms = 0;
        self.fast_moving = false;
        self.smooth_speed = 0.0;
    }

    pub fn reset_at(&mut self, ctx: &mut SnapContext, snap: &DesktopSnapshot, x: i32, y: i32, now_ms: u64) {
        self.clear_pending();
        self.bypass_active = false;
        self.last_pos = Some((x, y));
        self.last_move_ms = now_ms;
        self.last_anim_ms = now_ms;
        self.target = ctx.hover_at(snap, x, y);
        self.visual = self.target.map(|c| rect_to_f32(c.rect));
    }

    pub fn on_pointer_move(
        &mut self,
        ctx: &mut SnapContext,
        snap: &DesktopSnapshot,
        x: i32,
        y: i32,
        alt_held: bool,
        now_ms: u64,
    ) -> SnapTickRequest {
        let prev_pos = self.last_pos.replace((x, y));
        let dt_ms = now_ms.saturating_sub(self.last_move_ms).clamp(1, 250);
        self.last_move_ms = now_ms;

        if alt_held {
            let prev_bypass = self.bypass_active;
            self.bypass_active = true;
            self.clear_pending();
            return SnapTickRequest { needs_redraw: !prev_bypass, next_timer_ms: None };
        }

        let resuming = self.bypass_active;
        if resuming {
            self.bypass_active = false;
            self.clear_pending();
            let next = ctx.hover_at(snap, x, y);
            self.apply_target(next, true);
            return SnapTickRequest { needs_redraw: true, next_timer_ms: self.next_wakeup_ms(now_ms) };
        }

        let dist = prev_pos
            .map(|(lx, ly)| ((x - lx) as f32).hypot((y - ly) as f32))
            .unwrap_or(0.0);
        let inst_speed = (dist * 1000.0) / (dt_ms as f32);
        self.smooth_speed = if dt_ms >= 90 {
            inst_speed
        } else {
            self.smooth_speed * 0.55 + inst_speed * 0.45
        };

        let is_fast = dist >= FAST_STEP_PX || self.smooth_speed >= FAST_SPEED_PX_PER_SEC;
        self.fast_moving = is_fast;
        if is_fast {
            self.pending_pos = Some((x, y));
            self.pending_due_ms = now_ms.saturating_add(DWELL_SETTLE_MS);
            let moved = self.step_anim(now_ms);
            return SnapTickRequest { needs_redraw: moved, next_timer_ms: self.next_wakeup_ms(now_ms) };
        }

        let next = ctx.hover_at(snap, x, y);
        let changed = self.apply_with_fallback_hold(next, (x, y), now_ms);
        let moved = self.step_anim(now_ms);
        SnapTickRequest {
            needs_redraw: changed || moved,
            next_timer_ms: self.next_wakeup_ms(now_ms),
        }
    }

    pub fn on_timer(&mut self, ctx: &mut SnapContext, snap: &DesktopSnapshot, now_ms: u64) -> SnapTickRequest {
        let mut target_changed = false;
        if !self.bypass_active
            && let Some((px, py)) = self.pending_pos
            && now_ms >= self.pending_due_ms
        {
            self.clear_pending();
            self.last_pos = Some((px, py));
            self.last_move_ms = now_ms;
            let next = ctx.hover_at(snap, px, py);
            target_changed = self.apply_target(next, false);
        }
        let anim_moved = self.step_anim(now_ms);
        SnapTickRequest {
            needs_redraw: target_changed || anim_moved,
            next_timer_ms: self.next_wakeup_ms(now_ms),
        }
    }

    pub fn on_cycle_next(
        &mut self,
        ctx: &mut SnapContext,
        snap: &DesktopSnapshot,
        x: i32,
        y: i32,
        now_ms: u64,
    ) -> SnapTickRequest {
        self.clear_pending();
        self.bypass_active = false;
        self.last_pos = Some((x, y));
        self.last_move_ms = now_ms;
        let next = ctx.cycle_next(snap, x, y);
        let changed = self.apply_target(next, false);
        let moved = self.step_anim(now_ms);
        SnapTickRequest {
            needs_redraw: changed || moved,
            next_timer_ms: self.next_wakeup_ms(now_ms),
        }
    }

    pub fn on_alt_changed(
        &mut self,
        ctx: &mut SnapContext,
        snap: &DesktopSnapshot,
        x: i32,
        y: i32,
        alt_held: bool,
        now_ms: u64,
    ) -> SnapTickRequest {
        if alt_held == self.bypass_active {
            return SnapTickRequest { needs_redraw: false, next_timer_ms: self.next_wakeup_ms(now_ms) };
        }
        if alt_held {
            self.bypass_active = true;
            self.clear_pending();
            return SnapTickRequest { needs_redraw: true, next_timer_ms: None };
        }
        self.bypass_active = false;
        self.clear_pending();
        self.last_pos = Some((x, y));
        self.last_move_ms = now_ms;
        let next = ctx.hover_at(snap, x, y);
        self.apply_target(next, true);
        SnapTickRequest { needs_redraw: true, next_timer_ms: self.next_wakeup_ms(now_ms) }
    }

    fn apply_with_fallback_hold(&mut self, next: Option<SnapCandidate>, pos: (i32, i32), now_ms: u64) -> bool {
        if let (Some(prev), Some(n)) = (self.target, next)
            && matches!(prev.kind, SnapKind::Panel | SnapKind::Pane)
            && !matches!(n.kind, SnapKind::Panel | SnapKind::Pane)
        {
            let prev_area = (prev.rect.w.max(1) as i64) * (prev.rect.h.max(1) as i64);
            let next_area = (n.rect.w.max(1) as i64) * (n.rect.h.max(1) as i64);
            if next_area * 100 >= prev_area * 170 {
                if self.pending_pos.is_none() || self.pending_due_ms <= now_ms {
                    self.pending_due_ms = now_ms.saturating_add(FALLBACK_HOLD_MS);
                }
                self.pending_pos = Some(pos);
                return false;
            }
        }
        self.pending_pos = None;
        self.pending_due_ms = 0;
        self.apply_target(next, false)
    }

    fn apply_target(&mut self, next: Option<SnapCandidate>, snap_visual_now: bool) -> bool {
        let changed = self.target != next;
        self.target = next;
        match next {
            None => {
                self.visual = None;
            }
            Some(c) if snap_visual_now || self.visual.is_none() => {
                self.visual = Some(rect_to_f32(c.rect));
            }
            Some(_) => {}
        }
        changed
    }

    fn step_anim(&mut self, now_ms: u64) -> bool {
        let dt_ms = now_ms.saturating_sub(self.last_anim_ms);
        self.last_anim_ms = now_ms;
        if self.bypass_active {
            return false;
        }
        let Some(tgt) = self.target.map(|c| rect_to_f32(c.rect)) else {
            return self.visual.take().is_some();
        };
        let Some(mut cur) = self.visual else {
            self.visual = Some(tgt);
            return true;
        };
        let mut max_delta = 0.0f32;
        for i in 0..4 {
            max_delta = max_delta.max((tgt[i] - cur[i]).abs());
        }
        if max_delta <= SETTLE_EPS_PX {
            let snapped = cur != tgt;
            self.visual = Some(tgt);
            return snapped;
        }
        if dt_ms == 0 {
            return false;
        }
        let alpha = 1.0 - (-(dt_ms.min(120) as f32) / LERP_TAU_MS).exp();
        let before = cur;
        for i in 0..4 {
            cur[i] += (tgt[i] - cur[i]) * alpha;
            if (tgt[i] - cur[i]).abs() <= SETTLE_EPS_PX {
                cur[i] = tgt[i];
            }
        }
        self.visual = Some(cur);
        cur != before
    }

    fn anim_active(&self) -> bool {
        if self.bypass_active {
            return false;
        }
        match (self.visual, self.target) {
            (Some(v), Some(t)) => v != rect_to_f32(t.rect),
            _ => false,
        }
    }

    fn next_wakeup_ms(&self, now_ms: u64) -> Option<u32> {
        let dwell_wait = if !self.bypass_active && self.pending_pos.is_some() {
            Some(self.pending_due_ms.saturating_sub(now_ms).clamp(8, 250) as u32)
        } else {
            None
        };
        let anim_wait = self.anim_active().then_some(FRAME_INTERVAL_MS);
        match (dwell_wait, anim_wait) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

#[inline]
fn rect_to_f32(r: ScreenRect) -> [f32; 4] {
    [r.x as f32, r.y as f32, r.right() as f32, r.bottom() as f32]
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

    fn fixture_scene() -> (DesktopSnapshot, SnapContext, ScreenRect, ScreenRect, ScreenRect) {
        let (sw, sh) = (960, 640);
        let mut bgra = vec![0x20u8; (sw * sh * 4) as usize];
        for px in bgra.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let card_a = ScreenRect { x: 60, y: 80, w: 220, h: 180 };
        let card_b = ScreenRect { x: 300, y: 120, w: 280, h: 200 };
        paint_card(&mut bgra, sw, card_a, [0x3E, 0x46, 0x56], [0x3E, 0x46, 0x56]);
        paint_card(&mut bgra, sw, card_b, [0x3C, 0x4A, 0x5A], [0x3C, 0x4A, 0x5A]);
        let win = ScreenRect { x: 0, y: 0, w: sw, h: sh };
        let ctx = SnapContext::new(vec![win], vec![win], sw, sh);
        (DesktopSnapshot { origin_x: 0, origin_y: 0, width: sw, height: sh, bgra }, ctx, card_a, card_b, win)
    }

    #[test]
    fn fast_swipe_freezes_target_until_dwell_settles() {
        let (snap, mut ctx, card_a, card_b, _) = fixture_scene();
        let init = ctx.hover_at(&snap, 160, 160);
        let mut mot = SnapMotion::new(init, 0);
        assert_eq!(mot.target().unwrap().rect, card_a);
        let _ = mot.on_pointer_move(&mut ctx, &snap, 160, 160, false, 0);

        // Large jump (> 42px) in 10ms triggers fast swipe freeze + dwell timer.
        let tick = mot.on_pointer_move(&mut ctx, &snap, 440, 220, false, 10);
        assert!(mot.fast_moving);
        assert_eq!(mot.target().unwrap().rect, card_a);
        assert!(tick.next_timer_ms.is_some());

        // Before dwell deadline (68ms), still frozen on previous target.
        let _ = mot.on_timer(&mut ctx, &snap, 50);
        assert_eq!(mot.target().unwrap().rect, card_a);

        // Once dwell expires (>= 68ms), locks cleanly onto card_b.
        let done = mot.on_timer(&mut ctx, &snap, 72);
        assert!(done.needs_redraw);
        assert!(!mot.fast_moving);
        assert_eq!(mot.target().unwrap().rect, card_b);
    }

    #[test]
    fn panel_to_window_fallback_holds_briefly_and_skips_flash_on_reentry() {
        let (snap, mut ctx, card_a, card_b, win) = fixture_scene();
        let init = ctx.hover_at(&snap, 170, 180);
        let mut mot = SnapMotion::new(init, 0);
        assert_eq!(mot.target().unwrap().rect, card_a);
        let _ = mot.on_pointer_move(&mut ctx, &snap, 205, 180, false, 90);
        let _ = mot.on_pointer_move(&mut ctx, &snap, 240, 180, false, 180);
        let _ = mot.on_pointer_move(&mut ctx, &snap, 275, 180, false, 270);
        assert_eq!(mot.target().unwrap().rect, card_a);

        // Slow drift out of card_a into 20px gap -> holds card_a instead of flashing full `win`.
        let gap_tick = mot.on_pointer_move(&mut ctx, &snap, 290, 180, false, 330);
        assert_eq!(mot.target().unwrap().rect, card_a);
        assert!(gap_tick.next_timer_ms.is_some());

        // Move slowly onto card_b before fallback hold expires -> switches straight to card_b without `win` flash.
        let _ = mot.on_pointer_move(&mut ctx, &snap, 328, 180, false, 385);
        assert_ne!(mot.target().unwrap().rect, win);
        let _ = mot.on_pointer_move(&mut ctx, &snap, 366, 180, false, 440);
        assert_eq!(mot.target().unwrap().rect, card_b);

        // Drift back slowly to gap and let hold timer expire -> falls back cleanly to `win`.
        let _ = mot.on_pointer_move(&mut ctx, &snap, 328, 180, false, 510);
        let _ = mot.on_pointer_move(&mut ctx, &snap, 290, 180, false, 580);
        assert_eq!(mot.target().unwrap().rect, card_b);
        let _ = mot.on_timer(&mut ctx, &snap, 710);
        assert_eq!(mot.target().unwrap().rect, win);
    }

    #[test]
    fn visual_rect_glides_smoothly_and_stops_timer_when_settled() {
        let (snap, mut ctx, card_a, card_b, _) = fixture_scene();
        let mut mot = SnapMotion::new(ctx.hover_at(&snap, 160, 160), 0);
        let _ = mot.on_pointer_move(&mut ctx, &snap, 440, 220, false, 150);
        assert_eq!(mot.target().unwrap().rect, card_b);
        let mid = mot.visual_rect().unwrap();
        assert!(mid.x > card_a.x && mid.x < card_b.x);

        let mut t = 166;
        let mut last = SnapTickRequest::default();
        for _ in 0..24 {
            last = mot.on_timer(&mut ctx, &snap, t);
            if last.next_timer_ms.is_none() {
                break;
            }
            t += 16;
        }
        assert!(last.next_timer_ms.is_none());
        assert_eq!(mot.visual_rect().unwrap(), card_b);
    }

    #[test]
    fn alt_bypass_hides_snap_and_restores_on_release() {
        let (snap, mut ctx, card_a, _, _) = fixture_scene();
        let mut mot = SnapMotion::new(ctx.hover_at(&snap, 160, 160), 0);
        let t1 = mot.on_alt_changed(&mut ctx, &snap, 160, 160, true, 50);
        assert!(t1.needs_redraw);
        assert!(t1.next_timer_ms.is_none());
        assert!(mot.target().is_none());
        assert!(mot.visual_rect().is_none());

        let t2 = mot.on_alt_changed(&mut ctx, &snap, 160, 160, false, 90);
        assert!(t2.needs_redraw);
        assert_eq!(mot.target().unwrap().rect, card_a);
        assert_eq!(mot.visual_rect().unwrap(), card_a);
    }
}
