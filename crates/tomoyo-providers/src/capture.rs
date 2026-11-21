//! Built-in on-demand native Screenshot, Annotation, Color Picker & Pin-to-Desktop provider.
//!
//! Spawns the ephemeral `tomoyo-capture.exe` helper strictly on demand (0 idle RAM).

use std::sync::{Arc, RwLock};
use tomoyo_core::{score_match, IndexedText, Lang, Provider, TomoyoConfig};
use tomoyo_protocol::{
    ActionKind, ActionSpec, IconSpec, InternalCommandKind, PreviewSpec, QueryContext,
    ResultCategory, ResultItem, SearchFilter,
};

struct CaptureCmd {
    id: &'static str,
    mode: &'static str,
    title_zh: &'static str,
    title_en: &'static str,
    sub_zh: &'static str,
    sub_en: &'static str,
    badge: &'static str,
    indexed: IndexedText,
    base_score: i64,
}

pub struct CaptureProvider {
    config: Arc<RwLock<TomoyoConfig>>,
    commands: Vec<CaptureCmd>,
}

impl CaptureProvider {
    pub fn new(config: Arc<RwLock<TomoyoConfig>>) -> Self {
        let specs = [
            (
                "capture:region",
                "region",
                "区域截图与标注 (Screenshot & Annotate)",
                "Region Screenshot & Annotate",
                "多屏 DPI 精准选区 · 窗口吸附 · 取色放大镜 (C) · 框/箭/画笔/马赛克/贴图",
                "Multi-monitor DPI capture · Window snap · Color picker (C) · Annotate & Pin",
                "CAP",
                "截图 截屏 区域截图 标注 取色 screenshot capture snap cap shot jt",
                9_850,
            ),
            (
                "capture:pin_clip",
                "pin_clipboard",
                "将剪贴板图片贴图置顶 (Pin Clipboard Image)",
                "Pin Clipboard Image to Desktop",
                "创建无边框置顶悬浮贴图 · 滚轮缩放 · Ctrl+滚轮调透明度 · 右键 OCR/翻译",
                "Frameless topmost pin window · Wheel zoom · Ctrl+Wheel opacity · OCR/Translate",
                "PIN",
                "贴图 置顶贴图 悬浮图片 pin tie tu clipboard",
                9_760,
            ),
            (
                "capture:ocr",
                "ocr",
                "截图识字 OCR (Capture & Extract Text)",
                "Screenshot OCR (Extract Text)",
                "框选屏幕任意区域后直接调用本地 WinRT OCR 并打开对照审校窗",
                "Capture any region and extract text offline via WinRT OCR inspector",
                "OCR",
                "截图识字 截屏识别 提取文字 截图ocr jtocr capture ocr",
                9_720,
            ),
            (
                "capture:translate",
                "translate",
                "截图翻译 (Capture & Translate)",
                "Screenshot Translate",
                "框选外文区域 → 离线 OCR 识字 → 自动打开多引擎双语对照翻译窗",
                "Capture region → Offline OCR → Multi-engine parallel translation window",
                "TR",
                "截图翻译 截屏翻译 屏幕翻译 jtfy capture translate",
                9_700,
            ),
        ];

        let commands = specs
            .into_iter()
            .map(
                |(id, mode, title_zh, title_en, sub_zh, sub_en, badge, kw, base_score)| {
                    CaptureCmd {
                        id,
                        mode,
                        title_zh,
                        title_en,
                        sub_zh,
                        sub_en,
                        badge,
                        indexed: IndexedText::new(kw),
                        base_score,
                    }
                },
            )
            .collect();

        Self { config, commands }
    }

    fn lang_and_hk(&self) -> (Lang, String) {
        self.config
            .read()
            .map(|c| (c.lang(), c.capture.hotkey.clone()))
            .unwrap_or((Lang::Zh, "Alt+S".into()))
    }
}

impl Provider for CaptureProvider {
    fn id(&self) -> &'static str {
        "builtin.capture"
    }

    fn name(&self) -> &'static str {
        "Screenshot & Pin-to-Desktop"
    }

    fn prefixes(&self) -> &'static [&'static str] {
        &["cap", "shot", "jt", "pin", "jtocr", "jtfy"]
    }

    fn supports_global(&self) -> bool {
        true
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        if !matches!(ctx.filter, SearchFilter::All | SearchFilter::App) {
            return Vec::new();
        }
        let (lang, hk) = self.lang_and_hk();
        let dedicated_kw = ctx.keyword.as_deref().unwrap_or("").to_ascii_lowercase();
        let raw_trimmed = ctx.raw.trim();

        let mut out = Vec::new();
        for cmd in &self.commands {
            let score = if !dedicated_kw.is_empty() {
                match (dedicated_kw.as_str(), cmd.mode) {
                    ("pin", "pin_clipboard") => Some(cmd.base_score + 400),
                    ("jtocr", "ocr") => Some(cmd.base_score + 400),
                    ("jtfy", "translate") => Some(cmd.base_score + 400),
                    ("cap" | "shot" | "jt", _) => {
                        if ctx.args.trim().is_empty() {
                            Some(cmd.base_score)
                        } else {
                            score_match(ctx.args.trim(), &cmd.indexed).map(|s| cmd.base_score + s)
                        }
                    }
                    _ => None,
                }
            } else {
                if raw_trimmed.is_empty() {
                    return Vec::new();
                }
                score_match(raw_trimmed, &cmd.indexed).filter(|&s| s >= 120).map(|s| 8_200 + s)
            };

            let Some(final_score) = score else {
                continue;
            };
            let title = match lang {
                Lang::Zh => cmd.title_zh.to_string(),
                Lang::En => cmd.title_en.to_string(),
            };
            let mut subtitle = match lang {
                Lang::Zh => cmd.sub_zh.to_string(),
                Lang::En => cmd.sub_en.to_string(),
            };
            if cmd.mode == "region" && !hk.is_empty() {
                subtitle.push_str(&format!(" · 全局热键 {hk}"));
            }
            out.push(ResultItem {
                id: cmd.id.into(),
                provider_id: self.id().into(),
                title: title.clone(),
                subtitle: subtitle.clone(),
                category: ResultCategory::System,
                score: final_score,
                icon: IconSpec::Monogram(cmd.badge.into()),
                badge: Some(cmd.badge.into()),
                copy_text: None,
                actions: vec![ActionSpec {
                    id: format!("run_{}", cmd.mode),
                    label: title.clone(),
                    shortcut: Some("Enter".into()),
                    kind: ActionKind::InternalCommand {
                        command: InternalCommandKind::StartCapture {
                            mode: cmd.mode.into(),
                        },
                    },
                }],
                preview: Some(PreviewSpec::KeyValue {
                    title,
                    entries: vec![
                        (
                            lang.pick("全局截图热键", "Capture Hotkey").into(),
                            if hk.is_empty() {
                                lang.pick("未启用", "Disabled").into()
                            } else {
                                hk.clone()
                            },
                        ),
                        (
                            lang.pick("快捷键提示", "Shortcuts").into(),
                            "1-6 框/圆/箭/笔/马赛克/序号 · C 取色 · P 贴图 · O 识字 · T 翻译 · Ctrl+S 保存".into(),
                        ),
                        (
                            lang.pick("内存原则", "Idle Overhead").into(),
                            lang.pick("独立按需子进程，用完即退出（0 MB 常驻）", "Ephemeral on-demand process (0 MB idle)").into(),
                        ),
                    ],
                }),
            });
        }
        out.sort_by(|a, b| b.score.cmp(&a.score));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_provider_routes_prefixes_and_chinese_queries() {
        let cfg = Arc::new(RwLock::new(TomoyoConfig::default()));
        let prov = CaptureProvider::new(cfg);

        let ctx_cap = QueryContext {
            raw: "cap".into(),
            trimmed: "cap".into(),
            keyword: Some("cap".into()),
            args: String::new(),
            generation: 1,
            limit: 10,
            clipboard: None,
            attached: None,
            filter: SearchFilter::All,
        };
        assert_eq!(prov.query(&ctx_cap).len(), 4);

        let ctx_zh = QueryContext {
            raw: "截图翻译".into(),
            trimmed: "截图翻译".into(),
            keyword: None,
            args: String::new(),
            generation: 2,
            limit: 10,
            clipboard: None,
            attached: None,
            filter: SearchFilter::All,
        };
        let hits = prov.query(&ctx_zh);
        assert!(hits.iter().any(|it| it.id == "capture:translate"));
    }
}
