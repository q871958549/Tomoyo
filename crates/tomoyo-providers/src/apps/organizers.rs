//! Auto-discovery of managed directories from Windows desktop organizers (DeskBox, PecoFence, etc.).

use std::path::PathBuf;

pub(super) fn detect_desktop_organizer_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // 1. DeskBox (`%LOCALAPPDATA%\DeskBox\data\*.json` and `%USERPROFILE%\DeskBox`)
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        let deskbox_data = PathBuf::from(&local_app).join("DeskBox").join("data");
        if let Ok(raw) = std::fs::read_to_string(deskbox_data.join("settings.json"))
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw)
                && let Some(root) = json
                    .get("defaultManagedStorageRootPath")
                    .and_then(|v| v.as_str())
                {
                    push_existing_dir(&mut dirs, PathBuf::from(root));
                }
        if let Ok(raw) = std::fs::read_to_string(deskbox_data.join("widget-layout.json"))
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw)
                && let Some(widgets) = json
                    .get("layout")
                    .and_then(|l| l.get("widgets"))
                    .and_then(|w| w.as_array())
                {
                    for w in widgets {
                        if let Some(mapped) = w.get("mappedFolderPath").and_then(|v| v.as_str()) {
                            push_existing_dir(&mut dirs, PathBuf::from(mapped));
                        }
                    }
                }
    }

    // 2. PecoFence (`%APPDATA%\PecoFence\config.json` -> desktopPath & folder portal fences)
    if let Ok(appdata) = std::env::var("APPDATA") {
        let pf_cfg = PathBuf::from(&appdata).join("PecoFence").join("config.json");
        if let Ok(raw) = std::fs::read_to_string(pf_cfg)
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(dp) = json
                    .get("settings")
                    .and_then(|s| s.get("desktopPath"))
                    .and_then(|v| v.as_str())
                {
                    push_existing_dir(&mut dirs, PathBuf::from(dp));
                }
                if let Some(fences) = json.get("fences").and_then(|v| v.as_object()) {
                    for f in fences.values() {
                        for key in ["folderPath", "portalPath", "targetPath", "path"] {
                            if let Some(p) = f.get(key).and_then(|v| v.as_str()) {
                                push_existing_dir(&mut dirs, PathBuf::from(p));
                            }
                        }
                    }
                }
            }
    }

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        push_existing_dir(&mut dirs, PathBuf::from(&userprofile).join("DeskBox"));
    }

    dirs
}

fn push_existing_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dir.is_dir() {
        return;
    }
    let key = dir.to_string_lossy().to_lowercase();
    if !dirs.iter().any(|d| d.to_string_lossy().to_lowercase() == key) {
        dirs.push(dir);
    }
}
