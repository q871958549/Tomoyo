use super::{HttpDetailedResponse, HttpResponse};
use crate::linux_sys::curl_request;

pub fn http_get(url: &str, timeout_ms: u32, max_bytes: usize) -> Result<HttpResponse, String> {
    http_get_with_headers(url, &[], timeout_ms, max_bytes)
}

pub fn http_get_with_headers(
    url: &str,
    headers: &[(&str, &str)],
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<HttpResponse, String> {
    let res = curl_request("GET", url, headers, None, timeout_ms, max_bytes, true, false)?;
    Ok(HttpResponse {
        status_code: res.status_code,
        body: res.body,
    })
}

pub fn http_get_detailed(
    url: &str,
    headers: &[(&str, &str)],
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<HttpDetailedResponse, String> {
    let res = curl_request("GET", url, headers, None, timeout_ms, max_bytes, true, false)?;
    Ok(HttpDetailedResponse {
        status_code: res.status_code,
        body: res.body,
        final_url: res.final_url,
        etag: res.etag,
        location: res.location,
        content_length: res.content_length,
    })
}

pub fn http_head_redirect(url: &str, timeout_ms: u32) -> Result<HttpDetailedResponse, String> {
    let res = curl_request("HEAD", url, &[], None, timeout_ms, 0, false, true)?;
    Ok(HttpDetailedResponse {
        status_code: res.status_code,
        body: Vec::new(),
        final_url: res.final_url,
        etag: res.etag,
        location: res.location,
        content_length: res.content_length,
    })
}

pub fn http_post_bytes(
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<HttpResponse, String> {
    let res = curl_request("POST", url, headers, Some(body), timeout_ms, max_bytes, true, false)?;
    Ok(HttpResponse {
        status_code: res.status_code,
        body: res.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn libcurl_handles_get_headers_post_and_stream_download() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for idx in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let body = if idx == 0 {
                    assert!(req.contains("X-Test: ok"));
                    "hello-get"
                } else if idx == 1 {
                    assert!(req.starts_with("POST "));
                    assert!(req.contains("ping=1"));
                    "hello-post"
                } else {
                    "download-bytes-12345"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let base = format!("http://{addr}");
        let r1 = http_get_detailed(&format!("{base}/a"), &[("X-Test", "ok")], 3000, 4096).unwrap();
        assert_eq!(r1.status_code, 200);
        assert_eq!(r1.text_lossy(), "hello-get");
        assert_eq!(r1.etag.as_deref(), Some("\"v1\""));

        let r2 = http_post_bytes(&format!("{base}/b"), &[], b"ping=1", 3000, 4096).unwrap();
        assert_eq!(r2.status_code, 200);
        assert_eq!(r2.text_lossy(), "hello-post");

        let dl_path = std::env::temp_dir().join(format!("tomoyo_curl_dl_{}.txt", std::process::id()));
        let bytes = super::super::http_download_to_file(&format!("{base}/c"), &dl_path, 3000, 4096).unwrap();
        assert_eq!(bytes, 20);
        assert_eq!(std::fs::read_to_string(&dl_path).unwrap(), "download-bytes-12345");
        let _ = std::fs::remove_file(&dl_path);
        let _ = handle.join();
    }
}
