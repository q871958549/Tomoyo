//! macOS native backend adapter (`.app` bundle discovery, Spotlight `mdfind` & Cocoa NSPanel host).

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
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let Ok(out) = std::process::Command::new("mdfind")
            .arg("-name")
            .arg(trimmed)
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(limit)
            .map(ToString::to_string)
            .collect()
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
}
