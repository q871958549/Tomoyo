//! macOS native backend adapter (`.app` bundle discovery, Spotlight `mdfind`,
//! `pbcopy`/`pbpaste`, Finder reveal & LaunchAgent `.plist` autostart generator).

pub mod bridge;
pub mod panel;
pub use bridge::*;
pub use panel::*;

use std::path::Path;
use tomoyo_ui_shared::{DiscoveredDesktopApp, PlatformBackend};

pub struct MacOsBackend {
    app_roots: Vec<String>,
}

impl MacOsBackend {
    pub fn new() -> Self {
        let mut roots = vec![
            "/Applications".to_string(),
            "/System/Applications".to_string(),
        ];
        if let Ok(home) = std::env::var("HOME") {
            roots.push(format!("{home}/Applications"));
        }
        Self { app_roots: roots }
    }

    pub fn scan_app_bundles_in(dir: &Path) -> Vec<DiscoveredDesktopApp> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut apps = Vec::new();
        for ent in entries.flatten() {
            let p = ent.path();
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.ends_with(".app") {
                let stem = name.trim_end_matches(".app").to_string();
                let exec = p.to_string_lossy().into_owned();
                apps.push(DiscoveredDesktopApp {
                    id: format!("macos:app:{}", stem.to_ascii_lowercase()),
                    title: stem,
                    subtitle: exec.clone(),
                    exec,
                });
            }
        }
        apps.sort_by(|a, b| a.title.cmp(&b.title));
        apps
    }

    /// Builds sanitized `mdfind` arguments preventing flag injection from leading `-`.
    pub fn build_spotlight_args(query: &str, scope_dir: Option<&str>) -> Option<Vec<String>> {
        let trimmed = query.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            return None;
        }
        let mut args = Vec::with_capacity(4);
        if let Some(dir) = scope_dir.map(str::trim).filter(|d| !d.is_empty()) {
            args.push("-onlyin".into());
            args.push(dir.into());
        }
        args.push("-name".into());
        args.push(trimmed.trim_start_matches('-').to_string());
        if args.last().is_some_and(|s| s.is_empty()) {
            return None;
        }
        Some(args)
    }

    /// Parses newline-delimited `mdfind` output into bounded paths.
    pub fn parse_spotlight_output(raw: &str, limit: usize) -> Vec<String> {
        raw.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.starts_with('/'))
            .take(limit)
            .map(ToString::to_string)
            .collect()
    }

    /// Renders a valid per-user macOS LaunchAgent `.plist` (`~/Library/LaunchAgents/<label>.plist`).
    pub fn render_launch_agent_plist(exe_path: &str, label: &str) -> String {
        let esc = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
               <key>Label</key>\n\
               <string>{}</string>\n\
               <key>ProgramArguments</key>\n\
               <array>\n\
                 <string>{}</string>\n\
                 <string>--silent</string>\n\
               </array>\n\
               <key>RunAtLoad</key>\n\
               <true/>\n\
               <key>KeepAlive</key>\n\
               <false/>\n\
               <key>ProcessType</key>\n\
               <string>Interactive</string>\n\
             </dict>\n\
             </plist>\n",
            esc(label),
            esc(exe_path)
        )
    }

    #[cfg(target_os = "macos")]
    pub fn read_clipboard_text(&self) -> Result<String, String> {
        let out = std::process::Command::new("pbpaste")
            .output()
            .map_err(|e| format!("pbpaste failed: {e}"))?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn read_clipboard_text(&self) -> Result<String, String> {
        Err("MacOsBackend::read_clipboard_text requires macOS".into())
    }

    #[cfg(target_os = "macos")]
    pub fn reveal_in_finder(&self, path: &str) -> Result<(), String> {
        std::process::Command::new("open")
            .args(["-R", path])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("open -R failed: {e}"))
    }

    #[cfg(not(target_os = "macos"))]
    pub fn reveal_in_finder(&self, _path: &str) -> Result<(), String> {
        Err("MacOsBackend::reveal_in_finder requires macOS".into())
    }
}

impl Default for MacOsBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBackend for MacOsBackend {
    fn platform_id(&self) -> &'static str {
        "macos-appkit"
    }

    fn discover_desktop_apps(&self) -> Vec<DiscoveredDesktopApp> {
        let mut out = Vec::new();
        for root in &self.app_roots {
            out.extend(Self::scan_app_bundles_in(Path::new(root)));
        }
        out
    }

    #[cfg(target_os = "macos")]
    fn search_files_hint(&self, query: &str, limit: usize) -> Vec<String> {
        let Some(args) = Self::build_spotlight_args(query, None) else {
            return Vec::new();
        };
        let Ok(out) = std::process::Command::new("mdfind").args(args).output() else {
            return Vec::new();
        };
        Self::parse_spotlight_output(&String::from_utf8_lossy(&out.stdout), limit)
    }

    #[cfg(not(target_os = "macos"))]
    fn search_files_hint(&self, _query: &str, _limit: usize) -> Vec<String> {
        Vec::new()
    }

    #[cfg(target_os = "macos")]
    fn open_target(&self, target: &str) -> Result<(), String> {
        std::process::Command::new("open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("open failed: {e}"))
    }

    #[cfg(not(target_os = "macos"))]
    fn open_target(&self, _target: &str) -> Result<(), String> {
        Err("MacOsBackend::open_target requires macOS".into())
    }

    #[cfg(target_os = "macos")]
    fn copy_text(&self, text: &str) -> Result<(), String> {
        use std::io::Write;
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("pbcopy failed: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn copy_text(&self, _text: &str) -> Result<(), String> {
        Err("MacOsBackend::copy_text requires macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_synthetic_macos_app_bundles() {
        let tmp = std::env::temp_dir().join(format!("tomoyo-mac-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Raycast.app")).unwrap();
        std::fs::create_dir_all(tmp.join("Notes.app")).unwrap();
        std::fs::write(tmp.join("readme.txt"), "skip").unwrap();
        let apps = MacOsBackend::scan_app_bundles_in(&tmp);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].title, "Notes");
        assert_eq!(apps[1].title, "Raycast");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn builds_safe_spotlight_args_and_parses_results() {
        assert!(MacOsBackend::build_spotlight_args("   ", None).is_none());
        assert!(MacOsBackend::build_spotlight_args("---", None).is_none());
        let args = MacOsBackend::build_spotlight_args("manual.md", Some("/Users/demo")).unwrap();
        assert_eq!(args, vec!["-onlyin", "/Users/demo", "-name", "manual.md"]);

        let parsed = MacOsBackend::parse_spotlight_output("/Users/demo/a.md\nrelative/skip\n/Users/demo/b.md\n", 1);
        assert_eq!(parsed, vec!["/Users/demo/a.md"]);
    }

    #[test]
    fn renders_valid_launch_agent_plist_with_xml_escaping() {
        let plist = MacOsBackend::render_launch_agent_plist(
            "/Applications/Tomoyo & Tools/tomoyo",
            "dev.tomoyo.launcher",
        );
        assert!(plist.contains("<string>dev.tomoyo.launcher</string>"));
        assert!(plist.contains("<string>/Applications/Tomoyo &amp; Tools/tomoyo</string>"));
        assert!(plist.contains("<string>--silent</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
    }
}
