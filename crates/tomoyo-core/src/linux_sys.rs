//! Safe Rust wrappers around Linux C shims (`libcurl`, `fontconfig`, `gdk-pixbuf`, `libsecret`, `GDBus`).

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

type BytesCb = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> usize;
type ProgCb = unsafe extern "C" fn(*mut c_void, u64, u64) -> c_int;
type StrCb = unsafe extern "C" fn(*mut c_void, *const c_char, usize);
type WinCb = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char);

unsafe extern "C" {
    fn tomoyo_curl_perform(
        method: *const c_char,
        url: *const c_char,
        headers_nl: *const c_char,
        post_body: *const u8,
        post_len: usize,
        timeout_ms: u32,
        follow_redirects: c_int,
        nobody: c_int,
        body_cb: BytesCb,
        hdr_cb: BytesCb,
        prog_cb: Option<ProgCb>,
        ctx: *mut c_void,
        out_status: *mut u16,
        final_url_cb: Option<StrCb>,
        err_buf: *mut c_char,
        err_buf_len: usize,
    ) -> c_int;
    fn tomoyo_fc_add_font(path: *const c_char) -> c_int;
    fn tomoyo_fc_clear_fonts();
    fn tomoyo_fc_list_families(cb: StrCb, ctx: *mut c_void) -> c_int;
    fn tomoyo_decode_image_rgba(
        path: *const c_char,
        max_w: c_int,
        max_h: c_int,
        out_src_w: *mut u32,
        out_src_h: *mut u32,
        out_dst_w: *mut u32,
        out_dst_h: *mut u32,
        rgba_cb: BytesCb,
        ctx: *mut c_void,
    ) -> c_int;
    fn tomoyo_secret_store(key: *const c_char, val: *const c_char) -> c_int;
    fn tomoyo_secret_lookup(key: *const c_char, out_cb: StrCb, ctx: *mut c_void) -> c_int;
    fn tomoyo_secret_clear(key: *const c_char) -> c_int;
    fn tomoyo_dbus_krunner_windows(cb: WinCb, ctx: *mut c_void) -> c_int;
    fn tomoyo_dbus_krunner_run(match_id: *const c_char) -> c_int;
    fn tomoyo_dbus_reveal_file(file_uri: *const c_char) -> c_int;
    fn tomoyo_dbus_klipper_get(cb: StrCb, ctx: *mut c_void) -> c_int;
}

pub struct CurlReqOutcome {
    pub status_code: u16,
    pub body: Vec<u8>,
    pub final_url: String,
    pub etag: Option<String>,
    pub location: Option<String>,
    pub content_length: Option<u64>,
}

type ChunkSink<'a> = &'a mut dyn FnMut(&[u8]) -> Result<(), String>;
type ProgressSink<'a> = &'a mut dyn FnMut(u64, Option<u64>);

struct CaptureCtx<'a> {
    body: Option<&'a mut Vec<u8>>,
    max_bytes: usize,
    sink: Option<ChunkSink<'a>>,
    prog: Option<ProgressSink<'a>>,
    last_emit: u64,
    write_err: Option<String>,
    final_url: String,
    etag: Option<String>,
    location: Option<String>,
    content_length: Option<u64>,
}

unsafe extern "C" fn on_body_bytes(ctx: *mut c_void, data: *const u8, len: usize) -> usize {
    let st = unsafe { &mut *(ctx as *mut CaptureCtx<'_>) };
    let slice = unsafe { std::slice::from_raw_parts(data, len) };
    if let Some(sink) = st.sink.as_deref_mut() {
        if let Err(e) = sink(slice) {
            st.write_err = Some(e);
            return 0;
        }
        return len;
    }
    if let Some(buf) = st.body.as_deref_mut() {
        let rem = st.max_bytes.saturating_sub(buf.len());
        let take = len.min(rem);
        buf.extend_from_slice(&slice[..take]);
        return len;
    }
    len
}

unsafe extern "C" fn on_header_bytes(ctx: *mut c_void, data: *const u8, len: usize) -> usize {
    let st = unsafe { &mut *(ctx as *mut CaptureCtx<'_>) };
    let line = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(data, len) });
    if let Some((k, v)) = line.split_once(':') {
        let kl = k.trim().to_ascii_lowercase();
        let val = v.trim();
        match kl.as_str() {
            "etag" if !val.is_empty() => st.etag = Some(val.to_string()),
            "location" if !val.is_empty() => st.location = Some(val.to_string()),
            "content-length" => st.content_length = val.parse::<u64>().ok(),
            _ => {}
        }
    }
    len
}

unsafe extern "C" fn on_progress(ctx: *mut c_void, dl_now: u64, dl_total: u64) -> c_int {
    let st = unsafe { &mut *(ctx as *mut CaptureCtx<'_>) };
    if let Some(cb) = st.prog.as_deref_mut()
        && (dl_now == 0 || dl_now >= st.last_emit + 64 * 1024 || (dl_total > 0 && dl_now >= dl_total))
    {
        st.last_emit = dl_now;
        cb(dl_now, (dl_total > 0).then_some(dl_total));
    }
    0
}

unsafe extern "C" fn on_final_url(ctx: *mut c_void, s: *const c_char, len: usize) {
    let st = unsafe { &mut *(ctx as *mut CaptureCtx<'_>) };
    let bytes = unsafe { std::slice::from_raw_parts(s as *const u8, len) };
    st.final_url = String::from_utf8_lossy(bytes).into_owned();
}

#[allow(clippy::too_many_arguments)]
pub fn curl_request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    post_body: Option<&[u8]>,
    timeout_ms: u32,
    max_bytes: usize,
    follow_redirects: bool,
    nobody: bool,
) -> Result<CurlReqOutcome, String> {
    let mut body = Vec::new();
    let (status, final_url, etag, location, content_length) = {
        let mut ctx = CaptureCtx {
            body: Some(&mut body),
            max_bytes,
            sink: None,
            prog: None,
            last_emit: 0,
            write_err: None,
            final_url: url.to_string(),
            etag: None,
            location: None,
            content_length: None,
        };
        let status = run_curl_inner(
            method,
            url,
            headers,
            post_body,
            timeout_ms,
            follow_redirects,
            nobody,
            false,
            &mut ctx,
        )?;
        (status, ctx.final_url, ctx.etag, ctx.location, ctx.content_length)
    };
    Ok(CurlReqOutcome {
        status_code: status,
        body,
        final_url,
        etag,
        location,
        content_length,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn curl_stream_download(
    url: &str,
    timeout_ms: u32,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    prog: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(u16, Option<u64>), String> {
    let mut ctx = CaptureCtx {
        body: None,
        max_bytes: 0,
        sink: Some(sink),
        prog: Some(prog),
        last_emit: 0,
        write_err: None,
        final_url: url.to_string(),
        etag: None,
        location: None,
        content_length: None,
    };
    let status = run_curl_inner("GET", url, &[], None, timeout_ms, true, false, true, &mut ctx)?;
    if let Some(err) = ctx.write_err {
        return Err(err);
    }
    Ok((status, ctx.content_length))
}

#[allow(clippy::too_many_arguments)]
fn run_curl_inner(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    post_body: Option<&[u8]>,
    timeout_ms: u32,
    follow_redirects: bool,
    nobody: bool,
    with_prog: bool,
    ctx: &mut CaptureCtx<'_>,
) -> Result<u16, String> {
    let c_method = CString::new(method).map_err(|_| "invalid HTTP method")?;
    let c_url = CString::new(url).map_err(|_| "invalid URL")?;
    let hdr_str: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n");
    let c_hdr = CString::new(hdr_str).map_err(|_| "invalid HTTP header")?;
    let mut status = 0u16;
    let mut err_buf = [0i8; 256];
    let rc = unsafe {
        tomoyo_curl_perform(
            c_method.as_ptr(),
            c_url.as_ptr(),
            c_hdr.as_ptr(),
            post_body.map_or(std::ptr::null(), |b| b.as_ptr()),
            post_body.map_or(0, |b| b.len()),
            timeout_ms,
            c_int::from(follow_redirects),
            c_int::from(nobody),
            on_body_bytes,
            on_header_bytes,
            with_prog.then_some(on_progress as ProgCb),
            (ctx as *mut CaptureCtx<'_>).cast::<c_void>(),
            &mut status,
            Some(on_final_url),
            err_buf.as_mut_ptr(),
            err_buf.len(),
        )
    };
    if rc != 0 {
        let msg = unsafe { CStr::from_ptr(err_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        return Err(if msg.is_empty() { format!("libcurl error {rc}") } else { msg });
    }
    Ok(status)
}

pub fn fc_add_font(path: &Path) -> bool {
    let Ok(c_path) = CString::new(path.to_string_lossy().as_bytes()) else { return false; };
    unsafe { tomoyo_fc_add_font(c_path.as_ptr()) != 0 }
}

pub fn fc_clear_fonts() {
    unsafe { tomoyo_fc_clear_fonts(); }
}

pub fn fc_list_font_families() -> Vec<String> {
    use std::collections::HashSet;
    struct Collect {
        seen: HashSet<String>,
        out: Vec<String>,
    }
    unsafe extern "C" fn on_fam(ctx: *mut c_void, s: *const c_char, len: usize) {
        if ctx.is_null() || s.is_null() || len == 0 {
            return;
        }
        let c = unsafe { &mut *(ctx as *mut Collect) };
        let raw = unsafe { std::slice::from_raw_parts(s.cast::<u8>(), len) };
        let name = String::from_utf8_lossy(raw).trim().to_string();
        if !name.is_empty() && !name.starts_with('@') && !name.starts_with('.') {
            let key = name.to_ascii_lowercase();
            if c.seen.insert(key) {
                c.out.push(name);
            }
        }
    }
    let mut col = Collect {
        seen: HashSet::new(),
        out: Vec::new(),
    };
    unsafe {
        tomoyo_fc_list_families(on_fam, (&mut col as *mut Collect).cast());
    }
    col.out
}

unsafe extern "C" fn collect_rgba_row(ctx: *mut c_void, data: *const u8, len: usize) -> usize {
    let buf = unsafe { &mut *(ctx as *mut Vec<u8>) };
    buf.extend_from_slice(unsafe { std::slice::from_raw_parts(data, len) });
    len
}

pub fn decode_image_rgba(path: &Path, max_w: u32, max_h: u32) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let c_path = CString::new(path.to_string_lossy().as_bytes()).ok()?;
    let (mut sw, mut sh, mut dw, mut dh) = (0u32, 0u32, 0u32, 0u32);
    let mut rgba = Vec::new();
    let ok = unsafe {
        tomoyo_decode_image_rgba(
            c_path.as_ptr(),
            max_w as c_int,
            max_h as c_int,
            &mut sw,
            &mut sh,
            &mut dw,
            &mut dh,
            collect_rgba_row,
            (&mut rgba as *mut Vec<u8>).cast::<c_void>(),
        )
    };
    (ok != 0 && dw > 0 && dh > 0 && rgba.len() == (dw * dh * 4) as usize)
        .then_some((sw, sh, dw, dh, rgba))
}

unsafe extern "C" fn collect_str(ctx: *mut c_void, s: *const c_char, len: usize) {
    let out = unsafe { &mut *(ctx as *mut String) };
    let bytes = unsafe { std::slice::from_raw_parts(s as *const u8, len) };
    out.push_str(&String::from_utf8_lossy(bytes));
}

pub fn secret_store(key: &str, val: &str) -> bool {
    let (Ok(ck), Ok(cv)) = (CString::new(key), CString::new(val)) else { return false; };
    unsafe { tomoyo_secret_store(ck.as_ptr(), cv.as_ptr()) != 0 }
}

pub fn secret_lookup(key: &str) -> Option<String> {
    let ck = CString::new(key).ok()?;
    let mut out = String::new();
    let ok = unsafe {
        tomoyo_secret_lookup(ck.as_ptr(), collect_str, (&mut out as *mut String).cast::<c_void>())
    };
    (ok != 0 && !out.is_empty()).then_some(out)
}

pub fn secret_clear(key: &str) -> bool {
    let Ok(ck) = CString::new(key) else { return false; };
    unsafe { tomoyo_secret_clear(ck.as_ptr()) != 0 }
}

unsafe extern "C" fn collect_krunner_win(
    ctx: *mut c_void,
    id: *const c_char,
    title: *const c_char,
    app_id: *const c_char,
) {
    let out = unsafe { &mut *(ctx as *mut Vec<(String, String, String)>) };
    let sid = unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned();
    let stitle = unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned();
    let sapp = unsafe { CStr::from_ptr(app_id) }.to_string_lossy().into_owned();
    out.push((sid, stitle, sapp));
}

pub fn dbus_krunner_windows() -> Option<Vec<(String, String, String)>> {
    let mut out = Vec::new();
    let ok = unsafe {
        tomoyo_dbus_krunner_windows(
            collect_krunner_win,
            (&mut out as *mut Vec<(String, String, String)>).cast::<c_void>(),
        )
    };
    (ok != 0).then_some(out)
}

pub fn dbus_krunner_run(match_id: &str) -> bool {
    let Ok(cid) = CString::new(match_id) else { return false; };
    unsafe { tomoyo_dbus_krunner_run(cid.as_ptr()) != 0 }
}

pub fn dbus_reveal_file(path: &str) -> bool {
    let uri = if path.starts_with("file://") {
        path.to_string()
    } else {
        format!("file://{path}")
    };
    let Ok(curi) = CString::new(uri) else { return false; };
    unsafe { tomoyo_dbus_reveal_file(curi.as_ptr()) != 0 }
}

pub fn dbus_klipper_get() -> Option<String> {
    let mut out = String::new();
    let ok = unsafe {
        tomoyo_dbus_klipper_get(collect_str, (&mut out as *mut String).cast::<c_void>())
    };
    (ok != 0).then_some(out)
}
