//! Official Tomoyo Protocol v3 Plugin: Native Screenshot, Annotations, Pin-to-Desktop & Recording.
//!
//! Spawns the sibling ephemeral `tomoyo-capture` binary strictly on demand (0 idle RAM).

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use tomoyo_sdk::{
    ActionKind, ActionSpec, ExecuteRequest, ExecuteResponse, IconSpec, PluginEventKind,
    PluginEventPayload, PluginMeta, PreviewSpec, QueryContext, ResultCategory, ResultItem,
    TomoyoPlugin,
};

const PLUGIN_ID: &str = "dev.tomoyo.capture";

struct CaptureCmd {
    id: &'static str,
    mode: &'static str,
    title: &'static str,
    subtitle: &'static str,
    badge: &'static str,
    keywords: &'static str,
    base_score: i64,
}

const COMMANDS: &[CaptureCmd] = &[
    CaptureCmd {
        id: "capture:region",
        mode: "region",
        title: "区域截图与标注 (Screenshot & Annotate)",
        subtitle: "多屏 DPI 精准选区 · 窗口吸附 · 取色放大镜 (C) · 框/箭/画笔/马赛克/贴图",
        badge: "CAP",
        keywords: "截图 截屏 区域截图 标注 取色 screenshot capture snap cap shot jt",
        base_score: 9_850,
    },
    CaptureCmd {
        id: "capture:pin_clip",
        mode: "pin_clipboard",
        title: "将最近复制内容贴图置顶 (Pin Latest Clipboard)",
        subtitle: "支持图片、文件、色值与文本卡片 · 滚轮缩放 · Ctrl+滚轮调透明度 · Ctrl+C 复制原文",
        badge: "PIN",
        keywords: "贴图 置顶贴图 悬浮图片 文本贴图 色值贴图 pin tie tu clipboard f3",
        base_score: 9_760,
    },
    CaptureCmd {
        id: "capture:ocr",
        mode: "ocr",
        title: "截图识字 OCR (Capture & Extract Text)",
        subtitle: "框选屏幕任意区域后直接调用离线 OCR 并打开对照审校窗",
        badge: "OCR",
        keywords: "截图识字 截屏识别 提取文字 截图ocr jtocr capture ocr",
        base_score: 9_720,
    },
    CaptureCmd {
        id: "capture:translate",
        mode: "translate",
        title: "截图翻译 (Capture & Translate)",
        subtitle: "框选外文区域 → 离线 OCR 识字 → 自动打开多引擎双语对照翻译窗",
        badge: "TR",
        keywords: "截图翻译 截屏翻译 屏幕翻译 jtfy capture translate",
        base_score: 9_700,
    },
    CaptureCmd {
        id: "capture:record",
        mode: "record",
        title: "屏幕录制 MP4 / GIF (Record Screen)",
        subtitle: "智能预选窗口/区域 → 实时暂停继续 · 支持系统音/麦克风与高清调色板 GIF",
        badge: "REC",
        keywords: "录屏 屏幕录制 区域录屏 录制视频 动图 gif mp4 video record rec luping lp",
        base_score: 9_740,
    },
];

struct CapturePlugin;

impl TomoyoPlugin for CapturePlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("原生截图、贴图与录屏", env!("CARGO_PKG_VERSION"))
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let kw = ctx.keyword.as_deref().unwrap_or("").to_ascii_lowercase();
        let arg = ctx.args.trim().to_lowercase();
        let mut out = Vec::new();
        for cmd in COMMANDS {
            let score = match (kw.as_str(), cmd.mode) {
                ("pin", "pin_clipboard") => Some(cmd.base_score + 400),
                ("jtocr", "ocr") => Some(cmd.base_score + 400),
                ("jtfy", "translate") => Some(cmd.base_score + 400),
                ("rec" | "lp", "record") => Some(cmd.base_score + 400),
                ("cap" | "shot" | "jt" | "", _) => {
                    if arg.is_empty() {
                        Some(cmd.base_score)
                    } else if cmd.keywords.to_lowercase().contains(&arg)
                        || cmd.title.to_lowercase().contains(&arg)
                    {
                        Some(cmd.base_score + 180)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let Some(final_score) = score else {
                continue;
            };
            out.push(ResultItem {
                id: cmd.id.into(),
                provider_id: PLUGIN_ID.into(),
                title: cmd.title.into(),
                subtitle: cmd.subtitle.into(),
                category: ResultCategory::System,
                score: final_score,
                icon: IconSpec::Monogram(cmd.badge.into()),
                badge: Some(cmd.badge.into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: format!("run_{}", cmd.mode),
                    label: cmd.title.into(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::PluginExecute {
                        plugin_id: PLUGIN_ID.into(),
                        action_id: format!("run_{}", cmd.mode),
                        payload: json!({ "mode": cmd.mode, "from_launcher": true }),
                    },
                }],
                preview: Some(PreviewSpec::KeyValue {
                    title: cmd.title.into(),
                    entries: vec![
                        (
                            "标注快捷键".into(),
                            "R/E/A/B/H/M/N/X 标注 · C 取色 · V 录屏 · P 贴图 · O 识字 · T 翻译 · Ctrl+S 保存".into(),
                        ),
                        ("内存原则".into(), "独立按需子进程，用完即退出（0 MB 常驻）".into()),
                    ],
                }),
            });
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.score));
        out
    }

    fn execute(&self, req: &ExecuteRequest) -> ExecuteResponse {
        if req.action_id == "pin_image" {
            let path = req.payload.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() || !Path::new(path).is_file() {
                return ExecuteResponse { toast: Some("图片文件不存在".into()), ..Default::default() };
            }
            return match spawn_capture_args(&["--pin", path], false) {
                Ok(()) => ExecuteResponse { hide_window: true, ..Default::default() },
                Err(e) => ExecuteResponse { toast: Some(format!("贴图启动失败: {e}")), ..Default::default() },
            };
        }

        let mode = req
            .payload
            .get("mode")
            .and_then(|v| v.as_str())
            .or_else(|| req.action_id.strip_prefix("run_"))
            .unwrap_or("region");
        let delay = req
            .payload
            .get("from_launcher")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        launch_capture_mode(mode, delay)
    }

    fn on_event(&self, ev: &PluginEventPayload) -> ExecuteResponse {
        if ev.event == PluginEventKind::HotkeyTriggered {
            let mode = ev.data.get("hotkey_id").and_then(|v| v.as_str()).unwrap_or("region");
            return launch_capture_mode(mode, false);
        }
        ExecuteResponse::default()
    }
}

fn launch_capture_mode(mode: &str, delay_for_launcher_hide: bool) -> ExecuteResponse {
    let flag = match mode {
        "ocr" => "--ocr",
        "translate" => "--translate",
        "record" => "--record",
        "pin_clipboard" | "pin" => "--pin-clipboard",
        _ => "--capture",
    };
    match spawn_capture_args(&[flag], delay_for_launcher_hide) {
        Ok(()) => ExecuteResponse { hide_window: true, ..Default::default() },
        Err(e) => ExecuteResponse {
            toast: Some(format!("截图工具启动失败: {e}")),
            ..Default::default()
        },
    }
}

fn locate_capture_bin() -> Result<PathBuf, String> {
    let bin_name = if cfg!(windows) { "tomoyo-capture.exe" } else { "tomoyo-capture" };
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sib = dir.join(bin_name);
        if sib.is_file() {
            return Ok(sib);
        }
        if let Some(p) = dir.parent() {
            let up = p.join(bin_name);
            if up.is_file() {
                return Ok(up);
            }
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for d in std::env::split_paths(&path_var) {
            let cand = d.join(bin_name);
            if cand.is_file() {
                return Ok(cand);
            }
        }
    }
    Err(format!("未找到 {bin_name}"))
}

fn spawn_capture_args(args: &[&str], delay_ms_75: bool) -> Result<(), String> {
    let exe = locate_capture_bin()?;
    let owned_args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    if delay_ms_75 {
        let _ = std::thread::Builder::new()
            .name("tomoyo-cap-delay".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(75));
                if let Ok(mut child) = Command::new(&exe).args(&owned_args).spawn() {
                    let _ = child.wait();
                }
            });
        return Ok(());
    }
    let mut child = Command::new(&exe)
        .args(&owned_args)
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", exe.display()))?;
    let _ = std::thread::Builder::new()
        .name("tomoyo-cap-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

fn main() {
    if std::env::args().any(|a| a == "--self-test") {
        let p = CapturePlugin;
        let mut ctx = QueryContext::new("cap", 1, 10);
        ctx.keyword = Some("cap".into());
        ctx.args.clear();
        assert_eq!(p.query(&ctx).len(), 5);
        println!("[OK] tomoyo-plugin-capture v{} self-test passed", env!("CARGO_PKG_VERSION"));
        return;
    }
    tomoyo_sdk::run(CapturePlugin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_capture_keywords_cleanly() {
        let mut ctx = QueryContext::new("rec", 1, 10);
        ctx.keyword = Some("rec".into());
        ctx.args.clear();
        let hits = CapturePlugin.query(&ctx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "capture:record");
    }
}
