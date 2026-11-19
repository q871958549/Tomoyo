//! Zero-bloat HTTPS GET helper built directly on Windows native `WinHTTP`.
//!
//! Avoids pulling in reqwest/hyper/rustls/tokio (~3+ MB binary overhead & background runtime),
//! uses the OS certificate store & automatic system proxy, and enforces strict timeouts + size caps.

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Performs a bounded HTTPS/HTTP GET request with strict timeout (ms) and byte ceiling.
#[cfg(windows)]
pub fn http_get(url: &str, timeout_ms: u32, max_bytes: usize) -> Result<HttpResponse, String> {
    use std::ffi::c_void;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest,
        WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
        WinHttpSendRequest, WinHttpSetTimeouts, URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
        WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE,
    };

    let url_w: Vec<u16> = url.encode_utf16().collect();
    let mut host_buf = [0u16; 256];
    let mut path_buf = [0u16; 2048];

    let mut comps = URL_COMPONENTS {
        dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
        lpszHostName: windows::core::PWSTR(host_buf.as_mut_ptr()),
        dwHostNameLength: host_buf.len() as u32,
        lpszUrlPath: windows::core::PWSTR(path_buf.as_mut_ptr()),
        dwUrlPathLength: path_buf.len() as u32,
        ..Default::default()
    };

    unsafe {
        WinHttpCrackUrl(&url_w, 0, &mut comps)
            .map_err(|e| format!("invalid URL `{url}`: {e}"))?;
    }

    let host_len = comps.dwHostNameLength as usize;
    let path_len = comps.dwUrlPathLength as usize;
    let mut host_w = host_buf[..host_len].to_vec();
    host_w.push(0);
    let mut path_w = if path_len > 0 {
        path_buf[..path_len].to_vec()
    } else {
        vec![b'/' as u16]
    };
    path_w.push(0);

    let is_https = url.trim().to_ascii_lowercase().starts_with("https://");
    let port = if comps.nPort != 0 {
        comps.nPort
    } else if is_https {
        443
    } else {
        80
    };

    struct WinHttpHandle(*mut c_void);
    impl Drop for WinHttpHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    unsafe {
        let ua: Vec<u16> = format!("Tomoyo/{} (Windows; OpenSource)", env!("CARGO_PKG_VERSION"))
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let session_raw = WinHttpOpen(
            PCWSTR(ua.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session_raw.is_null() {
            return Err("WinHttpOpen failed".into());
        }
        let session = WinHttpHandle(session_raw);

        let t = timeout_ms as i32;
        let _ = WinHttpSetTimeouts(session.0, t, t, t, t);

        let conn_raw = WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), port, 0);
        if conn_raw.is_null() {
            return Err("WinHttpConnect failed".into());
        }
        let conn = WinHttpHandle(conn_raw);

        let flags = if is_https {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let req_raw = WinHttpOpenRequest(
            conn.0,
            w!("GET"),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        );
        if req_raw.is_null() {
            return Err("WinHttpOpenRequest failed".into());
        }
        let req = WinHttpHandle(req_raw);

        let headers: Vec<u16> = "Accept: application/vnd.github+json, application/json, */*\r\n"
            .encode_utf16()
            .collect();

        WinHttpSendRequest(req.0, Some(&headers), None, 0, 0, 0)
            .map_err(|e| format!("HTTP request failed: {e}"))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())
            .map_err(|e| format!("HTTP receive failed: {e}"))?;

        let mut status_code: u32 = 0;
        let mut status_len = std::mem::size_of::<u32>() as u32;
        let _ = WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status_code as *mut _ as *mut c_void),
            &mut status_len,
            std::ptr::null_mut(),
        );

        let mut body = Vec::with_capacity(8192);
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(req.0, &mut avail).is_err() || avail == 0 {
                break;
            }
            let to_read = (avail as usize).min(max_bytes.saturating_sub(body.len()));
            if to_read == 0 {
                break;
            }
            let old_len = body.len();
            body.resize(old_len + to_read, 0);
            let mut read: u32 = 0;
            if WinHttpReadData(
                req.0,
                body[old_len..].as_mut_ptr() as *mut c_void,
                to_read as u32,
                &mut read,
            )
            .is_err()
                || read == 0
            {
                body.truncate(old_len);
                break;
            }
            body.truncate(old_len + read as usize);
        }

        Ok(HttpResponse {
            status_code: status_code as u16,
            body,
        })
    }
}

#[cfg(not(windows))]
pub fn http_get(_url: &str, _timeout_ms: u32, _max_bytes: usize) -> Result<HttpResponse, String> {
    Err("WinHTTP is only available on Windows".into())
}
