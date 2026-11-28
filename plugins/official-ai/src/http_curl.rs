//! Zero-bloat cross-platform HTTP helper backed by system `curl` / `curl.exe`.
//!
//! Passes headers & target URL via stdin (`--config -`) so secret Authorization tokens
//! never appear in process arguments (`/proc/<pid>/cmdline`).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

pub struct HttpResponse {
    pub status_code: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

struct TempBodyFile(PathBuf);

impl Drop for TempBodyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn is_safe_http_url(url: &str) -> bool {
    let low = url.trim().to_ascii_lowercase();
    (low.starts_with("https://") || low.starts_with("http://"))
        && !low.chars().any(|c| c.is_control() || c.is_whitespace())
}

pub fn http_post_bytes(
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<HttpResponse, String> {
    if !is_safe_http_url(url) {
        return Err("无效的 HTTP/HTTPS 接口地址".into());
    }
    let timeout_secs = (timeout_ms.max(1000) as f64) / 1000.0;
    let mut cfg = String::with_capacity(512);
    cfg.push_str("silent\nshow-error\nlocation\nrequest = POST\n");
    cfg.push_str(&format!("max-time = {timeout_secs:.2}\n"));
    cfg.push_str(&format!("connect-timeout = {:.2}\n", timeout_secs.min(6.0)));
    cfg.push_str(&format!("url = {}\n", curl_escape(url)));
    cfg.push_str("write-out = \"\\n%{http_code}\"\n");
    for (k, v) in headers {
        let pair = format!("{k}: {v}");
        cfg.push_str(&format!("header = {}\n", curl_escape(&pair)));
    }

    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let body_path = std::env::temp_dir().join(format!("tomoyo_ai_{}_{seq}.json", std::process::id()));
    std::fs::write(&body_path, body).map_err(|e| format!("写入请求体失败: {e}"))?;
    let _tmp_body = TempBodyFile(body_path.clone());
    let at_path = format!("@{}", body_path.to_string_lossy());
    cfg.push_str(&format!("data-binary = {}\n", curl_escape(&at_path)));

    let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
    let mut cmd = Command::new(curl_bin);
    cmd.args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 {curl_bin} 失败: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(cfg.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("网络请求中断: {e}"))?;
    if !out.status.success() && out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "无法连接 AI 接口服务".into() } else { err });
    }

    let stdout = out.stdout;
    let split_pos = stdout.iter().rposition(|&b| b == b'\n');
    let (raw_body, status_bytes) = match split_pos {
        Some(pos) => (&stdout[..pos], &stdout[pos + 1..]),
        None => (&stdout[..], &b"0"[..]),
    };
    let status_code = std::str::from_utf8(status_bytes)
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(0);
    if status_code == 0 {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "AI 请求超时或无法连接（请确认 Ollama / 接口已启动）".into() } else { err });
    }
    let capped = raw_body[..raw_body.len().min(max_bytes)].to_vec();
    Ok(HttpResponse { status_code, body: capped })
}

fn curl_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' | '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::is_safe_http_url;

    #[test]
    fn validates_http_urls_strictly() {
        assert!(is_safe_http_url("http://127.0.0.1:11434/api/chat"));
        assert!(is_safe_http_url("https://api.openai.com/v1/chat/completions"));
        assert!(!is_safe_http_url("file:///etc/passwd"));
        assert!(!is_safe_http_url("http://evil.local/\nheader=x"));
    }
}
