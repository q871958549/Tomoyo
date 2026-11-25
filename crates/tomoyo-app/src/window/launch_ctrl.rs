//! Non-blocking COM-STA `ShellExecuteExW` app/file/URL launcher with persistent cross-hide
//! launch tracking, real OS window-readiness detection, foreground handoff, and zero leaks.

use super::launch_probe::{
    shell_execute_ex, snapshot_visible_top_windows, wait_until_app_ready, ComStaGuard,
};
use super::{AppShell, TIMER_LAUNCH_HIDE, TIMER_LAUNCH_TICK, WM_APP_LAUNCH_DONE};
use std::collections::HashSet;
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tomoyo_core::Lang;
use tomoyo_protocol::{ActionKind, ResultItem, SearchFilter};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetForegroundWindow, IsWindowVisible, KillTimer, PostMessageW,
    SetTimer, ASFW_ANY,
};

const MAX_ACTIVE_LAUNCHES: usize = 8;
const READY_KEEP_MS: u128 = 3_500;
const STALE_LAUNCH_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchPhase {
    Spawning,
    Starting,
    Ready(Instant),
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveLaunch {
    pub launch_id: u64,
    pub item_id: String,
    pub title: String,
    pub item_snapshot: ResultItem,
    pub is_admin: bool,
    pub started_at: Instant,
    pub phase: LaunchPhase,
}

impl ActiveLaunch {
    pub fn is_ready(&self) -> bool {
        matches!(self.phase, LaunchPhase::Ready(_))
    }

    pub fn is_in_flight(&self) -> bool {
        !self.is_ready()
    }
}

pub(crate) enum LaunchEvent {
    Spawned {
        launch_id: u64,
        is_admin: bool,
        track_ready: bool,
    },
    Ready {
        launch_id: u64,
        title: String,
    },
    Failed {
        launch_id: u64,
        item_id: String,
        title: String,
        is_admin: bool,
        error: String,
    },
}

enum LaunchSpec {
    Shell {
        verb_admin: bool,
        target: String,
        args: Option<String>,
        workdir: Option<String>,
    },
    Terminal {
        dir: String,
    },
}

impl AppShell {
    /// Starts an async shell launch whose lifecycle persists across launcher hide/re-open cycles
    /// until the target application's window actually appears (or safety timeout elapses).
    pub(super) fn try_start_async_launch(&mut self, kind: &ActionKind, item: &ResultItem) -> bool {
        let (spec, is_admin, track_ready) = match kind {
            ActionKind::LaunchPath { path, args, working_dir } => (
                LaunchSpec::Shell { verb_admin: false, target: path.clone(), args: args.clone(), workdir: working_dir.clone() },
                false,
                true,
            ),
            ActionKind::LaunchAsAdmin { path, args, working_dir } => (
                LaunchSpec::Shell { verb_admin: true, target: path.clone(), args: args.clone(), workdir: working_dir.clone() },
                true,
                true,
            ),
            ActionKind::OpenUrl { url } => (
                LaunchSpec::Shell { verb_admin: false, target: url.clone(), args: None, workdir: None },
                false,
                false,
            ),
            ActionKind::RevealInExplorer { path } => (
                LaunchSpec::Shell { verb_admin: false, target: "explorer.exe".into(), args: Some(format!("/select,\"{path}\"")), workdir: None },
                false,
                false,
            ),
            ActionKind::OpenTerminalAt { dir } => (LaunchSpec::Terminal { dir: dir.clone() }, false, false),
            _ => return false,
        };

        self.prune_finished_launches();
        // Persistent duplicate-launch guard: survives hide/re-open while the app is still starting!
        if let Some(active) = self.active_launches.iter().find(|l| l.item_id == item.id && l.is_in_flight()) {
            let secs = active.started_at.elapsed().as_secs();
            let msg = match self.lang {
                Lang::Zh => format!("⏳「{}」仍在启动中（{secs}s），请稍候…", active.title),
                Lang::En => format!("⏳ \"{}\" is still launching ({secs}s)…", active.title),
            };
            self.toast = Some(msg);
            self.arm_launch_tick_if_needed();
            self.invalidate();
            return true;
        }

        unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE); }
        self.action_menu_open = false;
        self.inter.drag_origin = None;
        if is_admin {
            super::set_async_modal_overlay(true);
        }
        self.next_launch_id = self.next_launch_id.wrapping_add(1);
        let launch_id = self.next_launch_id;
        self.active_launches.retain(|l| l.item_id != item.id);
        if self.active_launches.len() >= MAX_ACTIVE_LAUNCHES {
            self.active_launches.remove(0);
        }
        self.active_launches.push(ActiveLaunch {
            launch_id,
            item_id: item.id.clone(),
            title: item.title.clone(),
            item_snapshot: item.clone(),
            is_admin,
            started_at: Instant::now(),
            phase: LaunchPhase::Spawning,
        });

        self.toast = Some(super::launch_probe::format_in_flight_toast(
            self.lang, &item.title, is_admin, 0, self.in_flight_count(),
        ));
        self.arm_launch_tick_if_needed();
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = UpdateWindow(self.hwnd);
        }

        let queue = Arc::clone(&self.pending_launch_events);
        let hwnd_raw = self.hwnd.0 as usize;
        let id_owned = item.id.clone();
        let title_owned = item.title.clone();
        let lang = self.lang;
        let _ = std::thread::Builder::new()
            .name("tomoyo-launch".into())
            .spawn(move || {
                let _com = ComStaGuard::init();
                let fg_before = unsafe { GetForegroundWindow().0 as isize };
                let wins_before = if track_ready { snapshot_visible_top_windows() } else { HashSet::new() };
                unsafe { let _ = AllowSetForegroundWindow(ASFW_ANY); }
                let result = match spec {
                    LaunchSpec::Shell { verb_admin, target, args, workdir } => {
                        shell_execute_ex(verb_admin, &target, args.as_deref(), workdir.as_deref(), lang)
                    }
                    LaunchSpec::Terminal { dir } => {
                        let target_dir = if Path::new(&dir).is_dir() { dir.as_str() } else { "." };
                        let wt_arg = format!("-d \"{target_dir}\"");
                        shell_execute_ex(false, "wt.exe", Some(&wt_arg), Some(target_dir), lang)
                            .or_else(|_| shell_execute_ex(false, "powershell.exe", None, Some(target_dir), lang))
                    }
                };
                match result {
                    Ok(proc_guard) => {
                        push_launch_event(&queue, hwnd_raw, LaunchEvent::Spawned { launch_id, is_admin, track_ready });
                        if track_ready {
                            wait_until_app_ready(proc_guard, fg_before, hwnd_raw as isize, &wins_before);
                            push_launch_event(&queue, hwnd_raw, LaunchEvent::Ready { launch_id, title: title_owned });
                        }
                    }
                    Err(error) => {
                        push_launch_event(&queue, hwnd_raw, LaunchEvent::Failed {
                            launch_id, item_id: id_owned, title: title_owned, is_admin, error,
                        });
                    }
                }
            });
        true
    }

    pub(super) fn on_async_launch_done(&mut self) {
        let events: Vec<LaunchEvent> = self
            .pending_launch_events
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        if events.is_empty() {
            return;
        }
        let visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
        for ev in events {
            match ev {
                LaunchEvent::Spawned { launch_id, is_admin, track_ready } => {
                    if is_admin {
                        super::set_async_modal_overlay(false);
                    }
                    if let Some(a) = self.active_launches.iter_mut().find(|l| l.launch_id == launch_id) {
                        let elapsed = a.started_at.elapsed().as_millis() as u32;
                        let title = a.title.clone();
                        if track_ready {
                            a.phase = LaunchPhase::Starting;
                        } else {
                            a.phase = LaunchPhase::Ready(Instant::now());
                        }
                        if visible {
                            if !track_ready {
                                self.show_toast(&match self.lang {
                                    Lang::Zh => format!("✓ 已启动「{title}」"),
                                    Lang::En => format!("✓ Launched \"{title}\""),
                                });
                            }
                            let remain_ms = 240u32.saturating_sub(elapsed).clamp(140, 240);
                            unsafe {
                                let _ = InvalidateRect(Some(self.hwnd), None, false);
                                let _ = UpdateWindow(self.hwnd);
                                let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE);
                                let _ = SetTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE, remain_ms, None);
                            }
                        }
                    }
                }
                LaunchEvent::Ready { launch_id, title } => {
                    if let Some(a) = self.active_launches.iter_mut().find(|l| l.launch_id == launch_id) {
                        a.phase = LaunchPhase::Ready(Instant::now());
                    }
                    if visible {
                        if self.in_flight_count() > 0 {
                            self.sync_active_launch_toast();
                        } else {
                            self.show_toast(&match self.lang {
                                Lang::Zh => format!("✓ 已启动「{title}」"),
                                Lang::En => format!("✓ Launched \"{title}\""),
                            });
                        }
                        self.invalidate();
                    }
                }
                LaunchEvent::Failed { launch_id, item_id, title, is_admin, error } => {
                    if is_admin {
                        super::set_async_modal_overlay(false);
                    }
                    let tid = tomoyo_core::next_trace_id("ACT");
                    tracing::warn!(trace_id = %tid, item_id = %item_id, title = %title, "Async launch failed: {error}");
                    self.active_launches.retain(|l| l.launch_id != launch_id);
                    let fail_msg = match self.lang {
                        Lang::Zh => format!("✗ 启动失败「{title}」：{error}"),
                        Lang::En => format!("✗ Failed \"{title}\": {error}"),
                    };
                    if visible {
                        self.show_toast(&fail_msg);
                    } else {
                        self.pending_launch_failure = Some((fail_msg, Instant::now()));
                    }
                }
            }
        }
        self.prune_finished_launches();
        self.arm_launch_tick_if_needed();
    }

    pub(super) fn on_launch_tick(&mut self) {
        self.prune_finished_launches();
        let visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
        if !visible || self.active_launches.is_empty() {
            unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_TICK); }
            if visible && self.toast.as_deref().is_some_and(|t| t.starts_with('⏳')) {
                self.toast = None;
                self.invalidate();
            }
            return;
        }
        if self.in_flight_count() > 0 {
            self.sync_active_launch_toast();
        }
        self.invalidate();
    }

    pub(super) fn restore_launch_ui_on_show(&mut self) {
        self.on_async_launch_done();
        self.prune_finished_launches();
        if let Some((msg, at)) = self.pending_launch_failure.take()
            && at.elapsed().as_secs() < 12
        {
            self.show_toast(&msg);
        } else if self.in_flight_count() > 0 {
            self.sync_active_launch_toast();
            self.arm_launch_tick_if_needed();
        }
    }

    pub(super) fn inject_active_launches_into_empty_results(&mut self) {
        if !self.current_query.trim().is_empty()
            || self.active_filter != SearchFilter::All
            || self.attached.is_some()
        {
            return;
        }
        self.prune_finished_launches();
        for launch in self.active_launches.iter().rev() {
            if !launch.is_in_flight() {
                continue;
            }
            if let Some(pos) = self.results.iter().position(|r| r.id == launch.item_id) {
                if pos > 0 {
                    let it = self.results.remove(pos);
                    self.results.insert(0, it);
                }
            } else {
                self.results.insert(0, launch.item_snapshot.clone());
            }
        }
    }

    pub(super) fn resolve_launching_row(&self) -> Option<(usize, bool)> {
        for pass_in_flight in [true, false] {
            for l in self.active_launches.iter().rev() {
                if l.is_in_flight() != pass_in_flight {
                    continue;
                }
                if let Some(idx) = self.results.iter().position(|r| r.id == l.item_id) {
                    return Some((idx, l.is_ready()));
                }
            }
        }
        None
    }

    pub(super) fn has_in_flight_launch(&self) -> bool {
        self.in_flight_count() > 0
    }

    pub(super) fn cancel_pending_launch_hide(&mut self) {
        unsafe { let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_HIDE); }
    }

    fn in_flight_count(&self) -> usize {
        self.active_launches.iter().filter(|l| l.is_in_flight()).count()
    }

    fn sync_active_launch_toast(&mut self) {
        let count = self.in_flight_count();
        if let Some(l) = self.active_launches.iter().rev().find(|l| l.is_in_flight()) {
            let secs = l.started_at.elapsed().as_secs();
            unsafe { let _ = KillTimer(Some(self.hwnd), super::TIMER_CLEAR_TOAST); }
            self.toast = Some(super::launch_probe::format_in_flight_toast(
                self.lang, &l.title, l.is_admin, secs, count,
            ));
        }
    }

    fn arm_launch_tick_if_needed(&self) {
        let visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
        unsafe {
            if visible && !self.active_launches.is_empty() {
                let _ = SetTimer(Some(self.hwnd), TIMER_LAUNCH_TICK, 500, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_LAUNCH_TICK);
            }
        }
    }

    fn prune_finished_launches(&mut self) {
        prune_launch_list(&mut self.active_launches);
    }
}

fn prune_launch_list(list: &mut Vec<ActiveLaunch>) {
    list.retain(|l| match l.phase {
        LaunchPhase::Ready(at) => at.elapsed().as_millis() < READY_KEEP_MS,
        LaunchPhase::Spawning | LaunchPhase::Starting => l.started_at.elapsed().as_secs() < STALE_LAUNCH_SECS,
    });
}

fn push_launch_event(
    queue: &Arc<std::sync::Mutex<std::collections::VecDeque<LaunchEvent>>>,
    hwnd_raw: usize,
    ev: LaunchEvent,
) {
    if let Ok(mut q) = queue.lock() {
        q.push_back(ev);
    }
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd_raw as *mut c_void)), WM_APP_LAUNCH_DONE, WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tomoyo_protocol::{IconSpec, ResultCategory};

    #[test]
    fn prune_keeps_in_flight_and_drops_expired_ready_entries() {
        let snap = ResultItem {
            id: "app:idea".into(), provider_id: "apps".into(), title: "IntelliJ IDEA".into(),
            subtitle: String::new(), category: ResultCategory::App, score: 100,
            icon: IconSpec::Monogram("ID".into()), badge: None, copy_text: None, actions: vec![], preview: None,
        };
        let mut list = vec![
            ActiveLaunch {
                launch_id: 1, item_id: "app:idea".into(), title: "IntelliJ IDEA".into(),
                item_snapshot: snap.clone(), is_admin: false,
                started_at: Instant::now() - Duration::from_secs(5), phase: LaunchPhase::Starting,
            },
            ActiveLaunch {
                launch_id: 2, item_id: "app:calc".into(), title: "Calculator".into(),
                item_snapshot: snap, is_admin: false,
                started_at: Instant::now() - Duration::from_secs(6),
                phase: LaunchPhase::Ready(Instant::now() - Duration::from_secs(5)),
            },
        ];
        prune_launch_list(&mut list);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].item_id, "app:idea");
        assert!(list[0].is_in_flight());
    }
}
