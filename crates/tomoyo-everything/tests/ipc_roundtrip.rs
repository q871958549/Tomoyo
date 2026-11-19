use tomoyo_everything::wire::{
    parse_list2_buffer, EVERYTHING_IPC_COPYDATA_QUERY2W, EVERYTHING_IPC_QUERY2_REQUEST_DATE_MODIFIED,
    EVERYTHING_IPC_QUERY2_REQUEST_NAME, EVERYTHING_IPC_QUERY2_REQUEST_PATH,
    EVERYTHING_IPC_QUERY2_REQUEST_SIZE,
};
use tomoyo_everything::{detect_running, query, EverythingQueryOptions, EverythingVersion};

#[test]
fn parses_synthetic_list2_buffer() {
    let mut buf = Vec::new();
    let flags = EVERYTHING_IPC_QUERY2_REQUEST_NAME
        | EVERYTHING_IPC_QUERY2_REQUEST_PATH
        | EVERYTHING_IPC_QUERY2_REQUEST_SIZE;
    for v in [1u32, 1, 2, 1, 0, flags, 1] {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&36u32.to_le_bytes());

    buf.extend_from_slice(&7u32.to_le_bytes());
    for u in "demo.rs".encode_utf16().chain(std::iter::once(0)) {
        buf.extend_from_slice(&u.to_le_bytes());
    }
    buf.extend_from_slice(&11u32.to_le_bytes());
    for u in "D:\\Projects".encode_utf16().chain(std::iter::once(0)) {
        buf.extend_from_slice(&u.to_le_bytes());
    }
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
        PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, TranslateMessage, MSG,
        WM_CLOSE, WM_COPYDATA, WNDCLASSW,
    };

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
                let query_u16: Vec<u16> = req[28..]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&u| u != 0)
                    .collect();
                let query_str = String::from_utf16_lossy(&query_u16);

                let mut reply = Vec::new();
                for v in [0u32, 1, 1, 1, 0, req_flags, 1] {
                    reply.extend_from_slice(&v.to_le_bytes());
                }
                reply.extend_from_slice(&0u32.to_le_bytes());
                reply.extend_from_slice(&36u32.to_le_bytes());

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
    if let Some(_ver) = detect_running() {
        let resp = query(
            "Cargo.toml",
            &EverythingQueryOptions {
                max_results: 5,
                ..Default::default()
            },
        )
        .expect("live Everything query failed");
        assert!(resp.total_items > 0);
    }
}
