use crate::{EverythingEntry, EverythingError, EverythingQueryOptions, EverythingResponse, EverythingVersion};

// Everything IPC constants (from official Voidtools everything_ipc.h).
const EVERYTHING_IPC_COPYDATA_QUERY2W: usize = 18;

const EVERYTHING_IPC_MATCHCASE: u32 = 0x0000_0001;
const EVERYTHING_IPC_MATCHWHOLEWORD: u32 = 0x0000_0002;
const EVERYTHING_IPC_MATCHPATH: u32 = 0x0000_0004;
const EVERYTHING_IPC_REGEX: u32 = 0x0000_0008;

const EVERYTHING_IPC_QUERY2_REQUEST_NAME: u32 = 0x0000_0001;
const EVERYTHING_IPC_QUERY2_REQUEST_PATH: u32 = 0x0000_0002;
const EVERYTHING_IPC_QUERY2_REQUEST_FULL_PATH_AND_NAME: u32 = 0x0000_0004;
const EVERYTHING_IPC_QUERY2_REQUEST_EXTENSION: u32 = 0x0000_0008;
const EVERYTHING_IPC_QUERY2_REQUEST_SIZE: u32 = 0x0000_0010;
const EVERYTHING_IPC_QUERY2_REQUEST_DATE_CREATED: u32 = 0x0000_0020;
const EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
const EVERYTHING_IPC_QUERY2_REQUEST_DATE_ACCESSED: u32 = 0x0000_0080;
const EVERYTHING_IPC_QUERY2_REQUEST_ATTRIBUTES: u32 = 0x0000_0100;

const EVERYTHING_IPC_FOLDER: u32 = 0x0000_0001;
const EVERYTHING_IPC_DRIVE: u32 = 0x0000_0002;

/// Parses a raw `EVERYTHING_IPC_LIST2` reply buffer with strict bounds checking.
pub(crate) fn parse_list2_buffer(buf: &[u8]) -> Result<EverythingResponse, EverythingError> {
    if buf.len() < 28 {
        return Err(EverythingError::Ipc(format!(
            "reply buffer too short: {} bytes",
            buf.len()
        )));
    }

    let read_u32 = |offset: usize| -> Result<u32, EverythingError> {
        let end = offset.checked_add(4).ok_or_else(|| EverythingError::Ipc("offset overflow".into()))?;
        let slice = buf
            .get(offset..end)
            .ok_or_else(|| EverythingError::Ipc("unexpected EOF reading u32".into()))?;
        Ok(u32::from_le_bytes(slice.try_into().unwrap()))
    };

    let read_u64 = |offset: usize| -> Result<u64, EverythingError> {
        let end = offset.checked_add(8).ok_or_else(|| EverythingError::Ipc("offset overflow".into()))?;
        let slice = buf
            .get(offset..end)
            .ok_or_else(|| EverythingError::Ipc("unexpected EOF reading u64".into()))?;
        Ok(u64::from_le_bytes(slice.try_into().unwrap()))
    };

    let read_len_prefixed_utf16 = |cursor: &mut usize| -> Result<String, EverythingError> {
        let len_chars = read_u32(*cursor)? as usize;
        *cursor += 4;
        // Everything includes a trailing NUL character after `len_chars` WCHARs.
        let byte_len = len_chars
            .checked_mul(2)
            .ok_or_else(|| EverythingError::Ipc("utf16 len overflow".into()))?;
        let bytes = buf
            .get(*cursor..*cursor + byte_len)
            .ok_or_else(|| EverythingError::Ipc("unexpected EOF reading utf16 string".into()))?;
        let mut units = Vec::with_capacity(len_chars);
        for chunk in bytes.chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        // Advance past string + trailing NUL (2 bytes).
        *cursor = cursor
            .checked_add(byte_len)
            .and_then(|v| v.checked_add(2))
            .ok_or_else(|| EverythingError::Ipc("cursor overflow".into()))?;
        Ok(String::from_utf16_lossy(&units))
    };

    let total_folders = read_u32(0)?;
    let total_files = read_u32(4)?;
    let total_items = read_u32(8)?;
    let num_items = read_u32(12)?;
    let _offset = read_u32(16)?;
    let request_flags = read_u32(20)?;
    let _sort_type = read_u32(24)?;

    // Cap allocation against malformed buffers.
    let safe_count = (num_items as usize).min(1000);
    let mut entries = Vec::with_capacity(safe_count);

    for i in 0..safe_count {
        let item_header_offset = 28 + i * 8;
        if item_header_offset + 8 > buf.len() {
            break;
        }
        let flags = read_u32(item_header_offset)?;
        let data_offset = read_u32(item_header_offset + 4)? as usize;
        let mut cursor = data_offset;

        let mut name = String::new();
        let mut path = String::new();
        let mut full_path = String::new();
        let mut size = None;
        let mut date_modified_filetime = None;
        let mut attributes = None;

        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_NAME) != 0 {
            name = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_PATH) != 0 {
            path = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_FULL_PATH_AND_NAME) != 0 {
            full_path = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_EXTENSION) != 0 {
            let _ = read_len_prefixed_utf16(&mut cursor)?;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_SIZE) != 0 {
            let v = read_u64(cursor)?;
            cursor += 8;
            if v != u64::MAX {
                size = Some(v);
            }
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_DATE_CREATED) != 0 {
            cursor += 8;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED) != 0 {
            let v = read_u64(cursor)?;
            cursor += 8;
            if v != u64::MAX && v != 0 {
                date_modified_filetime = Some(v);
            }
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_DATE_ACCESSED) != 0 {
            cursor += 8;
        }
        if (request_flags & EVERYTHING_IPC_QUERY2_REQUEST_ATTRIBUTES) != 0 {
            attributes = Some(read_u32(cursor)?);
        }

        if full_path.is_empty() {
            if path.is_empty() {
                full_path = name.clone();
            } else if path.ends_with('\\') {
                full_path = format!("{path}{name}");
            } else {
                full_path = format!("{path}\\{name}");
            }
        }
        if name.is_empty() {
            name = full_path
                .rsplit('\\')
                .next()
                .unwrap_or(&full_path)
                .to_string();
        }
        if path.is_empty() {
            if let Some((parent, _)) = full_path.rsplit_once('\\') {
                path = parent.to_string();
            }
        }

        let is_drive = (flags & EVERYTHING_IPC_DRIVE) != 0;
        let is_folder = (flags & EVERYTHING_IPC_FOLDER) != 0 || is_drive;

        entries.push(EverythingEntry {
            name,
            path,
            full_path,
            is_folder,
            is_drive,
            size: if is_folder { None } else { size },
            date_modified_filetime,
            attributes,
        });
    }

    Ok(EverythingResponse {
        total_folders,
        total_files,
        total_items,
        entries,
    })
}

#[cfg(windows)]
mod win {
    use super::*;
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
        SendMessageTimeoutW, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, HWND_MESSAGE,
        MSG, PM_REMOVE, QS_ALLINPUT, SMTO_ABORTIFHUNG, WM_COPYDATA, WNDCLASSW,
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

                let hwnd = CreateWindowExW(
                    Default::default(),
                    RECV_CLASS_NAME,
                    w!(""),
                    Default::default(),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
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
            if let Ok(hwnd) = FindWindowW(w!("EVERYTHING_TASKBAR_NOTIFICATION_(1.5a)"), PCWSTR::null())
            {
                if !hwnd.is_invalid() {
                    return Some((hwnd, EverythingVersion::V15Alpha));
                }
            }
            if let Ok(hwnd) = FindWindowW(w!("EVERYTHING_TASKBAR_NOTIFICATION"), PCWSTR::null()) {
                if !hwnd.is_invalid() {
                    return Some((hwnd, EverythingVersion::V14));
                }
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
            let mut search_flags = 0u32;
            if opts.match_case {
                search_flags |= EVERYTHING_IPC_MATCHCASE;
            }
            if opts.match_whole_word {
                search_flags |= EVERYTHING_IPC_MATCHWHOLEWORD;
            }
            if opts.match_path {
                search_flags |= EVERYTHING_IPC_MATCHPATH;
            }
            if opts.regex {
                search_flags |= EVERYTHING_IPC_REGEX;
            }

            let request_flags = EVERYTHING_IPC_QUERY2_REQUEST_NAME
                | EVERYTHING_IPC_QUERY2_REQUEST_PATH
                | EVERYTHING_IPC_QUERY2_REQUEST_SIZE
                | EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED;

            // Pack EVERYTHING_IPC_QUERY2 header (7 x u32 = 28 bytes) + UTF-16LE NUL-terminated query.
            let wide: Vec<u16> = search.encode_utf16().chain(std::iter::once(0)).collect();
            let mut payload = Vec::with_capacity(28 + wide.len() * 2);
            payload.extend_from_slice(&(recv_hwnd.0 as u32).to_le_bytes());
            payload.extend_from_slice(&reply_id.to_le_bytes());
            payload.extend_from_slice(&search_flags.to_le_bytes());
            payload.extend_from_slice(&opts.offset.to_le_bytes());
            payload.extend_from_slice(&opts.max_results.to_le_bytes());
            payload.extend_from_slice(&request_flags.to_le_bytes());
            payload.extend_from_slice(&(opts.sort as u32).to_le_bytes());
            for u in wide {
                payload.extend_from_slice(&u.to_le_bytes());
            }

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
                    SMTO_ABORTIFHUNG,
                    opts.timeout_ms,
                    Some(&mut _out),
                )
            };

            let deadline = Instant::now() + Duration::from_millis(opts.timeout_ms as u64);
            if send_res.0 != 0 {
                while slot.result.is_none() {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
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
                .unwrap_or_else(|| Err(EverythingError::Timeout(opts.timeout_ms)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_list2_buffer() {
        let mut buf = Vec::new();
        // Header: totfolders=1, totfiles=1, totitems=2, numitems=1, offset=0, flags=NAME|PATH|SIZE, sort=1
        let flags = EVERYTHING_IPC_QUERY2_REQUEST_NAME
            | EVERYTHING_IPC_QUERY2_REQUEST_PATH
            | EVERYTHING_IPC_QUERY2_REQUEST_SIZE;
        for v in [1u32, 1, 2, 1, 0, flags, 1] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        // Item 0 header at offset 28: flags=0 (file), data_offset=36
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&36u32.to_le_bytes());

        // Name: "demo.rs" (len 7)
        buf.extend_from_slice(&7u32.to_le_bytes());
        for u in "demo.rs".encode_utf16().chain(std::iter::once(0)) {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        // Path: "D:\\Projects" (len 11)
        buf.extend_from_slice(&11u32.to_le_bytes());
        for u in "D:\\Projects".encode_utf16().chain(std::iter::once(0)) {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        // Size: 4096
        buf.extend_from_slice(&4096u64.to_le_bytes());

        let parsed = parse_list2_buffer(&buf).expect("should parse synthetic buffer");
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].name, "demo.rs");
        assert_eq!(parsed.entries[0].full_path, "D:\\Projects\\demo.rs");
        assert_eq!(parsed.entries[0].size, Some(4096));
        assert!(!parsed.entries[0].is_folder);
    }

    #[cfg(windows)]
    #[test]
    fn loopback_wm_copydata_roundtrip() {
        use std::ffi::c_void;
        use std::sync::mpsc;
        use windows::core::w;
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::DataExchange::COPYDATASTRUCT;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, TranslateMessage,
            MSG, WM_CLOSE, WM_COPYDATA, WNDCLASSW,
        };

        // Only run mock server if real Everything isn't already on this desktop.
        if detect_running().is_some() {
            return;
        }

        unsafe extern "system" fn mock_ev_wndproc(
            hwnd: HWND,
            msg: u32,
            _wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if msg == WM_COPYDATA {
                let cds = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
                if cds.dwData == EVERYTHING_IPC_COPYDATA_QUERY2W && cds.cbData >= 28 {
                    let req = unsafe {
                        std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize)
                    };
                    let reply_hwnd_u32 = u32::from_le_bytes(req[0..4].try_into().unwrap());
                    let reply_id = u32::from_le_bytes(req[4..8].try_into().unwrap());
                    let req_flags = u32::from_le_bytes(req[20..24].try_into().unwrap());

                    // Verify UTF-16LE query string sent by client
                    let query_u16: Vec<u16> = req[28..]
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .take_while(|&u| u != 0)
                        .collect();
                    let query_str = String::from_utf16_lossy(&query_u16);

                    // Build valid EVERYTHING_IPC_LIST2 reply
                    let mut reply = Vec::new();
                    for v in [0u32, 1, 1, 1, 0, req_flags, 1] {
                        reply.extend_from_slice(&v.to_le_bytes());
                    }
                    reply.extend_from_slice(&0u32.to_le_bytes()); // file
                    reply.extend_from_slice(&36u32.to_le_bytes()); // data_offset

                    if (req_flags & EVERYTHING_IPC_QUERY2_REQUEST_NAME) != 0 {
                        let name = format!("matched_{query_str}");
                        reply.extend_from_slice(&(name.encode_utf16().count() as u32).to_le_bytes());
                        for u in name.encode_utf16().chain(std::iter::once(0)) {
                            reply.extend_from_slice(&u.to_le_bytes());
                        }
                    }
                    if (req_flags & EVERYTHING_IPC_QUERY2_REQUEST_PATH) != 0 {
                        let path = "D:\\Projects\\Tomoyo";
                        reply.extend_from_slice(&(path.encode_utf16().count() as u32).to_le_bytes());
                        for u in path.encode_utf16().chain(std::iter::once(0)) {
                            reply.extend_from_slice(&u.to_le_bytes());
                        }
                    }
                    if (req_flags & EVERYTHING_IPC_QUERY2_REQUEST_SIZE) != 0 {
                        reply.extend_from_slice(&2048u64.to_le_bytes());
                    }
                    if (req_flags & EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED) != 0 {
                        reply.extend_from_slice(&133_500_000_000_000_000u64.to_le_bytes());
                    }

                    let reply_cds = COPYDATASTRUCT {
                        dwData: reply_id as usize,
                        cbData: reply.len() as u32,
                        lpData: reply.as_mut_ptr() as *mut c_void,
                    };
                    let target = HWND(reply_hwnd_u32 as usize as *mut c_void);
                    unsafe {
                        let _ = SendMessageW(
                            target,
                            WM_COPYDATA,
                            Some(WPARAM(hwnd.0 as usize)),
                            Some(LPARAM(&reply_cds as *const _ as isize)),
                        );
                    }
                    return LRESULT(1);
                }
            } else if msg == WM_CLOSE {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                    PostQuitMessage(0);
                }
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, _wparam, lparam) }
        }

        let (tx, rx) = mpsc::channel::<usize>();
        let server = std::thread::spawn(move || unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let cls = w!("EVERYTHING_TASKBAR_NOTIFICATION");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(mock_ev_wndproc),
                hInstance: hinst.into(),
                lpszClassName: cls,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                Default::default(),
                cls,
                w!(""),
                Default::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap();
            let _ = tx.send(hwnd.0 as usize);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });

        let server_hwnd = rx.recv().unwrap();
        assert_eq!(detect_running(), Some(EverythingVersion::V14));

        let resp = query("Cargo.toml", &EverythingQueryOptions::default())
            .expect("loopback Everything IPC query should succeed");
        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].name, "matched_Cargo.toml");
        assert_eq!(resp.entries[0].full_path, "D:\\Projects\\Tomoyo\\matched_Cargo.toml");
        assert_eq!(resp.entries[0].size, Some(2048));
        assert!(resp.entries[0].modified_local().is_some());

        unsafe {
            let _ = PostMessageW(
                Some(HWND(server_hwnd as *mut c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        let _ = server.join();
    }

    #[test]
    fn live_everything_query_if_running() {
        if let Some(ver) = detect_running() {
            let resp = query(
                "Cargo.toml",
                &EverythingQueryOptions {
                    max_results: 5,
                    ..Default::default()
                },
            )
            .expect("live Everything query failed");
            println!(
                "Live Everything ({ver:?}) returned {} / {} total items",
                resp.entries.len(),
                resp.total_items
            );
            for e in &resp.entries {
                println!(
                    "  [{}] {} ({:?})",
                    if e.is_folder { "DIR" } else { "FILE" },
                    e.full_path,
                    e.formatted_size()
                );
            }
            assert!(resp.total_items > 0, "expected at least one Cargo.toml on disk");
        }
    }
}
