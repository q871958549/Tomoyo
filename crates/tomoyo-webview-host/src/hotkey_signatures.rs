//! Curated Windows desktop app hotkey signatures, local config parsers, and probe noise filters.

#![cfg(windows)]

use std::collections::HashSet;

#[derive(Clone)]
pub(crate) struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub name_lower: String,
    pub exe_path: String,
}

/// Curated reference table `(exe_substring_lower, display_label, default_hotkeys)`.
/// Only verified factory defaults are listed to prevent false attribution.
pub(crate) const KNOWN_SIGNATURES: &[(&str, &str, &[&str])] = &[
    ("wechat", "微信 (WeChat)", &["Alt+A", "Ctrl+Alt+W"]),
    ("weixin", "微信 (WeChat)", &["Alt+A", "Ctrl+Alt+W"]),
    ("wetype", "微信输入法 (WeType)", &["Alt+C", "Ctrl+Shift+V", "Alt+Space"]),
    ("qq.exe", "QQ", &["Ctrl+Alt+A", "Ctrl+Alt+Z"]),
    ("pixpin", "PixPin 截图", &["Ctrl+1", "F1", "Ctrl+2"]),
    ("snipaste", "Snipaste", &["F1", "F3", "Shift+F1"]),
    ("pot.exe", "Pot 翻译", &["Alt+T", "Alt+Q", "Alt+D"]),
    ("copyq", "CopyQ 剪贴板", &["Alt+V", "Ctrl+Shift+V"]),
    ("ditto", "Ditto 剪贴板", &["Ctrl+`", "Alt+V"]),
    ("utools", "uTools", &["Alt+Space"]),
    ("powertoys", "Microsoft PowerToys", &["Alt+Space", "Win+Shift+T", "Win+Shift+C"]),
    ("sharex", "ShareX", &["PrintScreen", "Ctrl+PrintScreen", "Alt+PrintScreen"]),
    ("flameshot", "Flameshot", &["PrintScreen"]),
    ("nvdisplay", "NVIDIA Overlay", &["Alt+Z", "Alt+R", "Alt+F1", "Alt+F9"]),
    ("nvcontainer", "NVIDIA App / Overlay", &["Alt+Z", "Alt+R", "Alt+F1", "Alt+F9"]),
    ("nvidia share", "NVIDIA Overlay", &["Alt+Z", "Alt+R", "Alt+F1"]),
    ("igcc", "Intel 显卡控制中心", &["Ctrl+Alt+F12", "Ctrl+Alt+F1"]),
    ("igfxhk", "Intel 显卡热键服务", &["Ctrl+Alt+F12", "Ctrl+Alt+F8"]),
    ("radeonsoftware", "AMD Adrenalin", &["Alt+R", "Alt+Z"]),
    ("dingtalk", "钉钉", &["Ctrl+Shift+A", "Alt+A"]),
    ("feishu", "飞书", &["Ctrl+Shift+A", "Alt+A"]),
    ("lark", "Lark / 飞书", &["Ctrl+Shift+A"]),
    ("quicker", "Quicker", &["Alt+Q", "Ctrl+Shift+V"]),
    ("listary", "Listary", &["Alt+Space", "Ctrl+Space"]),
    ("youdao", "有道词典", &["Ctrl+Alt+D", "Alt+T"]),
    ("eudic", "欧路词典", &["Ctrl+Alt+D", "Alt+T"]),
    ("goldendict", "GoldenDict", &["Ctrl+C+C", "Ctrl+F11"]),
    ("saladict", "沙拉查词", &["Alt+T", "Alt+C"]),
    ("clash-verge", "Clash Verge", &["Ctrl+Alt+P", "Ctrl+Alt+S"]),
    ("raylink", "RayLink 远程控制", &["Ctrl+Alt+F", "Ctrl+Alt+S"]),
    ("obs64", "OBS Studio", &["F9", "F10", "F11"]),
    ("discord", "Discord", &["Ctrl+Shift+M", "Ctrl+Shift+D"]),
    ("screenclippinghost", "Windows 截图工具", &["PrintScreen"]),
    ("snippingtool", "Windows 截图工具", &["PrintScreen"]),
];

/// Processes that burn CPU continuously (OS daemons, live wallpapers, IDE servers, containers)
/// and must never be falsely blamed by silent CPU-wake attribution.
pub(crate) fn is_background_noise_process(name_lower: &str) -> bool {
    matches!(
        name_lower,
        "system"
            | "registry"
            | "idle"
            | "system idle process"
            | "svchost.exe"
            | "csrss.exe"
            | "smss.exe"
            | "dwm.exe"
            | "explorer.exe"
            | "audiodg.exe"
            | "antimalware service executable"
            | "msmpeng.exe"
            | "wallpaper32.exe"
            | "wallpaper64.exe"
            | "wallpaperservice32.exe"
            | "conhost.exe"
            | "openconsole.exe"
            | "tomoyo.exe"
            | "tomoyo-webview-host.exe"
            | "vmmem"
            | "vmmemwsl"
            | "vmwp.exe"
            | "vmcompute.exe"
            | "searchindexer.exe"
            | "searchhost.exe"
            | "taskhostw.exe"
            | "runtimebroker.exe"
    ) || name_lower.contains("msedgewebview2")
        || name_lower.contains("language_server")
        || name_lower.contains("antigravity")
        || name_lower.contains("docker")
        || name_lower.contains("crashpad")
}

/// Reads bounded local config files ONLY for currently running third-party apps.
pub(crate) fn scan_known_local_configs(procs: &[ProcInfo]) -> Vec<(String, String, String)> {
    let mut hits = Vec::new();
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let candidates: &[(&str, &str, String)] = &[
        ("pot.exe", "Pot 本地配置", format!("{appdata}\\com.pot-app.desktop\\config.json")),
        ("copyq", "CopyQ 本地配置", format!("{appdata}\\copyq\\copyq-commands.ini")),
        ("pixpin", "PixPin 本地配置", format!("{appdata}\\PixPin\\Config\\config.ini")),
        ("snipaste", "Snipaste 本地配置", format!("{home}\\.snipaste\\config.ini")),
        ("clash-verge", "Clash Verge 配置", format!("{appdata}\\io.github.clash-verge-rev.clash-verge-rev\\verge.yaml")),
        ("everything", "Everything 配置", format!("{appdata}\\Everything\\Everything.ini")),
        ("raylink", "RayLink 热键配置", format!("{appdata}\\RayLink\\hotkey_config.json")),
    ];
    for (proc_key, label, path) in candidates {
        if !procs.iter().any(|p| p.name_lower.contains(proc_key)) {
            continue;
        }
        if std::fs::metadata(path).is_ok_and(|m| m.len() <= 262_144)
            && let Ok(raw) = std::fs::read_to_string(path) {
                hits.push(((*proc_key).to_string(), (*label).to_string(), raw.to_ascii_lowercase()));
            }
    }
    hits
}

pub(crate) fn guess_suspects(
    spec: &str,
    procs: &[ProcInfo],
    cfg_hits: &[(String, String, String)],
) -> Vec<crate::hotkey_doctor::SuspectProcess> {
    let mut out = Vec::new();
    let mut seen_pids = HashSet::new();
    let spec_lower = spec.to_ascii_lowercase();
    let spec_compact = spec_lower.replace('+', " + ");

    for (proc_key, label, content) in cfg_hits {
        if (content.contains(&spec_lower) || content.contains(&spec_compact))
            && let Some(p) = procs.iter().find(|pr| pr.name_lower.contains(proc_key))
                && seen_pids.insert(p.pid) {
                    out.push(crate::hotkey_doctor::SuspectProcess {
                        process_name: p.name.clone(),
                        pid: p.pid,
                        exe_path: p.exe_path.clone(),
                        reason: format!("{label}命中 {spec}（配置文件实锤）"),
                        confidence: "high",
                    });
                }
    }
    for &(key, label, defaults) in KNOWN_SIGNATURES {
        if !defaults.iter().any(|d| d.eq_ignore_ascii_case(spec)) {
            continue;
        }
        if let Some(p) = procs.iter().find(|pr| pr.name_lower.contains(key))
            && seen_pids.insert(p.pid) {
                out.push(crate::hotkey_doctor::SuspectProcess {
                    process_name: p.name.clone(),
                    pid: p.pid,
                    exe_path: p.exe_path.clone(),
                    reason: format!("{label} 常见默认键参考"),
                    confidence: "medium",
                });
            }
    }
    out
}
