//! Linux Wayland/X11 backend adapter (XDG `.desktop` parser, `fd`/`locate` search & `xdg-open`).

use std::path::Path;
use tomoyo_ui_shared::{DiscoveredDesktopApp, PlatformBackend};

pub struct LinuxBackend {
    xdg_dirs: Vec<String>,
}

impl LinuxBackend {
    pub fn new() -> Self {
        let mut dirs = vec![
            "/usr/share/applications".to_string(),
            "/usr/local/share/applications".to_string(),
            "/var/lib/flatpak/exports/share/applications".to_string(),
        ];
        if let Ok(home) = std::env::var("HOME") {
            dirs.insert(0, format!("{home}/.local/share/applications"));
        }
        Self { xdg_dirs: dirs }
    }

    pub fn parse_desktop_entry(id_stem: &str, content: &str) -> Option<DiscoveredDesktopApp> {
        let mut in_main_section = false;
        let mut name = None;
        let mut exec = None;
        let mut no_display = false;
        let mut hidden = false;

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                in_main_section = line == "[Desktop Entry]";
                continue;
            }
            if !in_main_section {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                match k.trim() {
                    "Name" if name.is_none() => name = Some(v.trim().to_string()),
                    "Exec" => exec = Some(clean_desktop_exec(v.trim())),
                    "NoDisplay" => no_display = v.trim().eq_ignore_ascii_case("true"),
                    "Hidden" => hidden = v.trim().eq_ignore_ascii_case("true"),
                    _ => {}
                }
            }
        }
        if no_display || hidden {
            return None;
        }
        let title = name?;
        let exec_cmd = exec?;
        if exec_cmd.is_empty() {
            return None;
        }
        Some(DiscoveredDesktopApp {
            id: format!("linux:xdg:{id_stem}"),
            title,
            subtitle: exec_cmd.clone(),
            exec: exec_cmd,
        })
    }

    pub fn scan_desktop_dir(dir: &Path) -> Vec<DiscoveredDesktopApp> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for ent in entries.flatten() {
            let p = ent.path();
            if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
            if let Ok(raw) = std::fs::read_to_string(&p)
                && let Some(app) = Self::parse_desktop_entry(stem, &raw) {
                    out.push(app);
                }
        }
        out.sort_by(|a, b| a.title.cmp(&b.title));
        out
    }
}

fn clean_desktop_exec(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|tok| !matches!(*tok, "%f" | "%F" | "%u" | "%U" | "%i" | "%c" | "%k"))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Default for LinuxBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBackend for LinuxBackend {
    fn platform_id(&self) -> &'static str {
        "linux-wayland-x11"
    }

    fn discover_desktop_apps(&self) -> Vec<DiscoveredDesktopApp> {
        let mut out = Vec::new();
        for dir in &self.xdg_dirs {
            out.extend(Self::scan_desktop_dir(Path::new(dir)));
        }
        out
    }

    #[cfg(target_os = "linux")]
    fn search_files_hint(&self, query: &str, limit: usize) -> Vec<String> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let cmd = std::process::Command::new("fd")
            .args(["--max-results", &limit.to_string(), trimmed])
            .output()
            .or_else(|_| std::process::Command::new("locate").args(["-n", &limit.to_string(), trimmed]).output());
        let Ok(out) = cmd else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(limit)
            .map(ToString::to_string)
            .collect()
    }

    #[cfg(not(target_os = "linux"))]
    fn search_files_hint(&self, _query: &str, _limit: usize) -> Vec<String> {
        Vec::new()
    }

    #[cfg(target_os = "linux")]
    fn open_target(&self, target: &str) -> Result<(), String> {
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("xdg-open failed: {e}"))
    }

    #[cfg(not(target_os = "linux"))]
    fn open_target(&self, _target: &str) -> Result<(), String> {
        Err("LinuxBackend::open_target requires Linux".into())
    }

    #[cfg(target_os = "linux")]
    fn copy_text(&self, text: &str) -> Result<(), String> {
        use std::io::Write;
        let mut child = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .or_else(|_| {
                std::process::Command::new("xclip")
                    .args(["-selection", "clipboard"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
            })
            .map_err(|e| format!("clipboard copy failed: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn copy_text(&self, _text: &str) -> Result<(), String> {
        Err("LinuxBackend::copy_text requires Linux".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xdg_desktop_entries_and_filters_hidden() {
        let visible = "[Desktop Entry]\nName=Neovim\nExec=nvim %F\nType=Application\n";
        let app = LinuxBackend::parse_desktop_entry("nvim", visible).unwrap();
        assert_eq!(app.title, "Neovim");
        assert_eq!(app.exec, "nvim");

        let hidden = "[Desktop Entry]\nName=InternalHelper\nExec=helper\nNoDisplay=true\n";
        assert!(LinuxBackend::parse_desktop_entry("helper", hidden).is_none());
    }
}
