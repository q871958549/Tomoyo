//! Lock-free one-shot inline translation worker for Tomoyo Super Panel.
//!
//! Delegates translation exclusively to the installed `dev.tomoyo.translate` plugin binary
//! (`--quick-translate`) using a short-lived child process so Tomoyo Core holds **zero**
//! duplicate translation engine code and NEVER blocks `PluginHost::state` across network I/O.

use super::InlineTransState;
use serde_json::{json, Value};
use std::ffi::c_void;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tomoyo_core::Lang;
use tomoyo_plugin_host::StdioOneShotSpec;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const QUICK_TRANSLATE_TIMEOUT: Duration = Duration::from_secs(8);

pub struct TransOutcome {
    pub seq: u64,
    pub state: InlineTransState,
}

pub fn spawn_inline_translate(
    text: String,
    spec: StdioOneShotSpec,
    slot: Arc<Mutex<Option<TransOutcome>>>,
    hwnd: HWND,
    ready_msg: u32,
    seq: u64,
    lang: Lang,
) {
    let hwnd_raw = hwnd.0 as usize;
    let _ = std::thread::Builder::new()
        .name("tomoyo-sp-trans".into())
        .spawn(move || {
            let outcome = run_plugin_quick_translate(&spec, &text, lang);
            if let Ok(mut g) = slot.lock() {
                *g = Some(TransOutcome { seq, state: outcome });
            }
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd_raw as *mut c_void)),
                    ready_msg,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        });
}

fn run_plugin_quick_translate(spec: &StdioOneShotSpec, text: &str, lang: Lang) -> InlineTransState {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .arg("--quick-translate")
        .current_dir(&spec.workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(program = %spec.program, "Super Panel inline translate spawn failed: {e}");
            return InlineTransState::Error {
                message: lang.pick("翻译插件启动失败", "Failed to start translate plugin").into(),
            };
        }
    };

    let payload = json!({
        "text": text,
        "source_lang": "auto",
        "target_lang": "",
        "plugin_settings": spec.plugin_settings,
        "plugin_secrets": spec.plugin_secrets,
    });
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes());
    }

    let deadline = Instant::now() + QUICK_TRANSLATE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                tracing::warn!("Super Panel inline translate timed out after 8s");
                return InlineTransState::Error {
                    message: lang.pick("翻译请求超时，请点击「对照」重试", "Translation timed out").into(),
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break,
        }
    }

    let Ok(output) = child.wait_with_output() else {
        return InlineTransState::Error {
            message: lang.pick("读取翻译结果失败", "Failed to read translation output").into(),
        };
    };
    let raw = String::from_utf8_lossy(&output.stdout);
    let Ok(val) = serde_json::from_str::<Value>(raw.trim()) else {
        return InlineTransState::Error {
            message: lang.pick("翻译插件返回格式异常", "Invalid response from translate plugin").into(),
        };
    };
    parse_translate_json(&val, lang)
}

pub fn parse_translate_json(val: &Value, lang: Lang) -> InlineTransState {
    let results = val.get("results").and_then(Value::as_array);
    if let Some(arr) = results {
        for r in arr {
            let ok = r.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let translated = r.get("translated").and_then(Value::as_str).unwrap_or("").trim();
            if ok && !translated.is_empty() {
                let engine_name = r
                    .get("engine_name")
                    .and_then(Value::as_str)
                    .unwrap_or("Translate")
                    .to_string();
                let phonetic = r
                    .get("phonetic")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string);
                return InlineTransState::Ready {
                    translated: translated.to_string(),
                    engine_name,
                    phonetic,
                };
            }
        }
        for r in arr {
            if let Some(err) = r.get("error").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                return InlineTransState::Error { message: err.to_string() };
            }
        }
    }
    InlineTransState::Error {
        message: lang
            .pick("所有翻译引擎暂未返回结果", "No translation engine returned a result")
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_successful_engine_translation_with_phonetic() {
        let val = json!({
            "trace_id": "TRN-1",
            "source_text": "hello",
            "target_lang": "zh-Hans",
            "results": [
                { "engine_id": "deepl", "engine_name": "DeepL", "ok": false, "translated": "", "error": "missing key" },
                { "engine_id": "google", "engine_name": "Google", "ok": true, "translated": "你好，世界", "phonetic": "həˈloʊ" }
            ]
        });
        match parse_translate_json(&val, Lang::Zh) {
            InlineTransState::Ready { translated, engine_name, phonetic } => {
                assert_eq!(translated, "你好，世界");
                assert_eq!(engine_name, "Google");
                assert_eq!(phonetic.as_deref(), Some("həˈloʊ"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }
}
