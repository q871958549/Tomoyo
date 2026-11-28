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
                        remediation: remediation_for_app(&p.name_lower, spec),
                        can_terminate: is_safe_to_terminate(&p.name_lower),
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
                    remediation: remediation_for_app(&p.name_lower, spec),
                    can_terminate: is_safe_to_terminate(&p.name_lower),
                });
            }
    }
    out
}

pub(crate) fn remediation_for_app(name_lower: &str, spec: &str) -> String {
    if name_lower.contains("wetype") {
        format!("打开微信输入法「设置 → 快捷键」，取消或修改 {spec}（如剪贴板/常用语快捷键），保存后点击「重新扫描热键」即可彻底释放系统占位。")
    } else if name_lower.contains("wechat") || name_lower.contains("weixin") {
        format!("打开微信「设置 → 快捷键」，修改或清空 {spec} 绑定。")
    } else if name_lower.contains("qq.exe") {
        format!("打开 QQ「设置 → 热键 → 设置热键」，取消或更换 {spec}。")
    } else if name_lower.contains("nvdisplay") || name_lower.contains("nvcontainer") || name_lower.contains("nvidia") {
        format!("按 Alt+Z 打开 NVIDIA 浮层 → 设置 → 快捷键，解绑 {spec} 或关闭游戏内覆盖。")
    } else if name_lower.contains("powertoys") {
        format!("打开 Microsoft PowerToys，关闭对应模块的 {spec} 激活快捷键。")
    } else if name_lower.contains("pixpin") || name_lower.contains("snipaste") || name_lower.contains("sharex") {
        format!("打开该截图工具的「偏好设置 → 快捷键」，取消或更改 {spec}。")
    } else if name_lower.contains("copyq") || name_lower.contains("ditto") {
        format!("打开该剪贴板软件的全局快捷键设置，解绑 {spec} 或退出该进程。")
    } else if name_lower.contains("dingtalk") || name_lower.contains("feishu") || name_lower.contains("lark") {
        format!("打开该办公软件「设置 → 快捷键」，关闭或修改 {spec}。")
    } else {
        format!("请在 {name_lower} 的偏好设置中修改/关闭 {spec} 快捷键，或点击下方「结束进程」临时释放并重扫。")
    }
}

pub(crate) fn is_safe_to_terminate(name_lower: &str) -> bool {
    !is_background_noise_process(name_lower)
        && !matches!(
            name_lower,
            "lsass.exe" | "winlogon.exe" | "wininit.exe" | "services.exe" | "ctfmon.exe" | "sihost.exe"
        )
}

/// Reveals and selects `path` in File Explorer via native `SHOpenFolderAndSelectItems`,
/// eliminating `explorer.exe /select,` quoting bugs on paths containing spaces (`Program Files`).
pub(crate) fn reveal_path_native(path: &str) {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};

    let p = std::path::Path::new(path);
    if path.is_empty() || !p.exists() {
        return;
    }
    if tomoyo_core::reveal_in_custom_file_manager(path) {
        return;
    }
    let normalized = path.replace('/', "\\");
    let wide = HSTRING::from(&normalized);
    unsafe {
        let pidl = ILCreateFromPathW(PCWSTR(wide.as_ptr()));
        if !pidl.is_null() {
            let hr = SHOpenFolderAndSelectItems(pidl, None, 0);
            ILFree(Some(pidl));
            if hr.is_ok() {
                return;
            }
        }
    }
    if let Some(parent) = p.parent().filter(|d| d.is_dir()) {
        crate::bridge_os::shell_open(&parent.to_string_lossy());
    }
}

/// Safely terminates a user-space process holding a conflicted shortcut after verifying PID + exe basename.
pub(crate) fn terminate_conflict_process_safe(pid: u32, expected_name: &str) -> Result<String, String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    if pid <= 4 || pid == std::process::id() {
        return Err("拒绝结束系统关键进程或自身进程".into());
    }
    let lower_expected = expected_name.trim().to_ascii_lowercase();
    if lower_expected.is_empty() || !is_safe_to_terminate(&lower_expected) {
        return Err(format!("安全保护：禁止结束受保护进程 {expected_name}"));
    }
    unsafe {
        let h_q = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map_err(|_| format!("进程 {expected_name} (PID {pid}) 已退出或无权访问"))?;
        let mut buf = vec![0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h_q, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h_q);
        let actual_path = if ok && len > 0 { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() };
        let actual_name = std::path::Path::new(&actual_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if actual_name != lower_expected {
            return Err(format!("PID {pid} 对应进程已变化（{actual_name}），已取消操作"));
        }
        let h_t = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|_| format!("权限不足，无法结束 {expected_name}（可能需要管理员权限或在软件内关闭快捷键）"))?;
        let res = TerminateProcess(h_t, 0);
        let _ = CloseHandle(h_t);
        if res.is_err() {
            return Err(format!("结束 {expected_name} (PID {pid}) 失败"));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(140));
    Ok(format!("已结束进程 {expected_name} (PID {pid})，正在重新扫描热键…"))
}
