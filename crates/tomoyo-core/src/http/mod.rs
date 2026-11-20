//! Zero-bloat HTTPS GET/POST & streaming download helper built directly on Windows native `WinHTTP`.
//!
//! Uses the OS certificate store & automatic system proxy, follows HTTPS redirects,
//! reads response headers (`ETag`, `Location`, `Content-Length`, `WINHTTP_OPTION_URL`),
//! and enforces strict timeouts + size caps without pulling reqwest/hyper/tokio.

mod download;

pub use download::{http_download_to_file, http_download_to_file_with_progress};

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

#[derive(Debug, Clone)]
pub struct HttpDetailedResponse {
    pub status_code: u16,
    pub body: Vec<u8>,
    pub final_url: String,
    pub etag: Option<String>,
    pub location: Option<String>,
    pub content_length: Option<u64>,
}

impl HttpDetailedResponse {
    pub fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[cfg(windows)]
pub(super) struct WinHttpHandle(pub(super) *mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = windows::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
pub(super) struct OpenHttpStream {
    pub(super) _session: WinHttpHandle,
    pub(super) _conn: WinHttpHandle,
    pub(super) req: WinHttpHandle,
    pub(super) status_code: u16,
    pub(super) final_url: String,
    pub(super) etag: Option<String>,
    pub(super) location: Option<String>,
    pub(super) content_length: Option<u64>,
}

#[cfg(windows)]
unsafe fn query_winhttp_string_header(req: *mut std::ffi::c_void, flag: u32) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::WinHttpQueryHeaders;

    let mut buf = [0u16; 1024];
    let mut len_bytes = (buf.len() * 2) as u32;
    let ok = unsafe {
        WinHttpQueryHeaders(
            req,
            flag,
            PCWSTR::null(),
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            &mut len_bytes,
            std::ptr::null_mut(),
        )
    }
    .is_ok();
    if !ok || len_bytes == 0 {
        return None;
    }
    let chars = (len_bytes as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars])
        .trim_end_matches('\0')
        .trim()
        .to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(windows)]
unsafe fn query_winhttp_final_url(req: *mut std::ffi::c_void) -> Option<String> {
    use windows::Win32::Networking::WinHttp::{WinHttpQueryOption, WINHTTP_OPTION_URL};

    let mut buf = [0u16; 1024];
    let mut len_bytes = (buf.len() * 2) as u32;
    let ok = unsafe {
        WinHttpQueryOption(
            req,
            WINHTTP_OPTION_URL,
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            &mut len_bytes,
        )
    }
    .is_ok();
    if !ok || len_bytes == 0 {
        return None;
    }
    let chars = (len_bytes as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars])
        .trim_end_matches('\0')
        .trim()
        .to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(windows)]
pub(super) fn open_http_stream(
    method: &str,
    url: &str,
    extra_headers: &[(&str, &str)],
    body_opt: Option<&[u8]>,
    timeout_ms: u32,
    allow_redirects: bool,
) -> Result<OpenHttpStream, String> {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::{
        WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
        URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
        WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_REDIRECT_POLICY,
        WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS, WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
        WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_ETAG, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE,
    };

    let url_w: Vec<u16> = url.encode_utf16().collect();
    let mut host_buf = [0u16; 256];
    let mut path_buf = [0u16; 4096];
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
    let method_w: Vec<u16> = method.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let custom_ua = extra_headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| *v);
        let default_ua = format!("Tomoyo/{} (Windows; OpenSource)", env!("CARGO_PKG_VERSION"));
        let ua: Vec<u16> = custom_ua
            .unwrap_or(&default_ua)
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

        let flags = if is_https { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let req_raw = WinHttpOpenRequest(
            conn.0,
            PCWSTR(method_w.as_ptr()),
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

        let policy = if allow_redirects {
            WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS
        } else {
            WINHTTP_OPTION_REDIRECT_POLICY_NEVER
        };
        let policy_bytes = policy.to_ne_bytes();
        let _ = WinHttpSetOption(Some(req.0), WINHTTP_OPTION_REDIRECT_POLICY, Some(&policy_bytes));

        let mut hdr_str = String::from("Accept: application/vnd.github+json, application/json, */*\r\n");
        for (k, v) in extra_headers {
            if k.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            hdr_str.push_str(k);
            hdr_str.push_str(": ");
            hdr_str.push_str(v);
            hdr_str.push_str("\r\n");
        }
        let headers_w: Vec<u16> = hdr_str.encode_utf16().collect();
        let (opt_ptr, opt_len) = match body_opt {
            Some(b) if !b.is_empty() => (Some(b.as_ptr() as *const c_void), b.len() as u32),
            _ => (None, 0u32),
        };

        WinHttpSendRequest(req.0, Some(&headers_w), opt_ptr, opt_len, opt_len, 0)
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
        let final_url = query_winhttp_final_url(req.0).unwrap_or_else(|| url.to_string());
        let etag = query_winhttp_string_header(req.0, WINHTTP_QUERY_ETAG);
        let location = query_winhttp_string_header(req.0, WINHTTP_QUERY_LOCATION);
        let content_length = query_winhttp_string_header(req.0, WINHTTP_QUERY_CONTENT_LENGTH)
            .and_then(|s| s.parse::<u64>().ok());

        Ok(OpenHttpStream {
            _session: session,
            _conn: conn,
            req,
            status_code: status_code as u16,
            final_url,
            etag,
            location,
            content_length,
        })
    }
}

#[cfg(windows)]
fn read_stream_bounded(stream: &OpenHttpStream, max_bytes: usize) -> Vec<u8> {
    use std::ffi::c_void;
    use windows::Win32::Networking::WinHttp::{WinHttpQueryDataAvailable, WinHttpReadData};

    let mut body = Vec::with_capacity(4096);
    unsafe {
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(stream.req.0, &mut avail).is_err() || avail == 0 {
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
                stream.req.0,
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
    }
    body
}

#[cfg(windows)]
pub fn http_get(url: &str, timeout_ms: u32, max_bytes: usize) -> Result<HttpResponse, String> {
    http_get_with_headers(url, &[], timeout_ms, max_bytes)
}

#[cfg(windows)]
pub fn http_get_with_headers(
    url: &str,
    headers: &[(&str, &str)],
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<HttpResponse, String> {
    let stream = open_http_stream("GET", url, headers, None, timeout_ms, true)?;
    let body = read_stream_bounded(&stream, max_bytes);
    Ok(HttpResponse { status_code: stream.status_code, body })
}

#[cfg(windows)]
pub fn http_get_detailed(
    url: &str,
    headers: &[(&str, &str)],
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<HttpDetailedResponse, String> {
    let stream = open_http_stream("GET", url, headers, None, timeout_ms, true)?;
    let body = read_stream_bounded(&stream, max_bytes);
    Ok(HttpDetailedResponse {
        status_code: stream.status_code,
        body,
        final_url: stream.final_url,
        etag: stream.etag,
        location: stream.location,
        content_length: stream.content_length,
    })
}

#[cfg(windows)]
pub fn http_head_redirect(url: &str, timeout_ms: u32) -> Result<HttpDetailedResponse, String> {
    let stream = open_http_stream("HEAD", url, &[], None, timeout_ms, false)?;
    Ok(HttpDetailedResponse {
        status_code: stream.status_code,
        body: Vec::new(),
        final_url: stream.final_url,
        etag: stream.etag,
        location: stream.location,
        content_length: stream.content_length,
    })
}

#[cfg(windows)]
pub fn http_post_bytes(
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<HttpResponse, String> {
    let stream = open_http_stream("POST", url, headers, Some(body), timeout_ms, true)?;
    let out = read_stream_bounded(&stream, max_bytes);
    Ok(HttpResponse { status_code: stream.status_code, body: out })
}

#[cfg(not(windows))]
pub fn http_get(_url: &str, _timeout_ms: u32, _max_bytes: usize) -> Result<HttpResponse, String> {
    Err("WinHTTP is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn http_get_with_headers(
    _url: &str,
    _headers: &[(&str, &str)],
    _timeout_ms: u32,
    _max_bytes: usize,
) -> Result<HttpResponse, String> {
    Err("WinHTTP is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn http_get_detailed(
    _url: &str,
    _headers: &[(&str, &str)],
    _timeout_ms: u32,
    _max_bytes: usize,
) -> Result<HttpDetailedResponse, String> {
    Err("WinHTTP is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn http_head_redirect(_url: &str, _timeout_ms: u32) -> Result<HttpDetailedResponse, String> {
    Err("WinHTTP is only available on Windows".into())
}

#[cfg(not(windows))]
pub fn http_post_bytes(
    _url: &str,
    _headers: &[(&str, &str)],
    _body: &[u8],
    _timeout_ms: u32,
    _max_bytes: usize,
) -> Result<HttpResponse, String> {
    Err("WinHTTP is only available on Windows".into())
}
