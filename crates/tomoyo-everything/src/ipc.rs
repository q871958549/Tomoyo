//! Native Win32 `WM_COPYDATA` transport for Voidtools Everything 1.4 & 1.5a.

use crate::{EverythingError, EverythingQueryOptions, EverythingResponse, EverythingVersion};

#[cfg(windows)]
mod win {
    use super::*;
    use crate::wire::{pack_query2w_payload, parse_list2_buffer, EVERYTHING_IPC_COPYDATA_QUERY2W};
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
        GetWindowLongPtrW, MsgWaitForMultipleObjects, PeekMessageW, RegisterClassW,
        SendMessageTimeoutW, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, MSG, PM_REMOVE,
        QS_ALLINPUT, SMTO_NORMAL, WM_COPYDATA, WNDCLASSW,
    };

    const RECV_CLASS_NAME: PCWSTR = w!("TomoyoEverythingIpcRecv");
    static CLASS_REGISTERED: OnceLock<bool> = OnceLock::new();
    static REPLY_SEQ: AtomicU32 = AtomicU32::new(1);

    struct ReplySlot {
        expected_id: u32,
        result: Option<Result<EverythingResponse, EverythingError>>,
    }

    struct ThreadReceiver {
        hwnd: HWND,
    }

    impl ThreadReceiver {
        fn create() -> Result<Self, EverythingError> {
            unsafe {
                let hinstance = GetModuleHandleW(None)
                    .map_err(|e| EverythingError::Ipc(format!("GetModuleHandleW failed: {e}")))?;

                CLASS_REGISTERED.get_or_init(|| {
                    let wc = WNDCLASSW {
                        lpfnWndProc: Some(recv_wndproc),
                        hInstance: hinstance.into(),
                        lpszClassName: RECV_CLASS_NAME,
                        ..Default::default()
                    };
                    RegisterClassW(&wc) != 0
                });

                // Voidtools Everything 1.4 requires a hidden top-level receiver window (not HWND_MESSAGE).
                let hwnd = CreateWindowExW(
                    Default::default(),
                    RECV_CLASS_NAME,
                    w!(""),
                    Default::default(),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(hinstance.into()),
                    None,
                )
                .map_err(|e| EverythingError::Ipc(format!("CreateWindowExW failed: {e}")))?;

                Ok(Self { hwnd })
            }
        }
    }

    impl Drop for ThreadReceiver {
        fn drop(&mut self) {
            unsafe {
                if !self.hwnd.is_invalid() {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
        }
    }

    thread_local! {
        static THREAD_RECV: RefCell<Option<ThreadReceiver>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn recv_wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_COPYDATA {
            let slot_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ReplySlot;
            if !slot_ptr.is_null() {
                let slot = unsafe { &mut *slot_ptr };
                let cds = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
                if cds.dwData == slot.expected_id as usize && !cds.lpData.is_null() {
                    let slice = unsafe {
                        std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize)
                    };
                    slot.result = Some(parse_list2_buffer(slice));
                    return LRESULT(1);
                }
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn find_everything_window() -> Option<(HWND, EverythingVersion)> {
        unsafe {
            if let Ok(hwnd) =
                FindWindowW(w!("EVERYTHING_TASKBAR_NOTIFICATION_(1.5a)"), PCWSTR::null())
                && !hwnd.is_invalid() {
                    return Some((hwnd, EverythingVersion::V15Alpha));
                }
            if let Ok(hwnd) = FindWindowW(w!("EVERYTHING_TASKBAR_NOTIFICATION"), PCWSTR::null())
                && !hwnd.is_invalid() {
                    return Some((hwnd, EverythingVersion::V14));
                }
            None
        }
    }

    pub fn detect_running() -> Option<EverythingVersion> {
        find_everything_window().map(|(_, v)| v)
    }

    pub fn query(
        search: &str,
        opts: &EverythingQueryOptions,
    ) -> Result<EverythingResponse, EverythingError> {
        let (target_hwnd, _ver) = find_everything_window().ok_or(EverythingError::NotRunning)?;

        THREAD_RECV.with(|cell| {
            let mut guard = cell.borrow_mut();
            if guard.is_none() {
                *guard = Some(ThreadReceiver::create()?);
            }
            let recv_hwnd = guard.as_ref().unwrap().hwnd;
            let reply_id = REPLY_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
            let mut payload = pack_query2w_payload(recv_hwnd.0 as u32, reply_id, search, opts);

            let mut slot = ReplySlot {
                expected_id: reply_id,
                result: None,
            };
            let cds = COPYDATASTRUCT {
                dwData: EVERYTHING_IPC_COPYDATA_QUERY2W,
                cbData: payload.len() as u32,
                lpData: payload.as_mut_ptr() as *mut c_void,
            };

            unsafe {
                SetWindowLongPtrW(recv_hwnd, GWLP_USERDATA, &mut slot as *mut ReplySlot as isize);
            }

            let send_res = unsafe {
                let mut _out = 0usize;
                SendMessageTimeoutW(
                    target_hwnd,
                    WM_COPYDATA,
                    WPARAM(recv_hwnd.0 as usize),
                    LPARAM(&cds as *const COPYDATASTRUCT as isize),
                    SMTO_NORMAL,
                    opts.timeout_ms,
                    Some(&mut _out),
                )
            };

            let deadline = Instant::now() + Duration::from_millis(opts.timeout_ms as u64);
            if send_res.0 != 0 {
                while slot.result.is_none() && Instant::now() < deadline {
                    let mut msg = MSG::default();
                    let mut drained = false;
                    unsafe {
                        while PeekMessageW(&mut msg, Some(recv_hwnd), 0, 0, PM_REMOVE).as_bool() {
                            drained = true;
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                            if slot.result.is_some() {
                                break;
                            }
                        }
                    }
                    if slot.result.is_some() {
                        break;
                    }
                    if !drained {
                        let remain_ms = deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis()
                            .min(50) as u32;
                        if remain_ms == 0 {
                            break;
                        }
                        unsafe {
                            let _ = MsgWaitForMultipleObjects(None, false, remain_ms, QS_ALLINPUT);
                        }
                    }
                }
            }

            unsafe {
                SetWindowLongPtrW(recv_hwnd, GWLP_USERDATA, 0);
            }

            if send_res.0 == 0 && slot.result.is_none() {
                return Err(EverythingError::Timeout(opts.timeout_ms));
            }
            slot.result
                .unwrap_or(Err(EverythingError::Timeout(opts.timeout_ms)))
        })
    }
}

#[cfg(not(windows))]
mod win {
    use super::*;

    pub fn detect_running() -> Option<EverythingVersion> {
        None
    }

    pub fn query(
        _search: &str,
        _opts: &EverythingQueryOptions,
    ) -> Result<EverythingResponse, EverythingError> {
        Err(EverythingError::UnsupportedPlatform)
    }
}

pub use win::{detect_running, query};
